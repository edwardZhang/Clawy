use reqwest::{Client, StatusCode, Url};
use serde::Serialize;
use tauri::AppHandle;
use time::OffsetDateTime;

use super::capabilities::{self, RuntimeCapabilitiesResponse};
use super::errors::BridgeError;
use super::node::{self, NodeInfoData};
use super::response::BRIDGE_API_VERSION;
use super::runtime::{self, RuntimeStatusResponse};
use super::server::{BridgeAppState, BridgeRuntimeHandle};

const DEFAULT_REGISTRY_TIMEOUT_SECS: u64 = 10;
const DEFAULT_REGISTRY_RETRY_DELAY_SECS: u64 = 15;
const MIN_REGISTRY_HEARTBEAT_INTERVAL_SECS: u64 = 15;
const MAX_REGISTRY_HEARTBEAT_INTERVAL_SECS: u64 = 300;

static REGISTRY_CLIENT_STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();

#[derive(Debug, Clone)]
pub(crate) struct RegistryClientConfig {
    pub(crate) base_url: String,
    pub(crate) register_url: String,
    pub(crate) heartbeat_url: String,
    pub(crate) heartbeat_interval_secs: u64,
    pub(crate) auth_token: Option<String>,
    pub(crate) public_base_url: String,
}

#[derive(Debug, Clone, Serialize)]
struct BridgeAdvertisement {
    base_url: String,
    api_base_url: String,
    api_version: &'static str,
    auth_scheme: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    auth_token: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct RegisterNodeRequest {
    node_id: String,
    ts: String,
    bridge: BridgeAdvertisement,
    node: NodeInfoData,
    runtime: RuntimeStatusResponse,
    capabilities: RuntimeCapabilitiesResponse,
}

#[derive(Debug, Clone, Serialize)]
struct HeartbeatNodeRequest {
    node_id: String,
    ts: String,
    bridge: HeartbeatBridgeAdvertisement,
    runtime: RuntimeStatusResponse,
}

#[derive(Debug, Clone, Serialize)]
struct HeartbeatBridgeAdvertisement {
    api_version: &'static str,
    base_url: String,
}

pub(crate) fn start_registry_client(
    app_handle: AppHandle,
    bridge_state: crate::BridgeState,
    runtime_handle: BridgeRuntimeHandle,
    config: Option<RegistryClientConfig>,
) -> Result<(), String> {
    let Some(config) = config else {
        crate::append_log_line(
            "INFO",
            "Clawy Bridge registry client disabled: no ForgeAI registry URL configured",
        );
        return Ok(());
    };

    if REGISTRY_CLIENT_STARTED.get().is_some() {
        return Ok(());
    }

    let _ = REGISTRY_CLIENT_STARTED.set(());

    crate::append_log_line(
        "INFO",
        &format!(
            "Clawy Bridge registry client enabled: base_url={} heartbeat_secs={}",
            config.base_url, config.heartbeat_interval_secs
        ),
    );

    tauri::async_runtime::spawn(async move {
        if let Err(error) =
            run_registry_loop(app_handle, bridge_state, runtime_handle, config).await
        {
            crate::append_log_line(
                "WARN",
                &format!("Clawy Bridge registry client stopped: {error}"),
            );
        }
    });

    Ok(())
}

pub(crate) fn registry_client_config(
    base_url: impl Into<String>,
    register_path: impl AsRef<str>,
    heartbeat_path: impl AsRef<str>,
    heartbeat_interval_secs: u64,
    auth_token: Option<String>,
    public_base_url: impl Into<String>,
) -> Result<RegistryClientConfig, String> {
    let base_url = normalize_base_url(base_url.into())?;
    let public_base_url = normalize_base_url(public_base_url.into())?;
    let register_url = join_registry_url(&base_url, register_path.as_ref())?;
    let heartbeat_url = join_registry_url(&base_url, heartbeat_path.as_ref())?;
    let heartbeat_interval_secs = heartbeat_interval_secs.clamp(
        MIN_REGISTRY_HEARTBEAT_INTERVAL_SECS,
        MAX_REGISTRY_HEARTBEAT_INTERVAL_SECS,
    );
    let auth_token = auth_token
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty());

