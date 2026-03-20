use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::AppHandle;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use super::events::{self, BridgeError, BridgeEventEnvelope, SessionSummary};

static ADAPTER_STARTED: OnceLock<()> = OnceLock::new();
static ACTIVE_RUNS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

type GatewaySocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

#[derive(Debug, Default)]
struct GatewayEventMapper {
    thinking_text_by_run: HashMap<String, String>,
    visible_text_by_run: HashMap<String, String>,
    tool_calls_sent: HashSet<String>,
    tool_results_sent: HashSet<String>,
}

#[derive(Debug, Clone)]
struct NormalizedGatewayEvent {
    session_id: Option<String>,
    run_id: Option<String>,
    state: Option<String>,
    phase: Option<String>,
    message: Option<Value>,
    model: Option<String>,
    thinking_level: Option<String>,
    error_detail: Option<String>,
}

#[derive(Debug, Clone)]
struct ToolCallRecord {
    tool_call_id: String,
    name: String,
    arguments: Value,
    summary: Option<String>,
}

#[derive(Debug, Clone)]
struct ToolResultRecord {
    tool_call_id: String,
    name: String,
    status: &'static str,
    summary: String,
    output_text: Option<String>,
}

fn active_runs() -> &'static Mutex<HashSet<String>> {
    ACTIVE_RUNS.get_or_init(|| Mutex::new(HashSet::new()))
}

