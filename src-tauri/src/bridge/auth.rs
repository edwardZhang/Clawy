use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, Request};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::net::SocketAddr;
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

    if !context
        .remote_addr
        .map(|remote_addr| remote_addr.ip().is_loopback())
        .unwrap_or(false)
    {
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