    Ok(RegistryClientConfig {
        base_url,
        register_url,
        heartbeat_url,
        heartbeat_interval_secs,
        auth_token,
        public_base_url,
    })
}

async fn run_registry_loop(
    app_handle: AppHandle,
    bridge_state: crate::BridgeState,
    runtime_handle: BridgeRuntimeHandle,
    config: RegistryClientConfig,
) -> Result<(), String> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(
            DEFAULT_REGISTRY_TIMEOUT_SECS,
        ))
        .build()
        .map_err(|error| format!("Failed to build ForgeAI registry client: {error}"))?;

    let mut registered = false;

    loop {
        let app_state = registry_app_state(
            Some(app_handle.clone()),
            bridge_state.clone(),
            &runtime_handle,
        );

        let request_result = if registered {
            send_heartbeat_request(&client, &config, &app_state).await
        } else {
            send_register_request(&client, &config, &app_state).await
        };

        match request_result {
            Ok(_) => {
                registered = true;
                tokio::time::sleep(std::time::Duration::from_secs(
                    config.heartbeat_interval_secs,
                ))
                .await;
            }
            Err(error) => {
                crate::append_log_line(
                    "WARN",
                    &format!("Clawy Bridge registry request failed: {error}"),
                );
                registered = false;
                tokio::time::sleep(std::time::Duration::from_secs(
                    DEFAULT_REGISTRY_RETRY_DELAY_SECS,
                ))
                .await;
            }
        }
    }
}

fn registry_app_state(
    app_handle: Option<AppHandle>,
    bridge_state: crate::BridgeState,
    runtime_handle: &BridgeRuntimeHandle,
) -> BridgeAppState {
    BridgeAppState {
        app_handle,
        bridge_state,
        config: runtime_handle.config.clone(),
        events_tx: runtime_handle.events_tx.clone(),
    }
}

async fn send_register_request(
    client: &Client,
    config: &RegistryClientConfig,
    app_state: &BridgeAppState,
) -> Result<(), String> {
    let payload = build_register_payload(app_state, config)?;
    send_registry_request(
        client,
        &config.register_url,
        config.auth_token.as_deref(),
        &payload,
    )
    .await
}

async fn send_heartbeat_request(
    client: &Client,
    config: &RegistryClientConfig,
    app_state: &BridgeAppState,
) -> Result<(), String> {
    let payload = build_heartbeat_payload(app_state, config)?;
    send_registry_request(
        client,
        &config.heartbeat_url,
        config.auth_token.as_deref(),
        &payload,
    )
    .await
}

fn build_register_payload(
    app_state: &BridgeAppState,
    config: &RegistryClientConfig,
) -> Result<RegisterNodeRequest, String> {
    let node = node::build_node_info(app_state)?;
    let runtime = runtime::build_runtime_status(app_state).map_err(bridge_error_message)?;
    let capabilities =
        capabilities::build_runtime_capabilities(app_state).map_err(bridge_error_message)?;
    let node_id = app_state.config.node_id.clone();

    Ok(RegisterNodeRequest {
        node_id,
        ts: now_timestamp(),
        bridge: BridgeAdvertisement {
            base_url: config.public_base_url.clone(),
            api_base_url: format!(
                "{}/api/{}",
                config.public_base_url.trim_end_matches('/'),
                BRIDGE_API_VERSION
            ),
            api_version: BRIDGE_API_VERSION,
            auth_scheme: "bearer",
            auth_token: Some(app_state.config.auth_token.clone())
                .filter(|token| !token.trim().is_empty()),
        },
        node,
        runtime,
        capabilities,
    })
}

