use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;
use sha2::{Digest, Sha256};
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
    let route = route_template(&path);
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
            route,
            status.as_u16(),
            redact_value(caller_id.as_deref()),
            state.config.node_id,
            redact_value(session_id.as_deref()),
            summarize_remote_addr(remote_addr),
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
            redact_value(caller_id),
            state.config.node_id,
            redact_value(session_id),
            redact_detail(detail),
        ),
    );
}

pub(crate) fn redact_value(value: Option<&str>) -> String {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return "-".into();
    };

    let digest = Sha256::digest(value.as_bytes());
    let short_hash = digest[..4]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!(
        "redacted(len={},sha256={})",
        value.chars().count(),
        short_hash
    )
}

pub(crate) fn redact_detail(detail: Option<&str>) -> String {
    redact_value(detail)
}

pub(crate) fn summarize_remote_addr(remote_addr: Option<SocketAddr>) -> String {
    match remote_addr {
        Some(addr) if addr.ip().is_loopback() => "loopback".into(),
        Some(_) => "non-loopback".into(),
        None => "unknown".into(),
    }
}

pub(crate) fn route_template(path: &str) -> String {
    let mut segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .peekable();
    let mut normalized = Vec::new();

    while let Some(segment) = segments.next() {
        normalized.push(segment.to_string());
        if segment == "sessions" {
            if segments.peek().is_some() {
                normalized.push("{session_id}".into());
                let _ = segments.next();
            }
        }
    }

    if normalized.is_empty() {
        "/".into()
    } else {
        format!("/{}", normalized.join("/"))
    }
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
    use super::{redact_value, route_template, session_id_from_path, summarize_remote_addr};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

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

    #[test]
    fn normalizes_session_routes_for_audit_logs() {
        assert_eq!(
            route_template("/api/sessions/agent:main:main/history"),
            "/api/sessions/{session_id}/history"
        );
        assert_eq!(
            route_template("/api/v1/sessions/agent:main:main/send"),
            "/api/v1/sessions/{session_id}/send"
        );
        assert_eq!(route_template("/api/node/info"), "/api/node/info");
    }

    #[test]
    fn redacts_identifier_values_in_logs() {
        let raw = "agent:main:main";
        let redacted = redact_value(Some(raw));

        assert_ne!(redacted, raw);
        assert!(redacted.starts_with("redacted(len="));
        assert!(!redacted.contains(raw));
        assert_eq!(redact_value(None), "-");
    }

    #[test]
    fn summarizes_remote_addresses_without_logging_ports() {
        assert_eq!(
            summarize_remote_addr(Some(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                18790
            ))),
            "loopback"
        );
        assert_eq!(
            summarize_remote_addr(Some(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7)),
                18790
            ))),
            "non-loopback"
        );
        assert_eq!(summarize_remote_addr(None), "unknown");
    }
}
