use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

use super::response::attach_request_id;

pub(crate) type BridgeResult<T> = Result<T, BridgeError>;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum BridgeErrorCode {
    Unauthorized,
    ForbiddenRemote,
    OriginNotAllowed,
    InvalidRequest,
    GatewayNotRunning,
    OpenclawUnreachable,
    OpenclawRuntimeNotReady,
    UpstreamTimeout,
    UpstreamProtocolError,
    ConfigReadFailed,
    InternalError,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BridgeErrorSource {
    Bridge,
    Gateway,
    Openclaw,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct BridgeErrorBody {
    pub(crate) code: BridgeErrorCode,
    pub(crate) message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<String>,
    pub(crate) source: BridgeErrorSource,
    pub(crate) retryable: bool,
    pub(crate) status: u16,
}

#[derive(Debug, Clone)]
pub(crate) struct BridgeError {
    status: StatusCode,
    body: BridgeErrorBody,
    request_id: Option<String>,
    www_authenticate: Option<&'static str>,
}

#[derive(Debug, Serialize)]
struct BridgeErrorEnvelope {
    ok: bool,
    error: BridgeErrorBody,
}

impl BridgeError {
    pub(crate) fn unauthorized(detail: impl Into<String>) -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            BridgeErrorCode::Unauthorized,
            "Missing or invalid bearer token",
            Some(detail.into()),
            BridgeErrorSource::Bridge,
            false,
        )
        .with_www_authenticate(r#"Bearer realm="clawy-bridge""#)
    }

    pub(crate) fn forbidden_remote(detail: impl Into<String>) -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            BridgeErrorCode::ForbiddenRemote,
            "Remote address must be loopback",
            Some(detail.into()),
            BridgeErrorSource::Bridge,
            false,
        )
    }

    pub(crate) fn origin_not_allowed(detail: impl Into<String>) -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            BridgeErrorCode::OriginNotAllowed,
            "Origin is not allowed for Clawy Bridge",
            Some(detail.into()),
            BridgeErrorSource::Bridge,
            false,
        )
    }

    pub(crate) fn invalid_request(detail: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            BridgeErrorCode::InvalidRequest,
            "Request is not valid for this Bridge route",
            Some(detail.into()),
            BridgeErrorSource::Bridge,
            false,
        )
    }

    pub(crate) fn gateway_not_running(detail: impl Into<String>) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            BridgeErrorCode::GatewayNotRunning,
            "Local Gateway is not running",
            Some(detail.into()),
            BridgeErrorSource::Gateway,
            true,
        )
    }

    pub(crate) fn openclaw_unreachable(detail: impl Into<String>) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            BridgeErrorCode::OpenclawUnreachable,
            "OpenClaw runtime is not reachable",
            Some(detail.into()),
            BridgeErrorSource::Openclaw,
            true,
        )
    }

    pub(crate) fn openclaw_runtime_not_ready(detail: impl Into<String>) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            BridgeErrorCode::OpenclawRuntimeNotReady,
            "OpenClaw runtime is not ready",
            Some(detail.into()),
            BridgeErrorSource::Openclaw,
            true,
        )
    }

    pub(crate) fn upstream_timeout(detail: impl Into<String>) -> Self {
        Self::new(
            StatusCode::GATEWAY_TIMEOUT,
            BridgeErrorCode::UpstreamTimeout,
            "Upstream call timed out",
            Some(detail.into()),
            BridgeErrorSource::Openclaw,
            true,
        )
    }

    pub(crate) fn upstream_protocol_error(detail: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_GATEWAY,
            BridgeErrorCode::UpstreamProtocolError,
            "Upstream response does not satisfy the Bridge contract",
            Some(detail.into()),
            BridgeErrorSource::Openclaw,
            false,
        )
    }

    pub(crate) fn config_read_failed(path: &str, detail: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            BridgeErrorCode::ConfigReadFailed,
            "Bridge configuration could not be read",
            Some(format!("Failed to read {path}: {}", detail.into())),
            BridgeErrorSource::Bridge,
            false,
        )
    }

    pub(crate) fn internal(detail: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            BridgeErrorCode::InternalError,
            "Bridge encountered an internal error",
            Some(detail.into()),
            BridgeErrorSource::Bridge,
            false,
        )
    }

    pub(crate) fn map_message(message: impl Into<String>) -> Self {
        let message = message.into();
        let lower = message.to_ascii_lowercase();

        if lower.contains("bearer") || lower.contains("unauthorized") {
            return Self::unauthorized(message);
        }

        if lower.contains("origin") && lower.contains("allow") {
            return Self::origin_not_allowed(message);
        }

        if lower.contains("loopback") || lower.contains("remote address") {
            return Self::forbidden_remote(message);
        }

        if lower.contains("timeout") || lower.contains("timed out") {
            return Self::upstream_timeout(message);
        }

        if lower.contains("run_id") || lower.contains("protocol") {
            return Self::upstream_protocol_error(message);
        }

        if lower.contains("no compatible openclaw runtime")
            || lower.contains("openclaw runtime is not ready")
        {
            return Self::openclaw_runtime_not_ready(message);
        }

        if lower.contains("gateway is not reachable")
            || lower.contains("gateway not running")
            || lower.contains("failed to connect to gateway")
        {
            return Self::gateway_not_running(message);
        }

        if lower.contains("openclaw") && lower.contains("unreachable") {
            return Self::openclaw_unreachable(message);
        }

        if (lower.contains("failed to read")
            || lower.contains("could not read")
            || lower.contains("failed to parse")
            || lower.contains("invalid "))
            && (lower.contains("openclaw.json")
                || lower.contains("settings.json")
                || lower.contains("providers.json")
                || lower.contains("bridge-server.json"))
        {
            let path = if lower.contains("openclaw.json") {
                "openclaw.json"
            } else if lower.contains("settings.json") {
                "settings.json"
            } else if lower.contains("providers.json") {
                "providers.json"
            } else {
                "bridge-server.json"
            };
            return Self::config_read_failed(path, message);
        }

        if lower.contains("invalid") || lower.contains("expects") || lower.contains("required") {
            return Self::invalid_request(message);
        }

        Self::internal(message)
    }

    pub(crate) fn body(&self) -> BridgeErrorBody {
        self.body.clone()
    }

    pub(crate) fn into_response_with_request_id(mut self, request_id: &str) -> Response {
        self.request_id = Some(request_id.to_string());
        self.into_response()
    }

    fn new(
        status: StatusCode,
        code: BridgeErrorCode,
        message: impl Into<String>,
        detail: Option<String>,
        source: BridgeErrorSource,
        retryable: bool,
    ) -> Self {
        Self {
            status,
            body: BridgeErrorBody {
                code,
                message: message.into(),
                detail,
                source,
                retryable,
                status: status.as_u16(),
            },
            request_id: None,
            www_authenticate: None,
        }
    }

    fn with_www_authenticate(mut self, value: &'static str) -> Self {
        self.www_authenticate = Some(value);
        self
    }
}

