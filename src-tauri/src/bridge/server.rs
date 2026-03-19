use axum::extract::{Extension, Path};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::path::PathBuf;
use tauri::AppHandle;
use tokio::sync::broadcast;

use super::auth::{self, RequestContext};
use super::chat_control;
use super::events::BridgeEventEnvelope;
use super::response::ApiError;
use super::ws::events_ws_handler;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct BridgeRuntimeConfig {
    pub(crate) listen_addr: SocketAddr,
    pub(crate) auth_token: String,
    pub(crate) allowed_origins: Vec<String>,
    pub(crate) clawy_base_dir: PathBuf,
    pub(crate) node_id: String,
    pub(crate) openclaw_config_dir: PathBuf,
}

#[allow(dead_code)]
#[derive(Clone)]
pub(crate) struct BridgeAppState {
    pub(crate) app_handle: Option<AppHandle>,
    pub(crate) bridge_state: crate::BridgeState,
    pub(crate) config: BridgeRuntimeConfig,
    pub(crate) events_tx: broadcast::Sender<BridgeEventEnvelope>,
}

#[allow(dead_code)]
#[derive(Clone)]
pub(crate) struct BridgeRuntimeHandle {
    pub(crate) local_addr: SocketAddr,
    pub(crate) config: BridgeRuntimeConfig,
    pub(crate) events_tx: broadcast::Sender<BridgeEventEnvelope>,
}

pub(crate) fn spawn_server(
    app_handle: Option<AppHandle>,
    bridge_state: crate::BridgeState,
    mut config: BridgeRuntimeConfig,
) -> Result<BridgeRuntimeHandle, String> {
    let listener = StdTcpListener::bind(config.listen_addr).map_err(|error| {
        format!(
            "Failed to bind Clawy Bridge listener on {}: {error}",
            config.listen_addr
        )
    })?;
    listener.set_nonblocking(true).map_err(|error| {
        format!("Failed to set Clawy Bridge listener to nonblocking mode: {error}")
    })?;

    let local_addr = listener
        .local_addr()
        .map_err(|error| format!("Failed to read Clawy Bridge listener address: {error}"))?;
    config.listen_addr = local_addr;

    let (events_tx, _) = broadcast::channel(256);
    let app_state = BridgeAppState {
        app_handle,
        bridge_state,
        config: config.clone(),
        events_tx: events_tx.clone(),
    };
    let router = build_router(app_state);

    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::TcpListener::from_std(listener) {
            Ok(listener) => listener,
            Err(error) => {
                crate::append_log_line(
                    "WARN",
                    &format!("Failed to adopt Clawy Bridge listener into tokio runtime: {error}"),
                );
                return;
            }
        };

        if let Err(error) = axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        {
            crate::append_log_line("WARN", &format!("Clawy Bridge server stopped: {error}"));
        }
    });

    Ok(BridgeRuntimeHandle {
        local_addr,
        config,
        events_tx,
    })
}

pub(crate) fn build_router(state: BridgeAppState) -> Router {
    let api_router = Router::new()
        .merge(node_routes())
        .merge(session_routes())
        .merge(runtime_routes())
        .merge(events_routes())
        .fallback(api_not_found)
        .method_not_allowed_fallback(api_method_not_allowed)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::enforce_request_auth,
        ));

    Router::new().nest("/api", api_router).with_state(state)
}

fn node_routes() -> Router<BridgeAppState> {
    Router::new()
        .route("/node/info", get(node_info_handler))
        .route("/node/health", get(node_health_handler))
}

fn session_routes() -> Router<BridgeAppState> {
    Router::new()
        .route("/sessions", get(session_list_handler))
        .route("/sessions/{session_id}", get(session_detail_handler))
        .route(
            "/sessions/{session_id}/history",
            get(session_history_handler),
        )
        .route(
            "/sessions/{session_id}/send",
            post(chat_control::session_send_handler),
        )
        .route(
            "/sessions/{session_id}/abort",
            post(chat_control::session_abort_handler),
        )
}

fn runtime_routes() -> Router<BridgeAppState> {
    Router::new()
        .route("/runtime/status", get(runtime_status_handler))
        .route("/runtime/capabilities", get(runtime_capabilities_handler))
}

fn events_routes() -> Router<BridgeAppState> {
    Router::new().route("/events", get(events_ws_handler))
}

async fn api_not_found(Extension(context): Extension<RequestContext>) -> Response {
    ApiError::not_found(&context).into_response()
}

async fn api_method_not_allowed(Extension(context): Extension<RequestContext>) -> Response {
    ApiError::method_not_allowed(&context).into_response()
}

async fn node_info_handler(Extension(context): Extension<RequestContext>) -> Response {
    route_not_implemented(&context, "GET /api/node/info")
}

async fn node_health_handler(Extension(context): Extension<RequestContext>) -> Response {
    route_not_implemented(&context, "GET /api/node/health")
}

async fn session_list_handler(Extension(context): Extension<RequestContext>) -> Response {
    route_not_implemented(&context, "GET /api/sessions")
}

async fn session_detail_handler(
    Path(_session_id): Path<String>,
    Extension(context): Extension<RequestContext>,
) -> Response {
    route_not_implemented(&context, "GET /api/sessions/:session_id")
}

async fn session_history_handler(
    Path(_session_id): Path<String>,
    Extension(context): Extension<RequestContext>,
) -> Response {
    route_not_implemented(&context, "GET /api/sessions/:session_id/history")
}

async fn runtime_status_handler(Extension(context): Extension<RequestContext>) -> Response {
    route_not_implemented(&context, "GET /api/runtime/status")
}

async fn runtime_capabilities_handler(Extension(context): Extension<RequestContext>) -> Response {
    route_not_implemented(&context, "GET /api/runtime/capabilities")
}

fn route_not_implemented(context: &RequestContext, route_name: &'static str) -> Response {
    ApiError::not_implemented(context, route_name).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use reqwest::StatusCode;
    use serde_json::Value;
    use std::time::Duration;

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

    fn test_client() -> Client {
        Client::builder()
            .no_proxy()
            .build()
            .expect("test client should build")
    }

    async fn spawn_test_server() -> BridgeRuntimeHandle {
        let handle = spawn_server(None, crate::BridgeState::default(), test_config())
            .expect("bridge server should start for tests");
        tokio::time::sleep(Duration::from_millis(50)).await;
        handle
    }

    #[tokio::test]
    async fn rejects_missing_bearer_token() {
        let handle = spawn_test_server().await;
        let response = test_client()
            .get(format!("http://{}/api/node/info", handle.local_addr))
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().contains_key("x-request-id"));

        let body: Value = response.json().await.expect("json body should parse");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"]["code"], "UNAUTHORIZED");
    }

    #[tokio::test]
    async fn exposes_authorized_route_skeletons() {
        let handle = spawn_test_server().await;
        let response = test_client()
            .get(format!("http://{}/api/node/info", handle.local_addr))
            .header("Authorization", "Bearer bridge-test-token")
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        assert!(response.headers().contains_key("x-request-id"));

        let body: Value = response.json().await.expect("json body should parse");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"]["code"], "NOT_IMPLEMENTED");
    }

    #[tokio::test]
    async fn reserves_events_websocket_route() {
        let handle = spawn_test_server().await;
        let response = test_client()
            .get(format!("http://{}/api/events", handle.local_addr))
            .header("Authorization", "Bearer bridge-test-token")
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body: Value = response.json().await.expect("json body should parse");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"]["code"], "INVALID_REQUEST");
    }
}