fn build_heartbeat_payload(
    app_state: &BridgeAppState,
    config: &RegistryClientConfig,
) -> Result<HeartbeatNodeRequest, String> {
    let runtime = runtime::build_runtime_status(app_state).map_err(bridge_error_message)?;

    Ok(HeartbeatNodeRequest {
        node_id: app_state.config.node_id.clone(),
        ts: now_timestamp(),
        bridge: HeartbeatBridgeAdvertisement {
            api_version: BRIDGE_API_VERSION,
            base_url: config.public_base_url.clone(),
        },
        runtime,
    })
}

async fn send_registry_request<T: Serialize>(
    client: &Client,
    url: &str,
    auth_token: Option<&str>,
    payload: &T,
) -> Result<(), String> {
    let mut request = client.post(url).json(payload);
    if let Some(auth_token) = auth_token.filter(|token| !token.trim().is_empty()) {
        request = request.bearer_auth(auth_token.trim());
    }

    let response = request
        .send()
        .await
        .map_err(|error| format!("Failed to reach ForgeAI registry `{url}`: {error}"))?;

    if response.status().is_success() {
        return Ok(());
    }

    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| "<body unreadable>".into());
    Err(format_registry_http_error(url, status, &body))
}

fn format_registry_http_error(url: &str, status: StatusCode, body: &str) -> String {
    let trimmed = body.trim();
    let detail = if trimmed.is_empty() {
        "empty response body".into()
    } else {
        trimmed.chars().take(240).collect::<String>()
    };
    format!(
        "ForgeAI registry `{url}` responded with {}: {detail}",
        status.as_u16()
    )
}

fn bridge_error_message(error: BridgeError) -> String {
    let body = error.body();
    body.detail.unwrap_or(body.message)
}

fn join_registry_url(base_url: &str, path: &str) -> Result<String, String> {
    let base = Url::parse(&format!("{}/", base_url.trim_end_matches('/')))
        .map_err(|error| format!("Invalid ForgeAI registry base URL `{base_url}`: {error}"))?;
    let joined = base
        .join(path.trim_start_matches('/'))
        .map_err(|error| format!("Invalid ForgeAI registry path `{path}`: {error}"))?;
    Ok(joined.to_string())
}

fn normalize_base_url(value: String) -> Result<String, String> {
    let value = value.trim().trim_end_matches('/').to_string();
    if value.is_empty() {
        return Err("ForgeAI registry URL must not be empty".into());
    }

    let parsed = Url::parse(&value)
        .map_err(|error| format!("Invalid ForgeAI registry URL `{value}`: {error}"))?;
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(format!(
            "ForgeAI registry URL must use http or https, got `{scheme}`"
        ));
    }

    Ok(value)
}