pub(crate) fn start_gateway_event_adapter(app_handle: AppHandle, bridge_state: crate::BridgeState) {
    if ADAPTER_STARTED.set(()).is_err() {
        return;
    }

    tauri::async_runtime::spawn(async move {
        let mut mapper = GatewayEventMapper::default();

        loop {
            let port = crate::load_settings().gateway_port;
            match connect_gateway(&app_handle, port).await {
                Ok((mut socket, version)) => {
                    let _ = crate::gateway_mark_connected(
                        &app_handle,
                        &bridge_state,
                        &json!({ "version": version }),
                    );

                    if let Err(error) =
                        forward_gateway_stream(&app_handle, &bridge_state, &mut mapper, &mut socket)
                            .await
                    {
                        publish_runtime_error(&error, false);
                        let _ = crate::gateway_mark_disconnected(
                            &app_handle,
                            &bridge_state,
                            &json!({
                                "error": error,
                                "reconnecting": true,
                            }),
                        );
                    }
                }
                Err(error) => {
                    let snapshot =
                        crate::gateway_status_snapshot(&bridge_state).unwrap_or_default();
                    if snapshot.state != "stopped" {
                        publish_runtime_error(&error, false);
                        let _ = crate::gateway_mark_disconnected(
                            &app_handle,
                            &bridge_state,
                            &json!({
                                "error": error,
                                "reconnecting": true,
                            }),
                        );
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}

pub(crate) fn current_runtime_status_snapshot(
    bridge_state: &crate::BridgeState,
    node_id: &str,
) -> BridgeEventEnvelope {
    if let Some(snapshot) = events::cached_runtime_status() {
        return snapshot;
    }

    let gateway_status = crate::gateway_status_snapshot(bridge_state).unwrap_or_default();
    build_runtime_status_event(node_id, &gateway_status, None, None, None, None)
}

pub(crate) fn publish_gateway_status_snapshot(
    bridge_state: &crate::BridgeState,
    snapshot: &crate::GatewayStatus,
) {
    let Some(node_id) = resolve_node_id() else {
        return;
    };

    events::publish_event(build_runtime_status_event(
        &node_id, snapshot, None, None, None, None,
    ));
    let _ = bridge_state;
}

pub(crate) fn publish_runtime_error(detail: &str, fatal: bool) {
    let Some(node_id) = resolve_node_id() else {
        return;
    };

    let error = classify_runtime_error(detail);
    events::publish_event(events::new_event(
        &node_id,
        None,
        None,
        "runtime.error",
        events::runtime_error_payload(error, fatal),
    ));
}

async fn connect_gateway(
    app_handle: &AppHandle,
    port: u16,
) -> Result<(GatewaySocket, String), String> {
    let gateway_url = format!("ws://127.0.0.1:{port}/ws");
    let (mut socket, _) = connect_async(&gateway_url)
        .await
        .map_err(|error| format!("Failed to connect to Gateway WS at {gateway_url}: {error}"))?;

    let handshake_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut connect_request_id: Option<String> = None;

    loop {
        let remaining = handshake_deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err("Timed out waiting for connect.challenge from Gateway".into());
        }

        let frame = tokio::time::timeout(remaining, socket.next())
            .await
            .map_err(|_| "Timed out waiting for connect.challenge from Gateway".to_string())?
            .ok_or_else(|| "Gateway socket closed before connect handshake completed".to_string())?
            .map_err(|error| format!("Gateway WS handshake failed: {error}"))?;

        match frame {
            Message::Text(text) => {
                if let Some(nonce) = extract_connect_challenge_nonce(&text) {
                    let request_id = format!("connect-{}", Uuid::new_v4().simple());
                    let connect_params = crate::gateway_build_connect_params(
                        app_handle,
                        &json!({
                            "clientId": "gateway-client",
                            "clientMode": "ui",
                            "nonce": nonce,
                            "role": "operator",
                            "scopes": ["operator.admin"],
                        }),
                    )?;
                    let request = json!({
                        "type": "req",
                        "id": request_id,
                        "method": "connect",
                        "params": connect_params,
                    });

                    connect_request_id = Some(request_id);
                    socket
                        .send(Message::Text(request.to_string().into()))
                        .await
                        .map_err(|error| {
                            format!("Failed to send Gateway connect request: {error}")
                        })?;
                    continue;
                }

                if is_gateway_ping_text(&text) {
                    socket
                        .send(Message::Text(r#"{"method":"pong"}"#.into()))
                        .await
                        .map_err(|error| format!("Failed to respond to Gateway ping: {error}"))?;
                    continue;
                }

                if let Some(expected_id) = connect_request_id.as_deref() {
                    if let Some(version) = extract_connect_result(&text, expected_id)? {
                        return Ok((socket, version));
                    }
                }
            }
            Message::Ping(payload) => {
                socket
                    .send(Message::Pong(payload))
                    .await
                    .map_err(|error| format!("Failed to reply to Gateway ping frame: {error}"))?;
            }
            Message::Close(frame) => {
                let reason = frame
                    .map(|close_frame| close_frame.reason.to_string())
                    .unwrap_or_else(|| "no close reason".into());
                return Err(format!("Gateway socket closed before handshake ({reason})"));
            }
            Message::Binary(_) | Message::Pong(_) | Message::Frame(_) => {}
        }
    }
}

async fn forward_gateway_stream(
    app_handle: &AppHandle,
    bridge_state: &crate::BridgeState,
    mapper: &mut GatewayEventMapper,
    socket: &mut GatewaySocket,
) -> Result<(), String> {
    while let Some(frame) = socket.next().await {
        match frame.map_err(|error| format!("Gateway WS stream error: {error}"))? {
            Message::Text(text) => {
                if is_gateway_ping_text(&text) {
                    socket
                        .send(Message::Text(r#"{"method":"pong"}"#.into()))
                        .await
                        .map_err(|error| format!("Failed to reply to Gateway ping: {error}"))?;
                    continue;
                }

                for event in map_gateway_text_frame(text.as_ref(), bridge_state, mapper) {
                    events::publish_event(event);
                }
            }
            Message::Ping(payload) => {
                socket
                    .send(Message::Pong(payload))
                    .await
                    .map_err(|error| format!("Failed to reply to Gateway ping frame: {error}"))?;
            }
            Message::Close(frame) => {
                let reason = frame
                    .map(|close_frame| close_frame.reason.to_string())
                    .unwrap_or_else(|| "no close reason".into());
                return Err(format!("Gateway socket closed ({reason})"));
            }
            Message::Binary(_) | Message::Pong(_) | Message::Frame(_) => {}
        }
    }

    let _ = app_handle;
    Err("Gateway event stream ended".into())
}

fn map_gateway_text_frame(
    raw: &str,
    bridge_state: &crate::BridgeState,
    mapper: &mut GatewayEventMapper,
) -> Vec<BridgeEventEnvelope> {
    let Some(node_id) = resolve_node_id() else {
        return Vec::new();
    };

    let Ok(message) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };

    if message.get("type").and_then(Value::as_str) != Some("event") {
        return Vec::new();
    }

    let Some(event_name) = message.get("event").and_then(Value::as_str) else {
        return vec![protocol_error_event(
            &node_id,
            "Gateway event frame is missing `event`.".into(),
        )];
    };

    let payload = message.get("payload").cloned().unwrap_or(Value::Null);
    map_gateway_protocol_event(&node_id, bridge_state, mapper, event_name, &payload)
}

fn map_gateway_protocol_event(
    node_id: &str,
    bridge_state: &crate::BridgeState,
    mapper: &mut GatewayEventMapper,
    event_name: &str,
    payload: &Value,
) -> Vec<BridgeEventEnvelope> {
    let normalized = match normalize_protocol_event(event_name, payload) {
        Ok(Some(normalized)) => normalized,
        Ok(None) => return Vec::new(),
        Err(detail) => return vec![protocol_error_event(node_id, detail)],
    };

    let mut events_out = Vec::new();

    let lifecycle_phase = normalized
        .phase
        .clone()
        .or_else(|| match normalized.state.as_deref() {
            Some("started") => Some("started".into()),
            Some("aborted") => Some("aborted".into()),
            Some("error") => Some("error".into()),
            _ => None,
        });

    if let Some(phase) = lifecycle_phase.as_deref() {
        events_out.extend(map_lifecycle_phase(
            node_id,
            bridge_state,
            &normalized,
            mapper,
            phase,
        ));
    }

    if let Some(message) = normalized.message.as_ref() {
        events_out.extend(map_message_event(node_id, mapper, &normalized, message));
    } else if lifecycle_phase.is_some() {
        events_out.extend(close_thinking_if_needed(
            node_id,
            mapper,
            normalized.session_id.clone(),
            normalized.run_id.clone(),
        ));
    }

    if matches!(
        normalized.state.as_deref(),
        Some("final") | Some("aborted") | Some("error")
    ) || matches!(
        normalized.phase.as_deref(),
        Some("completed") | Some("done") | Some("finished") | Some("end") | Some("aborted")
    ) {
        events_out.extend(close_thinking_if_needed(
            node_id,
            mapper,
            normalized.session_id.clone(),
            normalized.run_id.clone(),
        ));
        clear_visible_text(
            mapper,
            normalized.session_id.as_deref(),
            normalized.run_id.as_deref(),
        );
    }

    events_out
}

fn map_lifecycle_phase(
    node_id: &str,
    bridge_state: &crate::BridgeState,
    normalized: &NormalizedGatewayEvent,
    mapper: &mut GatewayEventMapper,
    phase: &str,
) -> Vec<BridgeEventEnvelope> {
    let mut events_out = Vec::new();
    let session_id = normalized.session_id.clone();
    let run_id = normalized.run_id.clone();

    match phase {
        "started" => {
            if let Some(run_id_value) = run_id.as_deref() {
                set_run_active(run_id_value, true);
            }
            if let Some(session_event) = build_session_updated_event(
                node_id,
                session_id.clone(),
                run_id.clone(),
                "running",
                normalized.thinking_level.clone(),
                normalized.model.clone(),
            ) {
                events_out.push(session_event);
            }
            events_out.push(build_runtime_status_event(
                node_id,
                &crate::gateway_status_snapshot(bridge_state).unwrap_or_default(),
                session_id,
                run_id,
                Some("busy"),
                None,
            ));
        }
        "completed" | "done" | "finished" | "end" => {
            if let Some(run_id_value) = run_id.as_deref() {
                set_run_active(run_id_value, false);
            }
            if let Some(session_event) = build_session_updated_event(
                node_id,
                session_id.clone(),
                run_id.clone(),
                "completed",
                normalized.thinking_level.clone(),
                normalized.model.clone(),
            ) {
                events_out.push(session_event);
            }
            events_out.extend(close_thinking_if_needed(
                node_id,
                mapper,
                session_id.clone(),
                run_id.clone(),
            ));
            events_out.push(build_runtime_status_event(
                node_id,
                &crate::gateway_status_snapshot(bridge_state).unwrap_or_default(),
                session_id,
                run_id,
                Some("ready"),
                None,
            ));
        }
        "aborted" => {
            if let Some(run_id_value) = run_id.as_deref() {
                set_run_active(run_id_value, false);
            }
            if let Some(session_event) = build_session_updated_event(
                node_id,
                session_id.clone(),
                run_id.clone(),
                "aborted",
                normalized.thinking_level.clone(),
                normalized.model.clone(),
            ) {
                events_out.push(session_event);
            }
            events_out.extend(close_thinking_if_needed(
                node_id,
                mapper,
                session_id.clone(),
                run_id.clone(),
            ));
            events_out.push(build_runtime_status_event(
                node_id,
                &crate::gateway_status_snapshot(bridge_state).unwrap_or_default(),
                session_id,
                run_id,
                Some("ready"),
                None,
            ));
        }
        "error" | "failed" => {
            if let Some(run_id_value) = run_id.as_deref() {
                set_run_active(run_id_value, false);
            }
            let detail = normalized
                .error_detail
                .clone()
                .unwrap_or_else(|| "Gateway reported a run error".into());
            events_out.push(runtime_error_event(
                node_id,
                session_id.clone(),
                run_id.clone(),
                classify_runtime_error(&detail),
                false,
            ));
            if let Some(session_event) = build_session_updated_event(
                node_id,
                session_id.clone(),
                run_id.clone(),
                "error",
                normalized.thinking_level.clone(),
                normalized.model.clone(),
            ) {
                events_out.push(session_event);
            }
            events_out.extend(close_thinking_if_needed(
                node_id,
                mapper,
                session_id.clone(),
                run_id.clone(),
            ));
            events_out.push(build_runtime_status_event(
                node_id,
                &crate::gateway_status_snapshot(bridge_state).unwrap_or_default(),
                session_id,
                run_id,
                Some("degraded"),
                Some(detail),
            ));
        }
        _ => {}
    }

    events_out
}

fn map_message_event(
    node_id: &str,
    mapper: &mut GatewayEventMapper,
    normalized: &NormalizedGatewayEvent,
    message: &Value,
) -> Vec<BridgeEventEnvelope> {
    let mut events_out = Vec::new();
    let session_id = normalized.session_id.clone();
    let run_id = normalized.run_id.clone();

    let current_thinking = extract_thinking_text(message);
    events_out.extend(sync_thinking_events(
        node_id,
        mapper,
        session_id.clone(),
        run_id.clone(),
        current_thinking.as_deref(),
    ));

    for tool_call in extract_tool_calls(message) {
        let dedupe_key = format!(
            "{}:{}",
            run_id.clone().unwrap_or_else(|| "runless".into()),
            tool_call.tool_call_id
        );
        if mapper.tool_calls_sent.insert(dedupe_key) {
            events_out.push(events::new_event(
                node_id,
                session_id.clone(),
                run_id.clone(),
                "tool.call",
                json!({
                    "tool_call_id": tool_call.tool_call_id,
                    "name": tool_call.name,
                    "arguments": tool_call.arguments,
                    "summary": tool_call.summary,
                }),
            ));
        }
    }

    for tool_result in extract_tool_results(message) {
        let dedupe_key = format!(
            "{}:{}:{}",
            run_id.clone().unwrap_or_else(|| "runless".into()),
            tool_result.tool_call_id,
            tool_result.status
        );
        if mapper.tool_results_sent.insert(dedupe_key) {
            events_out.push(events::new_event(
                node_id,
                session_id.clone(),
                run_id.clone(),
                "tool.result",
                json!({
                    "tool_call_id": tool_result.tool_call_id,
                    "name": tool_result.name,
                    "status": tool_result.status,
                    "summary": tool_result.summary,
                    "output_text": tool_result.output_text,
                }),
            ));
        }
    }

    if normalized.state.as_deref() == Some("delta") {
        if let Some(delta) =
            next_visible_text_delta(mapper, session_id.as_deref(), run_id.as_deref(), message)
        {
            events_out.push(events::new_event(
                node_id,
                session_id.clone(),
                run_id.clone(),
                "message.delta",
                json!({
                    "role": "assistant",
                    "delta": delta,
                }),
            ));
        }
    }

    if normalized.state.as_deref() == Some("final") && !is_tool_only_message(message) {
        if let Some(final_message) = build_bridge_message(run_id.clone(), message) {
            events_out.extend(close_thinking_if_needed(
                node_id,
                mapper,
                session_id.clone(),
                run_id.clone(),
            ));
            events_out.push(events::new_event(
                node_id,
                session_id.clone(),
                run_id.clone(),
                "message.final",
                json!({ "message": final_message }),
            ));
        }
    }

    events_out
}

fn sync_thinking_events(
    node_id: &str,
    mapper: &mut GatewayEventMapper,
    session_id: Option<String>,
    run_id: Option<String>,
    current_thinking: Option<&str>,
) -> Vec<BridgeEventEnvelope> {
    let mut events_out = Vec::new();
    let Some(key) = run_cache_key(session_id.as_deref(), run_id.as_deref()) else {
        return events_out;
    };

    let previous = mapper
        .thinking_text_by_run
        .get(&key)
        .cloned()
        .unwrap_or_default();
    let current = current_thinking.unwrap_or("").trim().to_string();

    if current.is_empty() {
        if !previous.is_empty() {
            mapper.thinking_text_by_run.remove(&key);
            events_out.push(events::new_event(
                node_id,
                session_id,
                run_id,
                "message.thinking",
                json!({
                    "phase": "completed",
                    "delta": Value::Null,
                }),
            ));
        }
        return events_out;
    }

    if previous.is_empty() {
        events_out.push(events::new_event(
            node_id,
            session_id.clone(),
            run_id.clone(),
            "message.thinking",
            json!({
                "phase": "started",
                "delta": Value::Null,
            }),
        ));
    }

    let delta = if current.starts_with(&previous) {
        current[previous.len()..].to_string()
    } else {
        current.clone()
    };

    if !delta.trim().is_empty() {
        events_out.push(events::new_event(
            node_id,
            session_id.clone(),
            run_id.clone(),
            "message.thinking",
            json!({
                "phase": "delta",
                "delta": delta,
            }),
        ));
    }

    mapper.thinking_text_by_run.insert(key, current);
    events_out
}

fn close_thinking_if_needed(
    node_id: &str,
    mapper: &mut GatewayEventMapper,
    session_id: Option<String>,
    run_id: Option<String>,
) -> Vec<BridgeEventEnvelope> {
    let Some(key) = run_cache_key(session_id.as_deref(), run_id.as_deref()) else {
        return Vec::new();
    };

    if mapper.thinking_text_by_run.remove(&key).is_some() {
        return vec![events::new_event(
            node_id,
            session_id,
            run_id,
            "message.thinking",
            json!({
                "phase": "completed",
                "delta": Value::Null,
            }),
        )];
    }

    Vec::new()
}

fn next_visible_text_delta(
    mapper: &mut GatewayEventMapper,
    session_id: Option<&str>,
    run_id: Option<&str>,
    message: &Value,
) -> Option<String> {
    let key = run_cache_key(session_id, run_id)?;
    let current = extract_visible_text(message)?;
    let previous = mapper
        .visible_text_by_run
        .get(&key)
        .cloned()
        .unwrap_or_default();

    let delta = if current.starts_with(&previous) {
        current[previous.len()..].to_string()
    } else {
        current.clone()
    };

    mapper.visible_text_by_run.insert(key, current);

    if delta.trim().is_empty() {
        None
    } else {
        Some(delta)
    }
}

fn clear_visible_text(
    mapper: &mut GatewayEventMapper,
    session_id: Option<&str>,
    run_id: Option<&str>,
) {
    if let Some(key) = run_cache_key(session_id, run_id) {
        mapper.visible_text_by_run.remove(&key);
    }
}

fn normalize_protocol_event(
    event_name: &str,
    payload: &Value,
) -> Result<Option<NormalizedGatewayEvent>, String> {
    match event_name {
        "tick" | "heartbeat" | "channel.status" => Ok(None),
        "chat" => normalize_chat_event(payload).map(Some),
        "agent" => normalize_agent_event(payload).map(Some),
        _ => Ok(None),
    }
}

fn normalize_chat_event(payload: &Value) -> Result<NormalizedGatewayEvent, String> {
    let object = payload.as_object();
    let looks_wrapped = object
        .map(|record| {
            record.contains_key("state")
                || record.contains_key("message")
                || record.contains_key("sessionKey")
                || record.contains_key("runId")
        })
        .unwrap_or(false);

    let (session_id, run_id, state, message, error_detail, model, thinking_level) = if looks_wrapped
    {
        let record = object.expect("wrapped payload must be an object");
        (
            canonical_session_id(record.get("sessionKey"))?,
            optional_string(record.get("runId")),
            optional_string(record.get("state")),
            record
                .get("message")
                .cloned()
                .or_else(|| payload_looks_like_message(payload).then(|| payload.clone())),
            optional_string(record.get("errorMessage"))
                .or_else(|| optional_string(record.get("error"))),
            optional_string(record.get("model")).or_else(|| optional_string(record.get("modelId"))),
            optional_string(record.get("thinkingLevel"))
                .or_else(|| optional_string(record.get("thinking_level"))),
        )
    } else {
        (
            canonical_session_id(None)?,
            None,
            infer_message_state(payload).map(str::to_string),
            payload_looks_like_message(payload).then(|| payload.clone()),
            None,
            optional_string(payload.get("model"))
                .or_else(|| optional_string(payload.get("modelId"))),
            optional_string(payload.get("thinkingLevel"))
                .or_else(|| optional_string(payload.get("thinking_level"))),
        )
    };

    Ok(NormalizedGatewayEvent {
        session_id,
        run_id,
        state,
        phase: None,
        message,
        model,
        thinking_level,
        error_detail,
    })
}

fn normalize_agent_event(payload: &Value) -> Result<NormalizedGatewayEvent, String> {
    let record = payload
        .as_object()
        .ok_or_else(|| "Gateway agent payload is not an object".to_string())?;
    let data = record
        .get("data")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let session_id =
        canonical_session_id(record.get("sessionKey").or_else(|| data.get("sessionKey")))?;
    let run_id = optional_string(record.get("runId").or_else(|| data.get("runId")));
    let state = optional_string(record.get("state").or_else(|| data.get("state")));
    let phase = optional_string(record.get("phase").or_else(|| data.get("phase")));
    let message = record
        .get("message")
        .cloned()
        .or_else(|| data.get("message").cloned());
    let error_detail = optional_string(
        record
            .get("errorMessage")
            .or_else(|| data.get("errorMessage")),
    )
    .or_else(|| optional_string(record.get("error").or_else(|| data.get("error"))));
    let model = optional_string(record.get("model").or_else(|| data.get("model")))
        .or_else(|| optional_string(record.get("modelId").or_else(|| data.get("modelId"))));
    let thinking_level = optional_string(
        record
            .get("thinkingLevel")
            .or_else(|| data.get("thinkingLevel")),
    )
    .or_else(|| {
        optional_string(
            record
                .get("thinking_level")
                .or_else(|| data.get("thinking_level")),
        )
    });

    Ok(NormalizedGatewayEvent {
        session_id,
        run_id,
        state,
        phase,
        message,
        model,
        thinking_level,
        error_detail,
    })
}

fn canonical_session_id(raw: Option<&Value>) -> Result<Option<String>, String> {
    let Some(raw_value) = raw else {
        return Ok(None);
    };
    let Some(session_key) = raw_value.as_str() else {
        return Err("Gateway sessionKey is not a string".into());
    };

    let session_key = session_key.trim();
    if session_key.is_empty() {
        return Ok(None);
    }

    let parts = session_key.split(':').collect::<Vec<_>>();
    if parts.len() < 3 || parts[0] != "agent" || parts.iter().any(|part| part.is_empty()) {
        return Err(format!(
            "Gateway sessionKey `{session_key}` is not canonical"
        ));
    }

    Ok(Some(session_key.to_string()))
}

fn build_runtime_status_event(
    node_id: &str,
    snapshot: &crate::GatewayStatus,
    session_id: Option<String>,
    run_id: Option<String>,
    override_status: Option<&str>,
    detail_override: Option<String>,
) -> BridgeEventEnvelope {
    let gateway_running = snapshot.pid.is_some()
        || matches!(
            snapshot.state.as_str(),
            "running" | "starting" | "reconnecting"
        );
    let active_run_count = active_run_count();
    let status = override_status.unwrap_or_else(|| {
        if snapshot.state == "running" && active_run_count > 0 {
            "busy"
        } else {
            match snapshot.state.as_str() {
                "running" => "ready",
                "starting" | "reconnecting" => "degraded",
                "error" => "degraded",
                _ => "unavailable",
            }
        }
    });
    let openclaw_reachable = matches!(status, "ready" | "busy")
        && snapshot.state == "running"
        && snapshot.error.is_none();
    let detail = detail_override.or_else(|| runtime_status_detail(snapshot, status));

    events::new_event(
        node_id,
        session_id,
        run_id,
        "runtime.status",
        events::runtime_status_payload(status, gateway_running, openclaw_reachable, detail),
    )
}

fn build_session_updated_event(
    node_id: &str,
    session_id: Option<String>,
    run_id: Option<String>,
    state: &str,
    thinking_level: Option<String>,
    model: Option<String>,
) -> Option<BridgeEventEnvelope> {
    let session_id = session_id?;
    let summary = SessionSummary {
        display_name: display_name_for_session(&session_id),
        last_activity_at: crate::now_iso_string(),
        model,
        session_id: session_id.clone(),
        state: state.to_string(),
        thinking_level,
    };

    Some(events::new_event(
        node_id,
        Some(session_id),
        run_id,
        "session.updated",
        events::session_summary_payload(summary),
    ))
}

fn runtime_error_event(
    node_id: &str,
    session_id: Option<String>,
    run_id: Option<String>,
    error: BridgeError,
    fatal: bool,
) -> BridgeEventEnvelope {
    events::new_event(
        node_id,
        session_id,
        run_id,
        "runtime.error",
        events::runtime_error_payload(error, fatal),
    )
}

fn classify_runtime_error(detail: &str) -> BridgeError {
    let lowered = detail.to_ascii_lowercase();

    if lowered.contains("timed out") {
        return events::bridge_error(
            "UPSTREAM_TIMEOUT",
            "Gateway upstream request timed out",
            Some(detail.to_string()),
            "gateway",
            true,
        );
    }

    if lowered.contains("sessionkey") || lowered.contains("runid") || lowered.contains("nonce") {
        return events::bridge_error(
            "UPSTREAM_PROTOCOL_ERROR",
            "Gateway payload is missing required protocol fields",
            Some(detail.to_string()),
            "bridge",
            false,
        );
    }

    if lowered.contains("not running")
        || lowered.contains("gateway exited")
        || lowered.contains("connection refused")
    {
        return events::bridge_error(
            "GATEWAY_NOT_RUNNING",
            "Gateway is not running",
            Some(detail.to_string()),
            "gateway",
            true,
        );
    }

    if lowered.contains("protocol") || lowered.contains("invalid payload") {
        return events::bridge_error(
            "UPSTREAM_PROTOCOL_ERROR",
            "Gateway protocol payload is invalid",
            Some(detail.to_string()),
            "bridge",
            false,
        );
    }

    events::bridge_error(
        "OPENCLAW_UNREACHABLE",
        "Gateway cannot reach OpenClaw",
        Some(detail.to_string()),
        "gateway",
        true,
    )
}

fn protocol_error_event(node_id: &str, detail: String) -> BridgeEventEnvelope {
    runtime_error_event(
        node_id,
        None,
        None,
        events::bridge_error(
            "UPSTREAM_PROTOCOL_ERROR",
            "Gateway payload is missing required fields",
            Some(detail),
            "bridge",
            false,
        ),
        false,
    )
}

fn runtime_status_detail(snapshot: &crate::GatewayStatus, status: &str) -> Option<String> {
    if let Some(error) = snapshot.error.clone() {
        return Some(error);
    }

    match status {
        "degraded" if snapshot.state == "starting" => Some("Gateway is starting".into()),
        "degraded" if snapshot.state == "reconnecting" => Some("Gateway is reconnecting".into()),
        "unavailable" => Some("Gateway is not running".into()),
        _ => None,
    }
}

fn run_cache_key(session_id: Option<&str>, run_id: Option<&str>) -> Option<String> {
    let run_id = run_id?.trim();
    if run_id.is_empty() {
        return None;
    }

    Some(format!("{}::{run_id}", session_id.unwrap_or("sessionless")))
}

fn set_run_active(run_id: &str, active: bool) {
    if let Ok(mut guard) = active_runs().lock() {
        if active {
            guard.insert(run_id.to_string());
        } else {
            guard.remove(run_id);
        }
    }
}

fn active_run_count() -> usize {
    active_runs()
        .lock()
        .map(|guard| guard.len())
        .unwrap_or_default()
}

fn resolve_node_id() -> Option<String> {
    super::bootstrap::bridge_runtime()
        .map(|runtime| runtime.config.node_id.clone())
        .or_else(|| super::bootstrap::bridge_node_id().ok())
}

fn optional_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn payload_looks_like_message(payload: &Value) -> bool {
    payload.get("role").is_some()
        || payload.get("content").is_some()
        || payload.get("toolCallId").is_some()
        || payload.get("tool_call_id").is_some()
}

fn infer_message_state(message: &Value) -> Option<&'static str> {
    if message.get("stopReason").is_some() || message.get("stop_reason").is_some() {
        Some("final")
    } else if payload_looks_like_message(message) {
        Some("delta")
    } else {
        None
    }
}

fn extract_connect_challenge_nonce(raw: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(raw).ok()?;
    let payload = value.get("payload")?;
    if value.get("type").and_then(Value::as_str) != Some("event")
        || value.get("event").and_then(Value::as_str) != Some("connect.challenge")
    {
        return None;
    }

    payload
        .get("nonce")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|nonce| !nonce.is_empty())
        .map(str::to_string)
}

fn extract_connect_result(raw: &str, expected_id: &str) -> Result<Option<String>, String> {
    let value = serde_json::from_str::<Value>(raw)
        .map_err(|error| format!("Gateway connect response is not valid JSON: {error}"))?;

    if value.get("type").and_then(Value::as_str) != Some("res")
        || value.get("id").and_then(Value::as_str) != Some(expected_id)
    {
        return Ok(None);
    }

    if value.get("ok").and_then(Value::as_bool) == Some(false) || value.get("error").is_some() {
        let detail = value
            .get("error")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| "Gateway connect failed".into());
        return Err(detail);
    }

    let protocol = value
        .get("payload")
        .and_then(Value::as_object)
        .and_then(|payload| payload.get("protocol"))
        .and_then(Value::as_i64)
        .unwrap_or_default();

    Ok(Some(format!("protocol-{protocol}")))
}

fn is_gateway_ping_text(raw: &str) -> bool {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|value| {
            value
                .get("method")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .map(|method| method == "ping")
        .unwrap_or(false)
}

fn extract_visible_text(message: &Value) -> Option<String> {
    let role = optional_string(message.get("role")).unwrap_or_else(|| "assistant".into());
    if matches!(role.as_str(), "toolresult" | "tool_result") {
        return None;
    }

    if let Some(content) = message.get("content") {
        if let Some(text) = extract_text_from_content(content) {
            return Some(text);
        }
    }

    optional_string(message.get("text"))
}

fn extract_thinking_text(message: &Value) -> Option<String> {
    let content = message.get("content")?.as_array()?;
    let mut parts = Vec::new();

    for block in content {
        let block_type = block
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if block_type == "thinking" {
            if let Some(text) = optional_string(block.get("thinking").or_else(|| block.get("text")))
            {
                parts.push(text);
            }
        }
    }

    (!parts.is_empty()).then(|| parts.join("\n"))
}

fn extract_text_from_content(content: &Value) -> Option<String> {
    if let Some(text) = content.as_str() {
        let text = text.trim();
        return (!text.is_empty()).then(|| text.to_string());
    }

    let blocks = content.as_array()?;
    let mut parts = Vec::new();

    for block in blocks {
        let block_type = block
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if block_type == "text" {
            if let Some(text) = optional_string(block.get("text")) {
                parts.push(text);
            }
        }
    }

    (!parts.is_empty()).then(|| parts.join("\n"))
}

fn extract_tool_calls(message: &Value) -> Vec<ToolCallRecord> {
    let mut results = Vec::new();

    if let Some(content) = message.get("content").and_then(Value::as_array) {
        for block in content {
            let block_type = block
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !matches!(block_type, "tool_use" | "toolCall") {
                continue;
            }

            let name = optional_string(block.get("name")).unwrap_or_else(|| "tool".into());
            let tool_call_id = optional_string(block.get("id")).unwrap_or_else(|| name.clone());
            let arguments =
                normalize_arguments_object(block.get("input").or_else(|| block.get("arguments")));

            results.push(ToolCallRecord {
                summary: summarize_tool_call(&name, &arguments),
                tool_call_id,
                name,
                arguments,
            });
        }
    }

    if results.is_empty() {
        if let Some(tool_calls) = message
            .get("tool_calls")
            .or_else(|| message.get("toolCalls"))
            .and_then(Value::as_array)
        {
            for tool_call in tool_calls {
                let function = tool_call
                    .get("function")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let name = function
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| "tool".into());
                let tool_call_id =
                    optional_string(tool_call.get("id")).unwrap_or_else(|| name.clone());
                let arguments = normalize_arguments_object(function.get("arguments"));
                results.push(ToolCallRecord {
                    summary: summarize_tool_call(&name, &arguments),
                    tool_call_id,
                    name,
                    arguments,
                });
            }
        }
    }

    results
}

fn extract_tool_results(message: &Value) -> Vec<ToolResultRecord> {
    let mut results = Vec::new();

    if let Some(content) = message.get("content").and_then(Value::as_array) {
        for block in content {
            let block_type = block
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !matches!(block_type, "tool_result" | "toolResult") {
                continue;
            }

            let tool_call_id = optional_string(block.get("id"))
                .or_else(|| optional_string(block.get("tool_call_id")))
                .unwrap_or_else(|| "tool".into());
            let name = optional_string(block.get("name")).unwrap_or_else(|| tool_call_id.clone());
            let output_text = extract_output_text(block.get("content").unwrap_or(&Value::Null))
                .or_else(|| optional_string(block.get("text")));
            let status = if block.get("is_error").and_then(Value::as_bool) == Some(true)
                || optional_string(block.get("status"))
                    .map(|status| status.eq_ignore_ascii_case("error"))
                    .unwrap_or(false)
            {
                "error"
            } else {
                "success"
            };

            results.push(ToolResultRecord {
                summary: summarize_tool_result(status, output_text.as_deref()),
                tool_call_id,
                name,
                output_text,
                status,
            });
        }
    }

    let role = optional_string(message.get("role")).unwrap_or_default();
    if matches!(role.as_str(), "toolresult" | "tool_result") {
        let tool_call_id = optional_string(message.get("toolCallId"))
            .or_else(|| optional_string(message.get("tool_call_id")))
            .unwrap_or_else(|| "tool".into());
        let name = optional_string(message.get("toolName"))
            .or_else(|| optional_string(message.get("name")))
            .unwrap_or_else(|| tool_call_id.clone());
        let details = message
            .get("details")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let output_text = details
            .get("aggregated")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| extract_output_text(message.get("content").unwrap_or(&Value::Null)));
        let status = if message.get("isError").and_then(Value::as_bool) == Some(true)
            || optional_string(details.get("status"))
                .map(|status| status.eq_ignore_ascii_case("error"))
                .unwrap_or(false)
        {
            "error"
        } else {
            "success"
        };

        results.push(ToolResultRecord {
            summary: summarize_tool_result(status, output_text.as_deref()),
            tool_call_id,
            name,
            output_text,
            status,
        });
    }

    results
}

fn build_bridge_message(run_id: Option<String>, message: &Value) -> Option<Value> {
    let content = build_bridge_content_blocks(message);
    if content.is_empty() {
        return None;
    }

    let created_at = message_timestamp_to_iso(message).unwrap_or_else(crate::now_iso_string);
    let role = normalize_bridge_role(
        optional_string(message.get("role")).unwrap_or_else(|| "assistant".into()),
    );
    let message_id = optional_string(message.get("id"))
        .unwrap_or_else(|| format!("msg_{}", Uuid::new_v4().simple()));
    let stop_reason = normalize_stop_reason(message);
    let usage = usage_payload(message);

    Some(json!({
        "message_id": message_id,
        "run_id": run_id,
        "role": role,
        "created_at": created_at,
        "stop_reason": stop_reason,
        "content": content,
        "usage": usage,
    }))
}

fn build_bridge_content_blocks(message: &Value) -> Vec<Value> {
    let mut blocks = Vec::new();

    if let Some(content) = message.get("content") {
        if let Some(text) = content.as_str() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                blocks.push(json!({
                    "type": "text",
                    "text": trimmed,
                }));
            }
        } else if let Some(items) = content.as_array() {
            for block in items {
                let block_type = block
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                match block_type {
                    "text" => {
                        if let Some(text) = optional_string(block.get("text")) {
                            blocks.push(json!({
                                "type": "text",
                                "text": text,
                            }));
                        }
                    }
                    "thinking" => {
                        if let Some(text) =
                            optional_string(block.get("thinking").or_else(|| block.get("text")))
                        {
                            blocks.push(json!({
                                "type": "thinking",
                                "text": text,
                            }));
                        }
                    }
                    "tool_use" | "toolCall" => {
                        let name =
                            optional_string(block.get("name")).unwrap_or_else(|| "tool".into());
                        let tool_call_id =
                            optional_string(block.get("id")).unwrap_or_else(|| name.clone());
                        let arguments = normalize_arguments_object(
                            block.get("input").or_else(|| block.get("arguments")),
                        );
                        blocks.push(json!({
                            "type": "tool_call",
                            "tool_call_id": tool_call_id,
                            "name": name,
                            "arguments": arguments,
                        }));
                    }
                    "tool_result" | "toolResult" => {
                        let tool_call_id = optional_string(block.get("id"))
                            .or_else(|| optional_string(block.get("tool_call_id")))
                            .unwrap_or_else(|| "tool".into());
                        let name = optional_string(block.get("name"))
                            .unwrap_or_else(|| tool_call_id.clone());
                        let output_text =
                            extract_output_text(block.get("content").unwrap_or(&Value::Null))
                                .or_else(|| optional_string(block.get("text")));
                        let status = if block.get("is_error").and_then(Value::as_bool) == Some(true)
                        {
                            "error"
                        } else {
                            "success"
                        };
                        blocks.push(json!({
                            "type": "tool_result",
                            "tool_call_id": tool_call_id,
                            "name": name,
                            "status": status,
                            "output_text": output_text,
                        }));
                    }
                    _ => {}
                }
            }
        }
    }

    if blocks.is_empty() {
        if let Some(text) = optional_string(message.get("text")) {
            blocks.push(json!({
                "type": "text",
                "text": text,
            }));
        }
    }

    let role = optional_string(message.get("role")).unwrap_or_default();
    if matches!(role.as_str(), "toolresult" | "tool_result") && blocks.is_empty() {
        let tool_call_id = optional_string(message.get("toolCallId"))
            .or_else(|| optional_string(message.get("tool_call_id")))
            .unwrap_or_else(|| "tool".into());
        let name = optional_string(message.get("toolName"))
            .or_else(|| optional_string(message.get("name")))
            .unwrap_or_else(|| tool_call_id.clone());
        let output_text = extract_output_text(message.get("content").unwrap_or(&Value::Null));
        let status = if message.get("isError").and_then(Value::as_bool) == Some(true) {
            "error"
        } else {
            "success"
        };
        blocks.push(json!({
            "type": "tool_result",
            "tool_call_id": tool_call_id,
            "name": name,
            "status": status,
            "output_text": output_text,
        }));
    }

    blocks
}

fn normalize_arguments_object(raw: Option<&Value>) -> Value {
    let Some(raw) = raw else {
        return json!({});
    };

    if raw.is_object() {
        return raw.clone();
    }

    if let Some(raw_text) = raw.as_str() {
        if let Ok(parsed) = serde_json::from_str::<Value>(raw_text) {
            if parsed.is_object() {
                return parsed;
            }
        }
    }

    json!({})
}

fn message_timestamp_to_iso(message: &Value) -> Option<String> {
    let timestamp = message
        .get("createdAt")
        .or_else(|| message.get("created_at"))
        .or_else(|| message.get("timestamp"))?;

    if let Some(value) = timestamp.as_str() {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let numeric = timestamp
        .as_i64()
        .or_else(|| timestamp.as_u64().map(|value| value as i64))?;
    let millis = if numeric >= 1_000_000_000_000 {
        numeric
    } else {
        numeric.saturating_mul(1_000)
    };
    let datetime =
        time::OffsetDateTime::from_unix_timestamp_nanos(millis as i128 * 1_000_000).ok()?;

    datetime
        .format(&time::format_description::well_known::Rfc3339)
        .ok()
}

fn usage_payload(message: &Value) -> Value {
    let Some(usage) = message.get("usage").and_then(Value::as_object) else {
        return Value::Null;
    };

    let payload = json!({
        "input_tokens": usage_number(usage, &["input_tokens", "inputTokens"]),
        "output_tokens": usage_number(usage, &["output_tokens", "outputTokens"]),
        "cache_read_tokens": usage_number(usage, &["cache_read_tokens", "cacheReadTokens"]),
        "cache_write_tokens": usage_number(usage, &["cache_write_tokens", "cacheWriteTokens"]),
        "total_tokens": usage_number(usage, &["total_tokens", "totalTokens"]),
        "cost_usd": usage_float(usage, &["cost_usd", "costUsd", "cost"]),
    });

    if payload
        == json!({
            "input_tokens": 0_u64,
            "output_tokens": 0_u64,
            "cache_read_tokens": 0_u64,
            "cache_write_tokens": 0_u64,
            "total_tokens": 0_u64,
            "cost_usd": 0.0_f64,
        })
    {
        Value::Null
    } else {
        payload
    }
}

fn usage_number(usage: &Map<String, Value>, keys: &[&str]) -> u64 {
    keys.iter()
        .find_map(|key| usage.get(*key))
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_i64().map(|value| value.max(0) as u64))
        })
        .unwrap_or(0)
}

