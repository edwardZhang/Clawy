use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Extension, Query, State};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use serde::Deserialize;
use std::collections::HashSet;
use std::time::Duration;

use super::audit;
use super::auth::RequestContext;
use super::events::{self, EventReplayCursorStatus};
use super::gateway_adapter;
use super::response::ApiError;
use super::server::BridgeAppState;

const DEFAULT_WS_PING_INTERVAL_SECS: u64 = 20;
const MIN_WS_PING_INTERVAL_SECS: u64 = 5;
const MAX_WS_PING_INTERVAL_SECS: u64 = 120;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct EventsWsQuery {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default, rename = "type")]
    event_type: Option<String>,
    #[serde(default)]
    heartbeat_secs: Option<u64>,
    #[serde(default)]
    last_event_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct EventFilter {
    session_id: Option<String>,
    run_id: Option<String>,
    event_type: Option<String>,
}

pub(crate) async fn events_ws_handler(
    ws: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
    Query(query): Query<EventsWsQuery>,
    State(state): State<BridgeAppState>,
    Extension(context): Extension<RequestContext>,
) -> Response {
    match ws {
        Ok(upgrade) => upgrade
            .on_upgrade(move |socket| handle_events_socket(socket, context, state, query))
            .into_response(),
        Err(_) => {
            ApiError::invalid_request(&context, "This route requires a WebSocket upgrade request.")
                .into_response()
        }
    }
}

async fn handle_events_socket(
    mut socket: WebSocket,
    context: RequestContext,
    state: BridgeAppState,
    query: EventsWsQuery,
) {
    let filter = EventFilter {
        session_id: query.session_id.clone(),
        run_id: query.run_id.clone(),
        event_type: query.event_type.clone(),
    };
    let heartbeat_secs = normalize_heartbeat_secs(query.heartbeat_secs);

    crate::append_log_line(
        "INFO",
        &format!(
            "Clawy Bridge WS connected: request_id={} caller_id={} session_filter={} run_filter={} type_filter={} last_event_id={} heartbeat_secs={}",
            context.request_id,
            audit::redact_value(context.caller_id.as_deref()),
            audit::redact_value(filter.session_id.as_deref()),
            audit::redact_value(filter.run_id.as_deref()),
            filter.event_type.as_deref().unwrap_or("-"),
            audit::redact_value(query.last_event_id.as_deref()),
            heartbeat_secs,
        ),
    );

    let mut events_rx = state.events_tx.subscribe();
    let replay = events::replay_events_after(query.last_event_id.as_deref());
    let runtime_snapshot = gateway_adapter::current_runtime_status_snapshot(
        &state.bridge_state,
        &state.config.node_id,
    );
    let mut heartbeat = tokio::time::interval(Duration::from_secs(heartbeat_secs));
    heartbeat.tick().await;
    let mut replayed_event_ids = HashSet::new();
    let mut sent_initial_frame = false;

    if replay.cursor_status == EventReplayCursorStatus::NotFound {
        let replay_error = replay_cursor_error_event(&state, query.last_event_id.as_deref());
        if matches_filter(&replay_error, &filter)
            && send_event_frame(&mut socket, &replay_error).await.is_err()
        {
            return;
        }
        sent_initial_frame = true;
    }

    for event in replay.events {
        if !matches_filter(&event, &filter) {
            continue;
        }
        replayed_event_ids.insert(event.event_id.clone());
        if send_event_frame(&mut socket, &event).await.is_err() {
            return;
        }
        sent_initial_frame = true;
    }

    if !sent_initial_frame && matches_filter(&runtime_snapshot, &filter) {
        if send_event_frame(&mut socket, &runtime_snapshot)
            .await
            .is_err()
        {
            return;
        }
    }

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
            inbound = socket.next() => match inbound {
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(Message::Ping(payload))) => {
                    if socket.send(Message::Pong(payload)).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Text(_))) | Some(Ok(Message::Binary(_))) | Some(Ok(Message::Pong(_))) => {}
                Some(Err(error)) => {
                    crate::append_log_line(
                        "WARN",
                        &format!("Clawy Bridge WS receive error for {}: {error}", context.request_id),
                    );
                    break;
                }
            },
            outbound = events_rx.recv() => match outbound {
                Ok(event) => {
                    if replayed_event_ids.remove(&event.event_id) {
                        continue;
                    }

                    if !matches_filter(&event, &filter) {
                        continue;
                    }

                    if send_event_frame(&mut socket, &event).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    crate::append_log_line(
                        "WARN",
                        &format!("Clawy Bridge WS lagged and skipped {skipped} event(s)"),
                    );
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }

    crate::append_log_line(
        "INFO",
        &format!(
            "Clawy Bridge WS disconnected: request_id={}",
            context.request_id
        ),
    );
}