impl IntoResponse for BridgeError {
    fn into_response(self) -> Response {
        let request_id = self.request_id.clone();
        let mut response = (
            self.status,
            Json(BridgeErrorEnvelope {
                ok: false,
                error: self.body,
            }),
        )
            .into_response();

        if let Some(request_id) = request_id.as_deref() {
            attach_request_id(&mut response, request_id);
        }

        if let Some(www_authenticate) = self.www_authenticate {
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_static(www_authenticate),
            );
        }

        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_gateway_and_config_errors_to_stable_codes() {
        let gateway_error = BridgeError::map_message("Gateway is not reachable");
        assert_eq!(
            gateway_error.body().code,
            BridgeErrorCode::GatewayNotRunning
        );
        assert_eq!(gateway_error.body().status, 503);

        let config_error =
            BridgeError::map_message("Failed to parse openclaw.json: expected value at line 1");
        assert_eq!(config_error.body().code, BridgeErrorCode::ConfigReadFailed);
        assert_eq!(config_error.body().source, BridgeErrorSource::Bridge);
    }

    #[test]
    fn maps_runtime_and_protocol_errors_to_reusable_codes() {
        let runtime_error =
            BridgeError::map_message("No compatible OpenClaw runtime available: missing entry");
        assert_eq!(
            runtime_error.body().code,
            BridgeErrorCode::OpenclawRuntimeNotReady
        );

        let protocol_error =
            BridgeError::map_message("upstream protocol error: run_id missing from response");
        assert_eq!(
            protocol_error.body().code,
            BridgeErrorCode::UpstreamProtocolError
        );
    }
}
