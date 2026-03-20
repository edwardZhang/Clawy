use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Json, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use std::net::TcpStream;
use std::time::{Duration, Instant};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};
use uuid::Uuid;

use super::audit;
use super::auth::RequestContext;
use super::response::{self, ApiError};
use super::server::BridgeAppState;

const BRIDGE_CLIENT_ID: &str = "gateway-client";
const BRIDGE_CLIENT_MODE: &str = "ui";
const RPC_RETRY_DELAY_MS: u64 = 500;
const SEND_RPC_TIMEOUT: Duration = Duration::from_secs(120);
const ABORT_RPC_TIMEOUT: Duration = Duration::from_secs(30);

type GatewaySocket = WebSocket<MaybeTlsStream<TcpStream>>;

pub(crate) async fn session_send_handler(
    Path(session_id): Path<String>,
    State(state): State<BridgeAppState>,
    Extension(context): Extension<RequestContext>,
    body: Result<Json<Value>, JsonRejection>,
) -> Response {
    let body = match body {
        Ok(Json(value)) => value,
        Err(error) => {
            return ApiError::custom(
                StatusCode::BAD_REQUEST,
                "INVALID_REQUEST",
                "request body must be valid JSON",
                Some(error.body_text()),
                "bridge",
                false,
                &context,
            )
            .into_response();
        }
    };

    let request_id = context.request_id.clone();
    let task_context = context.clone();
    let task_state = state.clone();

    match tokio::task::spawn_blocking(move || {
        process_send(task_state, task_context, session_id, body)
    })
    .await
    {
        Ok(Ok(data)) => {
            audit::log_key_action(
                &state,
                &context.request_id,
                context.caller_id.as_deref(),
                "session.send",
                data.get("session_id").and_then(Value::as_str),
                "accepted",
                data.get("run_id")
                    .and_then(Value::as_str)
                    .map(|run_id| format!("run_id={run_id}"))
                    .as_deref(),
            );
            response::success(StatusCode::ACCEPTED, &request_id, data)
        }
        Ok(Err(error)) => error.into_response(),
        Err(error) => ApiError::custom(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "Bridge send task failed",
            Some(error.to_string()),
            "bridge",
            false,
            &context,
        )
        .into_response(),
    }
}

pub(crate) async fn session_abort_handler(
    Path(session_id): Path<String>,
    State(state): State<BridgeAppState>,
    Extension(context): Extension<RequestContext>,
) -> Response {
    let request_id = context.request_id.clone();
    let task_context = context.clone();
    let task_state = state.clone();

    match tokio::task::spawn_blocking(move || process_abort(task_state, task_context, session_id))
        .await
    {
        Ok(Ok(data)) => {
            audit::log_key_action(
                &state,
                &context.request_id,
                context.caller_id.as_deref(),
                "session.abort",
                data.get("session_id").and_then(Value::as_str),
                "accepted",
                data.get("run_id")
                    .and_then(Value::as_str)
                    .map(|run_id| format!("run_id={run_id}"))
                    .as_deref(),
            );
            response::success(StatusCode::ACCEPTED, &request_id, data)
        }
        Ok(Err(error)) => error.into_response(),
        Err(error) => ApiError::custom(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "Bridge abort task failed",
            Some(error.to_string()),
            "bridge",
            false,
            &context,
        )
        .into_response(),
    }
}