fn usage_float(usage: &Map<String, Value>, keys: &[&str]) -> f64 {
    keys.iter()
        .find_map(|key| usage.get(*key))
        .and_then(|value| {
            value
                .as_f64()
                .or_else(|| value.as_str()?.parse::<f64>().ok())
        })
        .unwrap_or(0.0)
}

fn normalize_bridge_role(role: String) -> &'static str {
    match role.as_str() {
        "user" => "user",
        "system" => "system",
        "toolresult" | "tool_result" | "tool" => "tool",
        _ => "assistant",
    }
}

fn normalize_stop_reason(message: &Value) -> Value {
    let normalized = optional_string(
        message
            .get("stopReason")
            .or_else(|| message.get("stop_reason")),
    )
    .map(|stop_reason| match stop_reason.as_str() {
        "tool_use" | "tool_call" => "tool_call".to_string(),
        "abort" | "aborted" => "aborted".to_string(),
        "error" | "failed" => "error".to_string(),
        _ => "completed".to_string(),
    })
    .unwrap_or_else(|| "completed".into());

    Value::String(normalized)
}

fn summarize_tool_call(name: &str, arguments: &Value) -> Option<String> {
    if let Some(path) = arguments.get("path").and_then(Value::as_str) {
        return Some(format!("{} {}", localized_tool_verb(name), path));
    }

    if arguments
        .as_object()
        .map(|map| map.is_empty())
        .unwrap_or(true)
    {
        return None;
    }

    Some(truncate_text(&format!("{name} {}", arguments), 120))
}

