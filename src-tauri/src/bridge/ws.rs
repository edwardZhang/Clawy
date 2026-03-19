use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Extension, State};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;

use super::auth::RequestContext;
use super::gateway_adapter;
use super::response::ApiError;
use super::server::BridgeAppState;

pub(crate) async fn events_ws_handler(
    ws: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
    State(state): State<BridgeAppState>,
    Extension(context): Extension<RequestContext>,
) -> Response {
    match ws {
        Ok(upgrade) => upgrade
            .on_upgrade(move |socket| handle_events_socket(socket, context, state))
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
) {
    crate::append_log_line(
        "INFO",
        &format!(
            "Clawy Bridge WS connected: request_id={} caller_id={}",
            context.request_id,
            context.caller_id.as_deref().unwrap_or("unknown")
        ),
    );

    let mut events_rx = state.events_tx.subscribe();
    let runtime_snapshot = gateway_adapter::current_runtime_status_snapshot(
        &state.bridge_state,
        &state.config.node_id,
    );

    match serde_json::to_string(&runtime_snapshot) {
        Ok(payload) => {
            if socket.send(Message::Text(payload.into())).await.is_err() {
                return;
            }
        }
        Err(error) => {
            crate::append_log_line(
                "WARN",
                &format!("Failed to serialize initial runtime snapshot: {error}"),
            );
        }
    }

    loop {
        tokio::select! {
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
                    let payload = match serde_json::to_string(&event) {
                        Ok(payload) => payload,
                        Err(error) => {
                            crate::append_log_line(
                                "WARN",
                                &format!("Failed to serialize Clawy Bridge event: {error}"),
                            );
                            continue;
                        }
                    };

                    if socket.send(Message::Text(payload.into())).await.is_err() {
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
}