fn process_send(
    state: BridgeAppState,
    context: RequestContext,
    session_id: String,
    body: Value,
) -> Result<Value, ApiError> {
    let session_key = resolve_session_id(&session_id, &context)?;
    let object = body.as_object().ok_or_else(|| {
        ApiError::custom(
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
            "request body must be a JSON object",
            None,
            "bridge",
            false,
            &context,
        )
    })?;

    if object.contains_key("attachments") {
        return Err(ApiError::custom(
            StatusCode::BAD_REQUEST,
            "ATTACHMENTS_NOT_SUPPORTED",
            "attachments are not supported in P0",
            Some("POST /api/sessions/{session_id}/send only accepts a text `message`.".into()),
            "bridge",
            false,
            &context,
        ));
    }

    let message = object
        .get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApiError::custom(
                StatusCode::BAD_REQUEST,
                "INVALID_REQUEST",
                "`message` must be a non-empty string",
                Some("POST /api/sessions/{session_id}/send requires a text `message`.".into()),
                "bridge",
                false,
                &context,
            )
        })?;

    let rpc_payload = json!({
        "sessionKey": session_key,
        "message": message,
        "deliver": false,
        "idempotencyKey": Uuid::new_v4().to_string(),
    });
    let result = invoke_gateway_rpc(
        &state,
        &context,
        "chat.send",
        rpc_payload,
        true,
        SEND_RPC_TIMEOUT,
    )?;
    let run_id = extract_run_id(&result).ok_or_else(|| {
        ApiError::custom(
            StatusCode::BAD_GATEWAY,
            "UPSTREAM_PROTOCOL_ERROR",
            "Gateway response did not satisfy the Bridge contract",
            Some(
                "chat.send succeeded but the Gateway response did not include a non-empty runId."
                    .into(),
            ),
            "gateway",
            false,
            &context,
        )
    })?;

    Ok(json!({
        "accepted": true,
        "session_id": session_key,
        "run_id": run_id,
    }))
}

fn process_abort(
    state: BridgeAppState,
    context: RequestContext,
    session_id: String,
) -> Result<Value, ApiError> {
    let session_key = resolve_session_id(&session_id, &context)?;
    let result = invoke_gateway_rpc(
        &state,
        &context,
        "chat.abort",
        json!({ "sessionKey": session_key }),
        false,
        ABORT_RPC_TIMEOUT,
    )?;
    let mut response = serde_json::Map::new();
    response.insert("accepted".into(), Value::Bool(true));
    response.insert("session_id".into(), Value::String(session_key));
    response.insert("aborted".into(), Value::Bool(true));
    if let Some(run_id) = extract_run_id(&result) {
        response.insert("run_id".into(), Value::String(run_id));
    }

    Ok(Value::Object(response))
}

fn invoke_gateway_rpc(
    state: &BridgeAppState,
    context: &RequestContext,
    method: &str,
    params: Value,
    allow_start: bool,
    timeout: Duration,
) -> Result<Value, ApiError> {
    ensure_gateway_state(state, context, allow_start)?;

    let deadline = Instant::now() + timeout;
    let mut attempted_auto_pair = false;
    let mut last_error: Option<ApiError> = None;

    while Instant::now() < deadline {
        let status = crate::gateway_status_snapshot(&state.bridge_state).map_err(|detail| {
            ApiError::custom(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "Failed to read Gateway status",
                Some(detail),
                "bridge",
                false,
                context,
            )
        })?;

        if !gateway_state_can_connect(&status.state) {
            last_error = Some(if allow_start {
                ApiError::custom(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "OPENCLAW_UNREACHABLE",
                    "Gateway / OpenClaw is unreachable",
                    Some(format!(
                        "Gateway did not become ready for `{method}` (state={}).",
                        status.state
                    )),
                    "gateway",
                    true,
                    context,
                )
            } else {
                ApiError::custom(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "GATEWAY_NOT_RUNNING",
                    "Gateway is not running",
                    Some(format!(
                        "Gateway is not running for `{method}` (state={}).",
                        status.state
                    )),
                    "bridge",
                    true,
                    context,
                )
            });
            std::thread::sleep(Duration::from_millis(RPC_RETRY_DELAY_MS));
            continue;
        }

        let gateway_url = format!("ws://127.0.0.1:{}/ws", status.port);
        match invoke_gateway_rpc_once(&gateway_url, method, params.clone(), |nonce| {
            build_bridge_connect_params(state, context, nonce)
        }) {
            Ok(result) => return Ok(result),
            Err(detail) => {
                if !attempted_auto_pair && pairing_required(&detail) {
                    attempted_auto_pair = true;
                    if try_auto_approve_pairing(context)? {
                        continue;
                    }
                }

                last_error = Some(map_gateway_error(context, method, detail));
                std::thread::sleep(Duration::from_millis(RPC_RETRY_DELAY_MS));
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        ApiError::custom(
            StatusCode::GATEWAY_TIMEOUT,
            "UPSTREAM_TIMEOUT",
            "Gateway RPC timed out",
            Some(format!(
                "Timed out waiting for Gateway RPC `{method}` after {} ms.",
                timeout.as_millis()
            )),
            "gateway",
            true,
            context,
        )
    }))
}