fn now_timestamp() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode as HttpStatusCode};
    use axum::routing::post;
    use axum::{Json, Router};
    use serde_json::{json, Value};
    use std::fs;
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use tokio::sync::oneshot;

    #[derive(Clone, Default)]
    struct CaptureState {
        entries: Arc<Mutex<Vec<(String, Value, Option<String>)>>>,
    }

    fn test_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "clawy-bridge-registry-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&path).expect("test dir should exist");
        path
    }

    fn test_runtime_handle(
        clawy_base_dir: PathBuf,
        openclaw_config_dir: PathBuf,
    ) -> BridgeRuntimeHandle {
        let (events_tx, _) = tokio::sync::broadcast::channel(16);
        BridgeRuntimeHandle {
            local_addr: SocketAddr::from(([127, 0, 0, 1], 18790)),
            config: super::super::server::BridgeRuntimeConfig {
                listen_addr: SocketAddr::from(([127, 0, 0, 1], 18790)),
                auth_token: "bridge-node-token".into(),
                lan_enabled: false,
                trusted_remote_cidrs: Vec::new(),
                allowed_origins: Vec::new(),
                clawy_base_dir,
                node_id: "node_test".into(),
                openclaw_config_dir,
            },
            events_tx,
        }
    }

    fn test_app_state(runtime_handle: &BridgeRuntimeHandle) -> BridgeAppState {
        BridgeAppState {
            app_handle: None,
            bridge_state: crate::BridgeState::default(),
            config: runtime_handle.config.clone(),
            events_tx: runtime_handle.events_tx.clone(),
        }
    }

    async fn capture_handler(
        State(state): State<CaptureState>,
        headers: HeaderMap,
        Json(payload): Json<Value>,
    ) -> (HttpStatusCode, Json<Value>) {
        let auth = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let path = payload
            .get("capabilities")
            .map(|_| "register")
            .unwrap_or("heartbeat")
            .to_string();
        state
            .entries
            .lock()
            .expect("capture lock should succeed")
            .push((path, payload, auth));
        (HttpStatusCode::OK, Json(json!({ "ok": true })))
    }

    async fn spawn_capture_server() -> (String, CaptureState, oneshot::Sender<()>) {
        let state = CaptureState::default();
        let app = Router::new()
            .route("/api/nodes/register", post(capture_handler))
            .route("/api/nodes/heartbeat", post(capture_handler))
            .with_state(state.clone());

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("listener should bind");
        let addr = listener.local_addr().expect("listener addr should exist");
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        tauri::async_runtime::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await;
        });

        (format!("http://{addr}"), state, shutdown_tx)
    }

    #[tokio::test]
    async fn registry_client_posts_register_and_heartbeat_payloads() {
        let clawy_base_dir = test_dir("base");
        let openclaw_config_dir = test_dir("openclaw");
        let runtime_handle = test_runtime_handle(clawy_base_dir, openclaw_config_dir);
        let app_state = test_app_state(&runtime_handle);
        let (base_url, capture_state, shutdown_tx) = spawn_capture_server().await;
        let config = registry_client_config(
            base_url.clone(),
            "/api/nodes/register",
            "/api/nodes/heartbeat",
            30,
            Some("forge-registry-token".into()),
            "http://127.0.0.1:18790",
        )
        .expect("registry config should build");
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .no_proxy()
            .build()
            .expect("client should build");

        send_register_request(&client, &config, &app_state)
            .await
            .expect("register request should succeed");
        send_heartbeat_request(&client, &config, &app_state)
            .await
            .expect("heartbeat request should succeed");

        let entries = capture_state
            .entries
            .lock()
            .expect("capture lock should succeed")
            .clone();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, "register");
        assert_eq!(entries[0].1["node_id"], "node_test");
        assert_eq!(entries[0].1["bridge"]["api_version"], BRIDGE_API_VERSION);
        assert_eq!(entries[0].1["bridge"]["auth_scheme"], "bearer");
        assert_eq!(entries[0].1["bridge"]["auth_token"], "bridge-node-token");
        assert!(entries[0].1.get("capabilities").is_some());
        assert_eq!(entries[0].2.as_deref(), Some("Bearer forge-registry-token"));

        assert_eq!(entries[1].0, "heartbeat");
        assert_eq!(entries[1].1["node_id"], "node_test");
        assert!(entries[1].1.get("runtime").is_some());
        assert!(entries[1].1.get("capabilities").is_none());

        let _ = shutdown_tx.send(());
    }

    #[test]
    fn registry_config_normalizes_urls_and_interval() {
        let config = registry_client_config(
            "https://forge.example.com/",
            "api/nodes/register",
            "/api/nodes/heartbeat",
            1,
            Some(" token ".into()),
            "http://127.0.0.1:18790/",
        )
        .expect("registry config should build");

        assert_eq!(config.base_url, "https://forge.example.com");
        assert_eq!(
            config.register_url,
            "https://forge.example.com/api/nodes/register"
        );
        assert_eq!(
            config.heartbeat_url,
            "https://forge.example.com/api/nodes/heartbeat"
        );
        assert_eq!(
            config.heartbeat_interval_secs,
            MIN_REGISTRY_HEARTBEAT_INTERVAL_SECS
        );
        assert_eq!(config.auth_token.as_deref(), Some("token"));
        assert_eq!(config.public_base_url, "http://127.0.0.1:18790");
    }
}
