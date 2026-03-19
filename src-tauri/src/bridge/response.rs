use axum::http::{header, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

use super::auth::RequestContext;

const REQUEST_ID_HEADER: &str = "x-request-id";

#[allow(dead_code)]
#[derive(Debug, Serialize)]
pub(crate) struct ApiSuccessEnvelope<T> {
    ok: bool,
    data: T,
}

#[derive(Debug, Serialize)]
pub(crate) struct ApiErrorEnvelope {
    ok: bool,
    error: ApiErrorBody,
}

#[derive(Debug, Serialize)]
pub(crate) struct ApiErrorBody {
    code: &'static str,
    message: String,
    detail: Option<String>,
    source: &'static str,
    retryable: bool,
    status: u16,
}

#[derive(Debug, Clone)]
pub(crate) struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    detail: Option<String>,
    source: &'static str,
    retryable: bool,
    request_id: String,
    www_authenticate: Option<&'static str>,
}

impl ApiError {
    pub(crate) fn custom(
        status: StatusCode,
        code: &'static str,
        message: impl Into<String>,
        detail: Option<String>,
        source: &'static str,
        retryable: bool,
        context: &RequestContext,
    ) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            detail,
            source,
            retryable,
            request_id: context.request_id.clone(),
            www_authenticate: None,
        }
    }

    pub(crate) fn unauthorized(context: &RequestContext) -> Self {
        Self::custom(
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "Missing or invalid bearer token",
            Some("Provide Authorization: Bearer <token>.".into()),
            "bridge",
            false,
            context,
        )
        .with_www_authenticate(r#"Bearer realm="clawy-bridge""#)
    }

    pub(crate) fn forbidden_remote(context: &RequestContext) -> Self {
        Self::custom(
            StatusCode::FORBIDDEN,
            "FORBIDDEN_REMOTE",
            "Remote address must be loopback",
            context
                .remote_addr
                .map(|remote_addr| format!("remote address `{remote_addr}` is not loopback")),
            "bridge",
            false,
            context,
        )
    }

    pub(crate) fn origin_not_allowed(context: &RequestContext) -> Self {
        Self::custom(
            StatusCode::FORBIDDEN,
            "ORIGIN_NOT_ALLOWED",
            "Origin is not allowed for Clawy Bridge",
            Some(
                "Configure the Bridge origin allowlist before sending browser-originated requests."
                    .into(),
            ),
            "bridge",
            false,
            context,
        )
    }

    pub(crate) fn invalid_request(context: &RequestContext, detail: impl Into<String>) -> Self {
        Self::custom(
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
            "Request is not valid for this route",
            Some(detail.into()),
            "bridge",
            false,
            context,
        )
    }

    pub(crate) fn method_not_allowed(context: &RequestContext) -> Self {
        Self::custom(
            StatusCode::METHOD_NOT_ALLOWED,
            "METHOD_NOT_ALLOWED",
            "HTTP method is not allowed for this route",
            Some(format!(
                "{} {} is not defined in the current Bridge skeleton",
                context.method, context.path
            )),
            "bridge",
            false,
            context,
        )
    }

    pub(crate) fn not_found(context: &RequestContext) -> Self {
        Self::custom(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "Route was not found under /api",
            Some(format!("No handler is registered for {}", context.path)),
            "bridge",
            false,
            context,
        )
    }

    pub(crate) fn not_implemented(context: &RequestContext, route_name: &'static str) -> Self {
        Self::custom(
            StatusCode::NOT_IMPLEMENTED,
            "NOT_IMPLEMENTED",
            "Route skeleton is present but no business handler is wired yet",
            Some(format!(
                "{route_name} is reserved for a follow-up implementation group."
            )),
            "bridge",
            false,
            context,
        )
    }

    fn with_www_authenticate(mut self, value: &'static str) -> Self {
        self.www_authenticate = Some(value);
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (
            self.status,
            Json(ApiErrorEnvelope {
                ok: false,
                error: ApiErrorBody {
                    code: self.code,
                    message: self.message,
                    detail: self.detail,
                    source: self.source,
                    retryable: self.retryable,
                    status: self.status.as_u16(),
                },
            }),
        )
            .into_response();

        attach_request_id(&mut response, &self.request_id);

        if let Some(www_authenticate) = self.www_authenticate {
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_static(www_authenticate),
            );
        }

        response
    }
}

#[allow(dead_code)]
pub(crate) fn success<T>(status: StatusCode, request_id: &str, data: T) -> Response
where
    T: Serialize,
{
    let mut response = (status, Json(ApiSuccessEnvelope { ok: true, data })).into_response();

    attach_request_id(&mut response, request_id);
    response
}

pub(crate) fn attach_request_id(response: &mut Response, request_id: &str) {
    if let Ok(value) = HeaderValue::from_str(request_id) {
        response
            .headers_mut()
            .insert(HeaderName::from_static(REQUEST_ID_HEADER), value);
    }
}