fn summarize_tool_result(status: &str, output_text: Option<&str>) -> String {
    if let Some(text) = output_text {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return truncate_text(trimmed, 160);
        }
    }

    if status == "error" {
        "工具调用失败".into()
    } else {
        "工具调用完成".into()
    }
}

fn localized_tool_verb(name: &str) -> &'static str {
    match name {
        "read" => "读取",
        "write" => "写入",
        "edit" => "编辑",
        "search" => "搜索",
        _ => "调用",
    }
}

fn truncate_text(text: &str, max_len: usize) -> String {
    if text.chars().count() <= max_len {
        text.to_string()
    } else {
        text.chars()
            .take(max_len.saturating_sub(1))
            .collect::<String>()
            + "…"
    }
}

fn extract_output_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        let trimmed = text.trim();
        return (!trimmed.is_empty()).then(|| trimmed.to_string());
    }

    if let Some(items) = value.as_array() {
        let mut parts = Vec::new();
        for item in items {
            if let Some(text) = optional_string(item.get("text")) {
                parts.push(text);
            }
        }
        return (!parts.is_empty()).then(|| parts.join("\n"));
    }

    None
}

fn is_tool_only_message(message: &Value) -> bool {
    let Some(content) = message.get("content") else {
        return false;
    };

    let Some(items) = content.as_array() else {
        return false;
    };

    let mut has_tool = false;
    let mut has_visible_text = false;

    for item in items {
        let block_type = item.get("type").and_then(Value::as_str).unwrap_or_default();
        match block_type {
            "text" => {
                if optional_string(item.get("text")).is_some() {
                    has_visible_text = true;
                }
            }
            "tool_use" | "toolCall" | "tool_result" | "toolResult" => has_tool = true,
            _ => {}
        }
    }

    has_tool && !has_visible_text
}