fn ensure_gateway_state(
    state: &BridgeAppState,
    context: &RequestContext,
    allow_start: bool,
) -> Result<(), ApiError> {
    let status = crate::gateway_status_snapshot(&state.bridge_state).map_err(|detail| {
        ApiError::custom(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "Failed to read Gateway status",
            Some(detail),
            "bridge",
            false,
            context,
        )
    })?;

    if gateway_state_can_connect(&status.state) {
        return Ok(());
    }

    if !allow_start {
        return Err(ApiError::custom(
            StatusCode::SERVICE_UNAVAILABLE,
            "GATEWAY_NOT_RUNNING",
            "Gateway is not running",
            Some(format!("Gateway is not running (state={}).", status.state)),
            "bridge",
            true,
            context,
        ));
    }

    let app_handle = state.app_handle.clone().ok_or_else(|| {
        ApiError::custom(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "Bridge app handle is unavailable",
            Some("Bridge runtime was started without a Tauri app handle.".into()),
            "bridge",
            false,
            context,
        )
    })?;

    crate::gateway_start_internal(&app_handle, &state.bridge_state)
        .map(|_| ())
        .map_err(|detail| {
            ApiError::custom(
                StatusCode::SERVICE_UNAVAILABLE,
                "OPENCLAW_UNREACHABLE",
                "Gateway / OpenClaw is unreachable",
                Some(detail),
                "gateway",
                true,
                context,
            )
        })
}

fn invoke_gateway_rpc_once<F>(
    gateway_url: &str,
    method: &str,
    params: Value,
    build_connect_params: F,
) -> Result<Value, String>
where
    F: Fn(&str) -> Result<Value, String>,
{
    let (mut socket, _) =
        connect(gateway_url).map_err(|err| format!("Failed to connect to Gateway: {err}"))?;

    let nonce = read_connect_challenge(&mut socket)?;
    let connect_params = build_connect_params(&nonce)?;
    let connect_id = format!("connect-{}", Uuid::new_v4());

    write_json_message(
        &mut socket,
        &json!({
            "type": "req",
            "id": connect_id,
            "method": "connect",
            "params": connect_params,
        }),
    )?;

    wait_for_response(&mut socket, &connect_id)?;

    let request_id = format!("rpc-{}", Uuid::new_v4());
    write_json_message(
        &mut socket,
        &json!({
            "type": "req",
            "id": request_id,
            "method": method,
            "params": params,
        }),
    )?;

    wait_for_response(&mut socket, &request_id)
}

fn read_connect_challenge(socket: &mut GatewaySocket) -> Result<String, String> {
    loop {
        let message = read_json_message(socket)?;
        if message.get("type").and_then(Value::as_str) != Some("event") {
            continue;
        }
        if message.get("event").and_then(Value::as_str) != Some("connect.challenge") {
            continue;
        }

        let nonce = message
            .get("payload")
            .and_then(Value::as_object)
            .and_then(|payload| payload.get("nonce"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "Gateway connect.challenge missing nonce".to_string())?;

        return Ok(nonce.to_string());
    }
}

fn wait_for_response(socket: &mut GatewaySocket, request_id: &str) -> Result<Value, String> {
    loop {
        let message = read_json_message(socket)?;
        if message.get("type").and_then(Value::as_str) != Some("res") {
            continue;
        }
        let response_id = message.get("id").map(value_to_id).unwrap_or_default();
        if response_id != request_id {
            continue;
        }

        if message.get("ok").and_then(Value::as_bool) == Some(false)
            || message.get("error").is_some()
        {
            return Err(extract_gateway_error_message(&message));
        }

        return Ok(message.get("payload").cloned().unwrap_or(Value::Null));
    }
}

fn read_json_message(socket: &mut GatewaySocket) -> Result<Value, String> {
    loop {
        match socket.read() {
            Ok(Message::Text(text)) => {
                return serde_json::from_str(text.as_ref())
                    .map_err(|err| format!("Invalid Gateway JSON frame: {err}"));
            }
            Ok(Message::Binary(bytes)) => {
                let text = String::from_utf8(bytes.to_vec())
                    .map_err(|err| format!("Gateway sent non-UTF8 binary frame: {err}"))?;
                return serde_json::from_str(&text)
                    .map_err(|err| format!("Invalid Gateway JSON frame: {err}"));
            }
            Ok(Message::Ping(payload)) => {
                socket
                    .send(Message::Pong(payload))
                    .map_err(|err| format!("Failed to reply to Gateway ping: {err}"))?;
            }
            Ok(Message::Pong(_)) => {}
            Ok(Message::Frame(_)) => {}
            Ok(Message::Close(frame)) => {
                let reason = frame
                    .map(|value| value.reason.to_string())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "no reason".into());
                return Err(format!(
                    "Gateway closed the socket before responding: {reason}"
                ));
            }
            Err(err) => return Err(format!("Gateway websocket error: {err}")),
        }
    }
}

