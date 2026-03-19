use axum::extract::Extension;
use axum::http::StatusCode;
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use ipnet::IpNet;
use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::path::PathBuf;
use tauri::AppHandle;
use tokio::sync::broadcast;

use super::audit;
use super::auth::{self, RequestContext};
use super::capabilities;
use super::chat_control;
use super::events::BridgeEventEnvelope;
use super::node;
use super::response::ApiError;
use super::runtime;
use super::sessions;
use super::ws::events_ws_handler;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct BridgeRuntimeConfig {
    pub(crate) listen_addr: SocketAddr,
    pub(crate) auth_token: String,
    pub(crate) lan_enabled: bool,
    pub(crate) trusted_remote_cidrs: Vec<IpNet>,
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
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            audit::log_http_request,
        ));

    Router::new()
        .nest("/api", api_router.clone())
        .nest("/api/v1", api_router)
        .with_state(state)
}

fn node_routes() -> Router<BridgeAppState> {
    Router::new()
        .route("/node/info", get(node::node_info_handler))
        .route("/node/health", get(node::node_health_handler))
}

fn session_routes() -> Router<BridgeAppState> {
    Router::new()
        .route("/sessions", get(sessions::session_list_handler))
        .route(
            "/sessions/{session_id}",
            get(sessions::session_detail_handler),
        )
        .route(
            "/sessions/{session_id}/history",
            get(sessions::session_history_handler),
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

async fn runtime_status_handler(
    axum::extract::State(state): axum::extract::State<BridgeAppState>,
    Extension(context): Extension<RequestContext>,
) -> Response {
    match runtime::build_runtime_status(&state) {
        Ok(snapshot) => super::response::success(StatusCode::OK, &context.request_id, snapshot),
        Err(error) => error.into_response_with_request_id(&context.request_id),
    }
}

async fn runtime_capabilities_handler(
    axum::extract::State(state): axum::extract::State<BridgeAppState>,
    Extension(context): Extension<RequestContext>,
) -> Response {
    match capabilities::build_runtime_capabilities(&state) {
        Ok(snapshot) => super::response::success(StatusCode::OK, &context.request_id, snapshot),
        Err(error) => error.into_response_with_request_id(&context.request_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use reqwest::StatusCode;
    use serde_json::Value;
    use std::fs;
    use std::path::Path;
    use std::time::Duration;

    fn test_config() -> BridgeRuntimeConfig {
        BridgeRuntimeConfig {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
            auth_token: "bridge-test-token".into(),
            lan_enabled: false,
            trusted_remote_cidrs: Vec::new(),
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

    fn test_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "clawy-bridge-server-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(path.join("bridge")).expect("test dir should exist");
        path
    }

    fn write_permissions_config(base_dir: &Path) {
        let config_path = base_dir.join("bridge").join("bridge-permissions.json");
        fs::write(
            config_path,
            serde_json::to_string(&serde_json::json!({
                "defaultProfile": "read_only",
                "callerProfiles": {
                    "readonly-client": "read_only",
                    "writer-client": "read_write"
                },
                "profiles": {
                    "read_only": {
                        "read": true,
                        "write": false,
                        "allowedSessions": ["agent:main:main"],
                        "capabilityTags": ["bridge.read", "bridge.session.scoped"]
                    },
                    "read_write": {
                        "read": true,
                        "write": true,
                        "allowedSessions": ["agent:main:main"],
                        "capabilityTags": ["bridge.read", "bridge.write", "bridge.session.scoped"]
                    }
                }
            }))
            .expect("permissions config should serialize"),
        )
        .expect("permissions config should write");
    }

    async fn spawn_test_server() -> BridgeRuntimeHandle {
        let handle = spawn_server(None, crate::BridgeState::default(), test_config())
            .expect("bridge server should start for tests");
        tokio::time::sleep(Duration::from_millis(50)).await;
        handle
    }

    async fn spawn_test_server_with_config(config: BridgeRuntimeConfig) -> BridgeRuntimeHandle {
        let handle = spawn_server(None, crate::BridgeState::default(), config)
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
            .get(format!("http://{}/api/v1/node/info", handle.local_addr))
            .header("Authorization", "Bearer bridge-test-token")
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("x-request-id"));
        assert_eq!(
            response
                .headers()
                .get("x-clawy-bridge-api-version")
                .and_then(|value| value.to_str().ok()),
            Some("v1")
        );

        let body: Value = response.json().await.expect("json body should parse");
        assert_eq!(body["ok"], true);
        assert_eq!(body["data"]["node_id"], "node_test");
        assert_eq!(body["data"]["os"], std::env::consts::OS);
        assert_eq!(body["permissions"]["profile"], "read_write");
        assert_eq!(body["permissions"]["read"], true);
        assert_eq!(body["permissions"]["write"], true);
        assert!(body["permissions"]["capabilities"].is_array());
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

    #[tokio::test]
    async fn read_only_caller_can_read_but_cannot_write() {
        let clawy_base_dir = test_dir("readonly");
        write_permissions_config(&clawy_base_dir);
        let config = BridgeRuntimeConfig {
            clawy_base_dir,
            ..test_config()
        };
        let handle = spawn_test_server_with_config(config).await;

        let read_response = test_client()
            .get(format!("http://{}/api/v1/node/info", handle.local_addr))
            .header("Authorization", "Bearer bridge-test-token")
            .header("x-clawy-caller-id", "readonly-client")
            .send()
            .await
            .expect("read request should succeed");
        assert_eq!(read_response.status(), StatusCode::OK);

        let read_body: Value = read_response.json().await.expect("json body should parse");
        assert_eq!(read_body["permissions"]["profile"], "read_only");
        assert_eq!(read_body["permissions"]["read"], true);
        assert_eq!(read_body["permissions"]["write"], false);

        let write_response = test_client()
            .post(format!(
                "http://{}/api/v1/sessions/agent:main:main/send",
                handle.local_addr
            ))
            .header("Authorization", "Bearer bridge-test-token")
            .header("x-clawy-caller-id", "readonly-client")
            .json(&serde_json::json!({ "message": "hello" }))
            .send()
            .await
            .expect("write request should succeed");

        let write_body: Value = write_response.json().await.expect("json body should parse");
        assert_eq!(write_body["ok"], false);
        assert_eq!(write_body["error"]["code"], "FORBIDDEN_PERMISSION");
    }

    #[tokio::test]
    async fn write_caller_can_reach_send_and_abort_paths() {
        let clawy_base_dir = test_dir("writer");
        write_permissions_config(&clawy_base_dir);
        let config = BridgeRuntimeConfig {
            clawy_base_dir,
            ..test_config()
        };
        let handle = spawn_test_server_with_config(config).await;

        let send_response = test_client()
            .post(format!(
                "http://{}/api/v1/sessions/agent:main:main/send",
                handle.local_addr
            ))
            .header("Authorization", "Bearer bridge-test-token")
            .header("x-clawy-caller-id", "writer-client")
            .json(&serde_json::json!({ "message": "hello" }))
            .send()
            .await
            .expect("send request should succeed");
        assert_ne!(send_response.status(), StatusCode::FORBIDDEN);

        let send_body: Value = send_response.json().await.expect("json body should parse");
        assert_ne!(send_body["error"]["code"], "FORBIDDEN_PERMISSION");

        let abort_response = test_client()
            .post(format!(
                "http://{}/api/v1/sessions/agent:main:main/abort",
                handle.local_addr
            ))
            .header("Authorization", "Bearer bridge-test-token")
            .header("x-clawy-caller-id", "writer-client")
            .send()
            .await
            .expect("abort request should succeed");
        assert_ne!(abort_response.status(), StatusCode::FORBIDDEN);

        let abort_body: Value = abort_response.json().await.expect("json body should parse");
        assert_ne!(abort_body["error"]["code"], "FORBIDDEN_PERMISSION");
    }

    #[tokio::test]
    async fn session_scope_restrictions_return_structured_error() {
        let clawy_base_dir = test_dir("session-scope");
        write_permissions_config(&clawy_base_dir);
        let config = BridgeRuntimeConfig {
            clawy_base_dir,
            ..test_config()
        };
        let handle = spawn_test_server_with_config(config).await;

        let response = test_client()
            .get(format!(
                "http://{}/api/v1/sessions/agent:other:chat/history",
                handle.local_addr
            ))
            .header("Authorization", "Bearer bridge-test-token")
            .header("x-clawy-caller-id", "readonly-client")
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let body: Value = response.json().await.expect("json body should parse");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"]["code"], "SESSION_NOT_ALLOWED");
    }
}