fn display_name_for_session(session_id: &str) -> String {
    session_id
        .rsplit(':')
        .next()
        .filter(|segment| !segment.is_empty())
        .unwrap_or(session_id)
        .to_string()
}

#[cfg(test)]
fn reset_runtime_tracking() {
    if let Ok(mut guard) = active_runs().lock() {
        guard.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_thinking_tool_and_text_delta_events() {
        events::reset_runtime_status_cache();
        reset_runtime_tracking();

        let mut mapper = GatewayEventMapper::default();
        let state = crate::BridgeState::default();
        let payload = json!({
            "runId": "run_123",
            "sessionKey": "agent:main:main",
            "state": "delta",
            "message": {
                "id": "msg_stream",
                "role": "assistant",
                "content": [
                    { "type": "thinking", "thinking": "先读取 package.json" },
                    { "type": "tool_use", "id": "call_1", "name": "read", "input": { "path": "/repo/package.json" } },
                    { "type": "text", "text": "正在分析..." }
                ]
            }
        });

        let mapped =
            map_gateway_protocol_event("node_test", &state, &mut mapper, "agent", &payload);

        let event_types = mapped
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            event_types,
            vec![
                "message.thinking",
                "message.thinking",
                "tool.call",
                "message.delta"
            ]
        );
        assert_eq!(mapped[3].payload["delta"], "正在分析...");
    }

    #[test]
    fn maps_tool_result_and_final_message() {
        events::reset_runtime_status_cache();
        reset_runtime_tracking();

        let mut mapper = GatewayEventMapper::default();
        let state = crate::BridgeState::default();
        let payload = json!({
            "runId": "run_456",
            "sessionKey": "agent:main:main",
            "state": "final",
            "message": {
                "id": "msg_final",
                "role": "assistant",
                "stopReason": "completed",
                "timestamp": 1_742_400_000,
                "usage": {
                    "inputTokens": 12,
                    "outputTokens": 34,
                    "totalTokens": 46,
                    "costUsd": 0.12
                },
                "content": [
                    { "type": "tool_result", "id": "call_1", "name": "read", "content": [{ "type": "text", "text": "package.json 已读取" }] },
                    { "type": "text", "text": "分析完成。" }
                ]
            }
        });

        let mapped = map_gateway_protocol_event("node_test", &state, &mut mapper, "chat", &payload);

        let event_types = mapped
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>();
        assert_eq!(event_types, vec!["tool.result", "message.final"]);
        assert_eq!(
            mapped[1].payload["message"]["content"][1]["text"],
            "分析完成。"
        );
        assert_eq!(mapped[1].payload["message"]["run_id"], "run_456");
    }
}