fn write_json_message(socket: &mut GatewaySocket, value: &Value) -> Result<(), String> {
    let serialized = serde_json::to_string(value)
        .map_err(|err| format!("Failed to encode Gateway frame: {err}"))?;
    socket
        .send(Message::Text(serialized.into()))
        .map_err(|err| format!("Failed to send Gateway frame: {err}"))
}

fn build_bridge_connect_params(
    state: &BridgeAppState,
    context: &RequestContext,
    nonce: &str,
) -> Result<Value, String> {
    let app_handle = state.app_handle.clone().ok_or_else(|| {
        format!(
            "Bridge app handle unavailable for request {}",
            context.request_id
        )
    })?;

    crate::gateway_build_connect_params(
        &app_handle,
        &json!({
            "clientId": BRIDGE_CLIENT_ID,
            "clientMode": BRIDGE_CLIENT_MODE,
            "nonce": nonce,
            "role": "operator",
            "scopes": crate::DEFAULT_GATEWAY_SCOPES,
        }),
    )
}

fn try_auto_approve_pairing(context: &RequestContext) -> Result<bool, ApiError> {
    match crate::auto_approve_local_device_pairing() {
        Ok(value) => Ok(value
            .get("approved")
            .and_then(Value::as_bool)
            .unwrap_or(false)),
        Err(detail) => Err(ApiError::custom(
            StatusCode::SERVICE_UNAVAILABLE,
            "OPENCLAW_UNREACHABLE",
            "Gateway / OpenClaw is unreachable",
            Some(detail),
            "gateway",
            true,
            context,
        )),
    }
}

fn map_gateway_error(context: &RequestContext, method: &str, detail: String) -> ApiError {
    let lower = detail.to_lowercase();

    if lower.contains("timed out") {
        return ApiError::custom(
            StatusCode::GATEWAY_TIMEOUT,
            "UPSTREAM_TIMEOUT",
            "Gateway RPC timed out",
            Some(detail),
            "gateway",
            true,
            context,
        );
    }
    if lower.contains("no active run")
        || lower.contains("run not found")
        || lower.contains("nothing to abort")
    {
        return ApiError::custom(
            StatusCode::NOT_FOUND,
            "RUN_NOT_FOUND",
            "no active run found for this session",
            Some(detail),
            "gateway",
            false,
            context,
        );
    }
    if method == "chat.send"
        && (lower.contains("session busy")
            || lower.contains("already running")
            || lower.contains("active run")
            || lower.contains("busy"))
    {
        return ApiError::custom(
            StatusCode::CONFLICT,
            "SESSION_BUSY",
            "session already has an active run",
            Some(detail),
            "gateway",
            false,
            context,
        );
    }
    if lower.contains("session not found")
        || lower.contains("unknown session")
        || lower.contains("invalid session")
    {
        return ApiError::custom(
            StatusCode::NOT_FOUND,
            "SESSION_NOT_FOUND",
            "session_id not found",
            Some(detail),
            "bridge",
            false,
            context,
        );
    }
    if lower.contains("connection refused")
        || lower.contains("failed to connect")
        || lower.contains("websocket")
        || lower.contains("handshake")
        || lower.contains("gateway")
        || lower.contains("openclaw")
    {
        return ApiError::custom(
            StatusCode::SERVICE_UNAVAILABLE,
            "OPENCLAW_UNREACHABLE",
            "Gateway / OpenClaw is unreachable",
            Some(detail),
            "gateway",
            true,
            context,
        );
    }

    ApiError::custom(
        StatusCode::INTERNAL_SERVER_ERROR,
        "INTERNAL_ERROR",
        "Gateway RPC failed",
        Some(detail),
        "bridge",
        false,
        context,
    )
}