async fn send_event_frame(
    socket: &mut WebSocket,
    event: &crate::bridge::events::BridgeEventEnvelope,
) -> Result<(), ()> {
    let payload = match serde_json::to_string(event) {
        Ok(payload) => payload,
        Err(error) => {
            crate::append_log_line(
                "WARN",
                &format!("Failed to serialize Clawy Bridge event: {error}"),
            );
            return Ok(());
        }
    };

    socket
        .send(Message::Text(payload.into()))
        .await
        .map_err(|_| ())
}

fn replay_cursor_error_event(
    state: &BridgeAppState,
    last_event_id: Option<&str>,
) -> crate::bridge::events::BridgeEventEnvelope {
    let detail = last_event_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            format!("last_event_id `{value}` is no longer available in the in-memory replay buffer")
        })
        .unwrap_or_else(|| "requested replay cursor is not available".into());

    events::new_event(
        &state.config.node_id,
        None,
        None,
        "runtime.error",
        events::runtime_error_payload(
            events::bridge_error(
                "EVENT_REPLAY_CURSOR_EXPIRED",
                "Requested replay cursor is no longer available",
                Some(detail),
                "bridge",
                true,
            ),
            false,
        ),
    )
}

fn normalize_heartbeat_secs(value: Option<u64>) -> u64 {
    value
        .unwrap_or(DEFAULT_WS_PING_INTERVAL_SECS)
        .clamp(MIN_WS_PING_INTERVAL_SECS, MAX_WS_PING_INTERVAL_SECS)
}

