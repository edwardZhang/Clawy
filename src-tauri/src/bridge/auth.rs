use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, Request};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::net::{IpAddr, SocketAddr};
use uuid::Uuid;

use super::permissions;
use super::response::ApiError;
use super::server::BridgeAppState;

const CALLER_ID_HEADERS: [&str; 2] = ["x-clawy-caller-id", "x-caller-id"];
const REQUEST_ID_HEADER: &str = "x-request-id";

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct RequestContext {
    pub(crate) request_id: String,
    pub(crate) caller_id: Option<String>,
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) remote_addr: Option<SocketAddr>,
    pub(crate) received_at_ms: u64,
}

pub(crate) async fn enforce_request_auth(
    State(state): State<BridgeAppState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let context = RequestContext {
        request_id: extract_request_id(request.headers()),
        caller_id: extract_caller_id(request.headers()),
        method: request.method().to_string(),
        path: request.uri().path().to_string(),
        remote_addr: request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|connect_info| connect_info.0),
        received_at_ms: crate::now_ms(),
    };

    if !remote_addr_is_allowed(context.remote_addr, &state) {
        return ApiError::forbidden_remote(&context).into_response();
    }

    if !origin_is_allowed(request.headers(), &state.config.allowed_origins) {
        return ApiError::origin_not_allowed(&context).into_response();
    }

    if !bearer_token_matches(request.headers(), &state.config.auth_token) {
        return ApiError::unauthorized(&context).into_response();
    }

    let permissions = match permissions::authorize_request(&state, &context) {
        Ok(permissions) => permissions,
        Err(error) => return error.into_response(),
    };

    request.extensions_mut().insert(context.clone());
    request.extensions_mut().insert(permissions.clone());

    let mut response =
        permissions::scope_request_permissions(permissions, async move { next.run(request).await })
            .await;
    super::response::attach_request_id(&mut response, &context.request_id);
    response
}

pub(crate) fn extract_request_id(headers: &HeaderMap) -> String {
    headers
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("req_{}", Uuid::new_v4().simple()))
}

pub(crate) fn extract_caller_id(headers: &HeaderMap) -> Option<String> {
    CALLER_ID_HEADERS.iter().find_map(|header_name| {
        headers
            .get(*header_name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn bearer_token_matches(headers: &HeaderMap, expected_token: &str) -> bool {
    if expected_token.trim().is_empty() {
        return false;
    }

    extract_bearer_token(headers)
        .map(|provided_token| provided_token == expected_token)
        .unwrap_or(false)
}

fn extract_bearer_token<'a>(headers: &'a HeaderMap) -> Option<&'a str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?.trim();
    let mut parts = value.split_whitespace();
    let scheme = parts.next()?;
    let token = parts.next()?;

    if !scheme.eq_ignore_ascii_case("Bearer") || parts.next().is_some() || token.is_empty() {
        return None;
    }

    Some(token)
}

fn origin_is_allowed(headers: &HeaderMap, allowlist: &[String]) -> bool {
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return true;
    };

    allowlist
        .iter()
        .any(|allowed_origin| allowed_origin == origin)
}

fn remote_addr_is_allowed(remote_addr: Option<SocketAddr>, state: &BridgeAppState) -> bool {
    let Some(remote_addr) = remote_addr else {
        return false;
    };
    let remote_ip = remote_addr.ip();

    if remote_ip.is_loopback() {
        return true;
    }

    if !state.config.lan_enabled || !is_local_network_ip(remote_ip) {
        return false;
    }

    if state.config.trusted_remote_cidrs.is_empty() {
        return true;
    }

    state
        .config
        .trusted_remote_cidrs
        .iter()
        .any(|cidr| cidr.contains(&remote_ip))
}

fn is_local_network_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_private() || ip.is_link_local(),
        IpAddr::V6(ip) => ip.is_unique_local() || ip.is_unicast_link_local(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::events::BridgeEventEnvelope;
    use crate::bridge::server::{BridgeAppState, BridgeRuntimeConfig};
    use ipnet::IpNet;
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use tokio::sync::broadcast;

    fn test_state(lan_enabled: bool, trusted_remote_cidrs: Vec<IpNet>) -> BridgeAppState {
        let (events_tx, _) = broadcast::channel::<BridgeEventEnvelope>(8);
        BridgeAppState {
            app_handle: None,
            bridge_state: crate::BridgeState::default(),
            config: BridgeRuntimeConfig {
                listen_addr: SocketAddr::from(([127, 0, 0, 1], 18790)),
                auth_token: "bridge-test-token".into(),
                lan_enabled,
                trusted_remote_cidrs,
                allowed_origins: Vec::new(),
                clawy_base_dir: PathBuf::from("."),
                node_id: "node_test".into(),
                openclaw_config_dir: PathBuf::from("."),
            },
            events_tx,
        }
    }

    #[test]
    fn local_only_mode_rejects_lan_remote() {
        let state = test_state(false, Vec::new());
        assert!(!remote_addr_is_allowed(
            Some(SocketAddr::from(([192, 168, 1, 25], 43123))),
            &state
        ));
        assert!(remote_addr_is_allowed(
            Some(SocketAddr::from(([127, 0, 0, 1], 43123))),
            &state
        ));
    }

    #[test]
    fn lan_mode_accepts_private_remote_and_respects_cidr_scope() {
        let unrestricted = test_state(true, Vec::new());
        assert!(remote_addr_is_allowed(
            Some(SocketAddr::from(([192, 168, 1, 25], 43123))),
            &unrestricted
        ));
        assert!(!remote_addr_is_allowed(
            Some(SocketAddr::from(([8, 8, 8, 8], 43123))),
            &unrestricted
        ));

        let scoped = test_state(
            true,
            vec!["192.168.1.0/24".parse().expect("cidr should parse")],
        );
        assert!(remote_addr_is_allowed(
            Some(SocketAddr::from(([192, 168, 1, 25], 43123))),
            &scoped
        ));
        assert!(!remote_addr_is_allowed(
            Some(SocketAddr::from(([192, 168, 2, 25], 43123))),
            &scoped
        ));
    }
}