fn extract_gateway_error_message(message: &Value) -> String {
    message
        .get("error")
        .and_then(|error| match error {
            Value::String(value) => Some(value.clone()),
            Value::Object(object) => object
                .get("message")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            _ => None,
        })
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Gateway request failed".into())
}

fn resolve_session_id(raw_session_id: &str, context: &RequestContext) -> Result<String, ApiError> {
    let decoded = percent_decode(raw_session_id).map_err(|detail| {
        ApiError::custom(
            StatusCode::NOT_FOUND,
            "SESSION_NOT_FOUND",
            "session_id not found",
            Some(detail),
            "bridge",
            false,
            context,
        )
    })?;
    let trimmed = decoded.trim();
    if !trimmed.starts_with("agent:") {
        return Err(ApiError::custom(
            StatusCode::NOT_FOUND,
            "SESSION_NOT_FOUND",
            "session_id not found",
            Some(format!(
                "session_id must be a canonical `agent:<agent_id>:<session_suffix>` key, got `{trimmed}`."
            )),
            "bridge",
            false,
            context,
        ));
    }

    let parts: Vec<&str> = trimmed.split(':').collect();
    if parts.len() < 3 || parts[1].trim().is_empty() || parts[2..].join(":").trim().is_empty() {
        return Err(ApiError::custom(
            StatusCode::NOT_FOUND,
            "SESSION_NOT_FOUND",
            "session_id not found",
            Some(format!("No canonical session matched `{trimmed}`.")),
            "bridge",
            false,
            context,
        ));
    }

    Ok(trimmed.to_string())
}

fn percent_decode(input: &str) -> Result<String, String> {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(format!("Invalid percent-encoding in `{input}`."));
            }
            let high = decode_hex_digit(bytes[index + 1])?;
            let low = decode_hex_digit(bytes[index + 2])?;
            output.push((high << 4) | low);
            index += 3;
            continue;
        }

        output.push(bytes[index]);
        index += 1;
    }

    String::from_utf8(output).map_err(|err| format!("Invalid UTF-8 session_id: {err}"))
}

fn decode_hex_digit(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(format!("Invalid percent-encoding byte `{}`.", byte as char)),
    }
}

