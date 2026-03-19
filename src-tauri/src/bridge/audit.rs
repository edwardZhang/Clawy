use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;
use std::net::SocketAddr;

use super::server::BridgeAppState;

pub(crate) async fn log_http_request(
    State(state): State<BridgeAppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let caller_id = super::auth::extract_caller_id(request.headers());
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    let session_id = session_id_from_path(&path);
    let remote_addr = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|connect_info| connect_info.0);

    let response = next.run(request).await;
    let status = response.status();
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("unknown");

    crate::append_log_line(
        "INFO",
        &format!(
            "Clawy Bridge HTTP request_id={} method={} path={} status={} caller_id={} node_id={} session_id={} remote_addr={}",
            request_id,
            method,
            path,
            status.as_u16(),
            caller_id.as_deref().unwrap_or("unknown"),
            state.config.node_id,
            session_id.as_deref().unwrap_or("-"),
            remote_addr
                .map(|addr| addr.to_string())
                .unwrap_or_else(|| "unknown".into()),
        ),
    );

    response
}

pub(crate) fn log_key_action(
    state: &BridgeAppState,
    request_id: &str,
    caller_id: Option<&str>,
    action: &str,
    session_id: Option<&str>,
    outcome: &str,
    detail: Option<&str>,
) {
    crate::append_log_line(
        "INFO",
        &format!(
            "Clawy Bridge action request_id={} action={} outcome={} caller_id={} node_id={} session_id={} detail={}",
            request_id,
            action,
            outcome,
            caller_id.unwrap_or("unknown"),
            state.config.node_id,
            session_id.unwrap_or("-"),
            detail.unwrap_or("-"),
        ),
    );
}

fn session_id_from_path(path: &str) -> Option<String> {
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    let api = segments.next()?;
    if api != "api" {
        return None;
    }

    let next = segments.next()?;
    let resource = if next == "v1" { segments.next()? } else { next };

    if resource != "sessions" {
        return None;
    }

    segments
        .next()
        .filter(|session_id| !session_id.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::session_id_from_path;

    #[test]
    fn extracts_session_id_from_api_paths() {
        assert_eq!(
            session_id_from_path("/api/sessions/agent:main:main/history").as_deref(),
            Some("agent:main:main")
        );
        assert_eq!(
            session_id_from_path("/api/v1/sessions/agent:main:main/send").as_deref(),
            Some("agent:main:main")
        );
        assert_eq!(session_id_from_path("/api/node/info"), None);
    }
}