fn matches_filter(
    event: &crate::bridge::events::BridgeEventEnvelope,
    filter: &EventFilter,
) -> bool {
    if filter
        .session_id
        .as_deref()
        .is_some_and(|session_id| event.session_id.as_deref() != Some(session_id))
    {
        return false;
    }

    if filter
        .run_id
        .as_deref()
        .is_some_and(|run_id| event.run_id.as_deref() != Some(run_id))
    {
        return false;
    }

    if filter
        .event_type
        .as_deref()
        .is_some_and(|event_type| event.event_type != event_type)
    {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::events;
    use crate::bridge::server::{spawn_server, BridgeRuntimeConfig};
    use serde_json::Value;
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::time::Duration;
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::Message as WsMessage;

    fn test_config() -> BridgeRuntimeConfig {
        BridgeRuntimeConfig {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
            auth_token: "bridge-test-token".into(),
            allowed_origins: Vec::new(),
            clawy_base_dir: PathBuf::from("."),
            node_id: "node_test".into(),
            openclaw_config_dir: PathBuf::from("."),
        }
    }

    #[tokio::test]
    async fn websocket_sends_runtime_snapshot_then_events() {
        events::reset_runtime_status_cache();
        let handle = spawn_server(None, crate::BridgeState::default(), test_config())
            .expect("bridge server should start");
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut request = format!("ws://{}/api/events", handle.local_addr)
            .into_client_request()
            .expect("websocket request should build");
        request.headers_mut().insert(
            "Authorization",
            "Bearer bridge-test-token"
                .parse()
                .expect("authorization header should parse"),
        );

        let (mut socket, _) = connect_async(request)
            .await
            .expect("websocket should connect");

        let first = socket
            .next()
            .await
            .expect("snapshot should arrive")
            .expect("snapshot frame should be ok");
        let WsMessage::Text(first_text) = first else {
            panic!("expected text snapshot frame");
        };
        let first_payload: Value =
            serde_json::from_str(first_text.as_ref()).expect("snapshot json should parse");
        assert_eq!(first_payload["type"], "runtime.status");
        assert_eq!(first_payload["payload"]["status"], "unavailable");

        handle
            .events_tx
            .send(events::new_event(
                "node_test",
                Some("agent:main:main".into()),
                Some("run_1".into()),
                "message.delta",
                serde_json::json!({
                    "role": "assistant",
                    "delta": "hello",
                }),
            ))
            .expect("event send should succeed");

        let second = socket
            .next()
            .await
            .expect("live event should arrive")
            .expect("live frame should be ok");
        let WsMessage::Text(second_text) = second else {
            panic!("expected text live frame");
        };
        let second_payload: Value =
            serde_json::from_str(second_text.as_ref()).expect("live json should parse");
        assert_eq!(second_payload["type"], "message.delta");
        assert_eq!(second_payload["run_id"], "run_1");
        assert_eq!(second_payload["payload"]["delta"], "hello");
    }

    #[tokio::test]
    async fn websocket_supports_session_filtering() {
        events::reset_runtime_status_cache();
        let handle = spawn_server(None, crate::BridgeState::default(), test_config())
            .expect("bridge server should start");
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut request = format!(
            "ws://{}/api/v1/events?session_id=agent:main:main&type=message.delta",
            handle.local_addr
        )
        .into_client_request()
        .expect("websocket request should build");
        request.headers_mut().insert(
            "Authorization",
            "Bearer bridge-test-token"
                .parse()
                .expect("authorization header should parse"),
        );

        let (mut socket, _) = connect_async(request)
            .await
            .expect("websocket should connect");
        tokio::time::sleep(Duration::from_millis(50)).await;

        handle
            .events_tx
            .send(events::new_event(
                "node_test",
                Some("agent:other:chat".into()),
                Some("run_skip".into()),
                "message.delta",
                serde_json::json!({ "delta": "skip" }),
            ))
            .expect("event send should succeed");

        handle
            .events_tx
            .send(events::new_event(
                "node_test",
                Some("agent:main:main".into()),
                Some("run_match".into()),
                "message.delta",
                serde_json::json!({ "delta": "match" }),
            ))
            .expect("event send should succeed");

        let frame = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("filtered event should arrive")
            .expect("socket should stay open")
            .expect("frame should be ok");

        let WsMessage::Text(text) = frame else {
            panic!("expected text frame");
        };
        let payload: Value =
            serde_json::from_str(text.as_ref()).expect("filtered json should parse");
        assert_eq!(payload["session_id"], "agent:main:main");
        assert_eq!(payload["run_id"], "run_match");
        assert_eq!(payload["type"], "message.delta");
    }

    #[tokio::test]
    async fn websocket_replays_events_after_last_event_id() {
        events::reset_runtime_status_cache();
        let handle = spawn_server(None, crate::BridgeState::default(), test_config())
            .expect("bridge server should start");
        tokio::time::sleep(Duration::from_millis(50)).await;

        let first = events::new_event(
            "node_test",
            Some("agent:main:main".into()),
            Some("run_1".into()),
            "message.delta",
            serde_json::json!({ "delta": "first" }),
        );
        let second = events::new_event(
            "node_test",
            Some("agent:main:main".into()),
            Some("run_1".into()),
            "message.delta",
            serde_json::json!({ "delta": "second" }),
        );
        events::remember_recent_event(&first);
        events::remember_recent_event(&second);

        let mut request = format!(
            "ws://{}/api/v1/events?last_event_id={}",
            handle.local_addr, first.event_id
        )
        .into_client_request()
        .expect("websocket request should build");
        request.headers_mut().insert(
            "Authorization",
            "Bearer bridge-test-token"
                .parse()
                .expect("authorization header should parse"),
        );

        let (mut socket, _) = connect_async(request)
            .await
            .expect("websocket should connect");

        let frame = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("replayed event should arrive")
            .expect("socket should stay open")
            .expect("frame should be ok");

        let WsMessage::Text(text) = frame else {
            panic!("expected text replay frame");
        };
        let payload: Value = serde_json::from_str(text.as_ref()).expect("replay json should parse");
        assert_eq!(payload["event_id"], second.event_id);
        assert_eq!(payload["payload"]["delta"], "second");
    }

    #[tokio::test]
    async fn websocket_reports_expired_replay_cursor() {
        events::reset_runtime_status_cache();
        let handle = spawn_server(None, crate::BridgeState::default(), test_config())
            .expect("bridge server should start");
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut request = format!(
            "ws://{}/api/v1/events?last_event_id=evt_missing",
            handle.local_addr
        )
        .into_client_request()
        .expect("websocket request should build");
        request.headers_mut().insert(
            "Authorization",
            "Bearer bridge-test-token"
                .parse()
                .expect("authorization header should parse"),
        );

        let (mut socket, _) = connect_async(request)
            .await
            .expect("websocket should connect");

        let frame = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("cursor error should arrive")
            .expect("socket should stay open")
            .expect("frame should be ok");

        let WsMessage::Text(text) = frame else {
            panic!("expected text replay error frame");
        };
        let payload: Value =
            serde_json::from_str(text.as_ref()).expect("replay error json should parse");
        assert_eq!(payload["type"], "runtime.error");
        assert_eq!(
            payload["payload"]["error"]["code"],
            "EVENT_REPLAY_CURSOR_EXPIRED"
        );
    }
}