fn extract_run_id(payload: &Value) -> Option<String> {
    payload
        .get("runId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn pairing_required(detail: &str) -> bool {
    detail.to_lowercase().contains("pairing required")
}

fn gateway_state_can_connect(state: &str) -> bool {
    matches!(state, "running" | "starting" | "reconnecting")
}

fn value_to_id(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use tungstenite::accept;

    fn test_context() -> RequestContext {
        RequestContext {
            request_id: "req_test".into(),
            caller_id: None,
            method: "POST".into(),
            path: "/api/sessions/agent%3Amain%3Amain/send".into(),
            remote_addr: None,
            received_at_ms: 0,
        }
    }

    #[test]
    fn resolve_session_id_accepts_url_encoded_canonical_key() {
        let session_id =
            resolve_session_id("agent%3Amain%3Asession-123", &test_context()).expect("session id");
        assert_eq!(session_id, "agent:main:session-123");
    }

    #[test]
    fn resolve_session_id_rejects_non_canonical_values() {
        let error = resolve_session_id("session-123", &test_context()).expect_err("should fail");
        assert_eq!(error.into_response().status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn invoke_gateway_rpc_once_reuses_gateway_rpc_protocol_for_send() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake gateway");
        let port = listener.local_addr().expect("fake gateway addr").port();

        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept gateway socket");
            let mut socket = accept(stream).expect("accept websocket");

            socket
                .send(Message::Text(
                    json!({
                        "type": "event",
                        "event": "connect.challenge",
                        "payload": { "nonce": "nonce-123" }
                    })
                    .to_string()
                    .into(),
                ))
                .expect("send challenge");

            let connect_frame = read_json_message_for_test(&mut socket);
            assert_eq!(
                connect_frame.get("method").and_then(Value::as_str),
                Some("connect")
            );
            assert_eq!(
                connect_frame
                    .get("params")
                    .and_then(Value::as_object)
                    .and_then(|params| params.get("nonce"))
                    .and_then(Value::as_str),
                Some("nonce-123")
            );

            socket
                .send(Message::Text(
                    json!({
                        "type": "res",
                        "id": connect_frame.get("id").cloned().expect("connect id"),
                        "ok": true,
                        "payload": { "protocol": 3 }
                    })
                    .to_string()
                    .into(),
                ))
                .expect("send connect response");

            let rpc_frame = read_json_message_for_test(&mut socket);
            assert_eq!(
                rpc_frame.get("method").and_then(Value::as_str),
                Some("chat.send")
            );
            assert_eq!(
                rpc_frame
                    .get("params")
                    .and_then(Value::as_object)
                    .and_then(|params| params.get("sessionKey"))
                    .and_then(Value::as_str),
                Some("agent:main:main")
            );

            socket
                .send(Message::Text(
                    json!({
                        "type": "res",
                        "id": rpc_frame.get("id").cloned().expect("rpc id"),
                        "ok": true,
                        "payload": { "runId": "run-send-1" }
                    })
                    .to_string()
                    .into(),
                ))
                .expect("send rpc response");
        });

        let result = invoke_gateway_rpc_once(
            &format!("ws://127.0.0.1:{port}"),
            "chat.send",
            json!({
                "sessionKey": "agent:main:main",
                "message": "hello"
            }),
            |nonce| {
                Ok(json!({
                    "nonce": nonce,
                    "auth": { "token": "bridge-token" }
                }))
            },
        )
        .expect("rpc response");

        assert_eq!(extract_run_id(&result).as_deref(), Some("run-send-1"));
        server.join().expect("fake gateway thread");
    }

    #[test]
    fn invoke_gateway_rpc_once_reuses_gateway_rpc_protocol_for_abort() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake gateway");
        let port = listener.local_addr().expect("fake gateway addr").port();

        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept gateway socket");
            let mut socket = accept(stream).expect("accept websocket");

            socket
                .send(Message::Text(
                    json!({
                        "type": "event",
                        "event": "connect.challenge",
                        "payload": { "nonce": "nonce-abc" }
                    })
                    .to_string()
                    .into(),
                ))
                .expect("send challenge");

            let connect_frame = read_json_message_for_test(&mut socket);
            socket
                .send(Message::Text(
                    json!({
                        "type": "res",
                        "id": connect_frame.get("id").cloned().expect("connect id"),
                        "ok": true,
                        "payload": { "protocol": 3 }
                    })
                    .to_string()
                    .into(),
                ))
                .expect("send connect response");

            let rpc_frame = read_json_message_for_test(&mut socket);
            assert_eq!(
                rpc_frame.get("method").and_then(Value::as_str),
                Some("chat.abort")
            );

            socket
                .send(Message::Text(
                    json!({
                        "type": "res",
                        "id": rpc_frame.get("id").cloned().expect("rpc id"),
                        "ok": true,
                        "payload": { "runId": "run-abort-1" }
                    })
                    .to_string()
                    .into(),
                ))
                .expect("send abort response");
        });

        let result = invoke_gateway_rpc_once(
            &format!("ws://127.0.0.1:{port}"),
            "chat.abort",
            json!({ "sessionKey": "agent:main:main" }),
            |nonce| Ok(json!({ "nonce": nonce })),
        )
        .expect("abort response");

        assert_eq!(extract_run_id(&result).as_deref(), Some("run-abort-1"));
        server.join().expect("fake gateway thread");
    }

    fn read_json_message_for_test(socket: &mut tungstenite::WebSocket<TcpStream>) -> Value {
        loop {
            match socket.read().expect("read websocket message") {
                Message::Text(text) => {
                    return serde_json::from_str(text.as_ref()).expect("json message")
                }
                Message::Binary(bytes) => {
                    let text = String::from_utf8(bytes.to_vec()).expect("utf8 frame");
                    return serde_json::from_str(&text).expect("json message");
                }
                Message::Ping(payload) => {
                    socket.send(Message::Pong(payload)).expect("reply pong");
                }
                Message::Pong(_) | Message::Frame(_) => {}
                Message::Close(frame) => panic!("socket closed early: {frame:?}"),
            }
        }
    }
}
