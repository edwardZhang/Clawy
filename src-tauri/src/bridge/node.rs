use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;

use super::auth::RequestContext;
use super::response::{success, ApiError, ApiErrorDetail};
use super::server::BridgeAppState;

const BRIDGE_NODE_IDENTITY_FILE_NAME: &str = "node-identity.json";

#[derive(Debug, Clone)]
pub(crate) enum RuntimeIssue {
    Internal { detail: String },
    GatewayNotRunning { detail: Option<String> },
    OpenClawUnreachable { detail: String },
}

impl RuntimeIssue {
    pub(crate) fn to_api_error(&self, context: &RequestContext) -> ApiError {
        match self {
            Self::Internal { detail } => ApiError::custom(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "Bridge failed to inspect local runtime state",
                Some(detail.clone()),
                "bridge",
                false,
                context,
            ),
            Self::GatewayNotRunning { detail } => ApiError::custom(
                StatusCode::SERVICE_UNAVAILABLE,
                "GATEWAY_NOT_RUNNING",
                "Gateway is not running",
                detail.clone(),
                "gateway",
                true,
                context,
            ),
            Self::OpenClawUnreachable { detail } => ApiError::custom(
                StatusCode::SERVICE_UNAVAILABLE,
                "OPENCLAW_UNREACHABLE",
                "OpenClaw session store is not reachable",
                Some(detail.clone()),
                "openclaw",
                true,
                context,
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeHealthSnapshot {
    pub(crate) gateway_running: bool,
    pub(crate) openclaw_reachable: bool,
    pub(crate) runtime_ready: bool,
    pub(crate) last_error: Option<ApiErrorDetail>,
    pub(crate) issue: Option<RuntimeIssue>,
}

#[derive(Debug, Serialize)]
struct NodeInfoData {
    node_id: String,
    node_name: String,
    machine_name: String,
    clawy_version: String,
    os: String,
    arch: String,
}

#[derive(Debug, Serialize)]
struct NodeHealthData {
    online: bool,
    gateway_running: bool,
    openclaw_reachable: bool,
    runtime_ready: bool,
    last_error: Option<ApiErrorDetail>,
    last_seen_at: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct PersistedNodeIdentity {
    #[serde(default)]
    version: u8,
    #[serde(default)]
    node_id: String,
    #[serde(default)]
    created_at: String,
}

pub(crate) async fn node_info_handler(
    State(state): State<BridgeAppState>,
    Extension(context): Extension<RequestContext>,
) -> Response {
    match build_node_info(&state) {
        Ok(data) => success(StatusCode::OK, &context.request_id, data),
        Err(error) => ApiError::custom(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "Failed to build node info",
            Some(error),
            "bridge",
            false,
            &context,
        )
        .into_response(),
    }
}

pub(crate) async fn node_health_handler(
    State(state): State<BridgeAppState>,
    Extension(context): Extension<RequestContext>,
) -> Response {
    let snapshot = evaluate_runtime_health(&state, &context);
    success(
        StatusCode::OK,
        &context.request_id,
        NodeHealthData {
            online: true,
            gateway_running: snapshot.gateway_running,
            openclaw_reachable: snapshot.openclaw_reachable,
            runtime_ready: snapshot.runtime_ready,
            last_error: snapshot.last_error,
            last_seen_at: crate::now_iso_string(),
        },
    )
}

pub(crate) fn evaluate_runtime_health(
    state: &BridgeAppState,
    context: &RequestContext,
) -> RuntimeHealthSnapshot {
    let gateway_snapshot = crate::gateway_status_snapshot(&state.bridge_state);
    let gateway_running = gateway_snapshot
        .as_ref()
        .map(is_gateway_running)
        .unwrap_or(false);
    let openclaw_reachable = openclaw_store_is_reachable(&state.config.openclaw_config_dir).is_ok();

    let issue = match (gateway_snapshot, gateway_running, openclaw_reachable) {
        (Err(detail), _, _) => Some(RuntimeIssue::Internal { detail }),
        (Ok(status), false, _) => Some(RuntimeIssue::GatewayNotRunning {
            detail: status.error.clone().or_else(|| {
                Some(format!(
                    "Gateway state is `{}` and no live process is attached.",
                    status.state
                ))
            }),
        }),
        (Ok(_), true, false) => Some(RuntimeIssue::OpenClawUnreachable {
            detail: openclaw_store_is_reachable(&state.config.openclaw_config_dir)
                .unwrap_err_or_else(|| "OpenClaw session store is not readable".into()),
        }),
        (Ok(_), true, true) => None,
    };

    let last_error = issue
        .as_ref()
        .map(|runtime_issue| runtime_issue.to_api_error(context).detail());

    RuntimeHealthSnapshot {
        gateway_running,
        openclaw_reachable,
        runtime_ready: gateway_running && openclaw_reachable,
        last_error,
        issue,
    }
}

pub(crate) fn ensure_runtime_ready(
    state: &BridgeAppState,
    context: &RequestContext,
) -> Result<(), Response> {
    let snapshot = evaluate_runtime_health(state, context);
    match snapshot.issue {
        Some(issue) => Err(issue.to_api_error(context).into_response()),
        None => Ok(()),
    }
}

pub(crate) fn load_or_create_node_id(base_dir: &Path) -> Result<String, String> {
    let path = node_identity_path(base_dir);

    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(identity) = serde_json::from_str::<PersistedNodeIdentity>(&content) {
                if is_valid_node_id(&identity.node_id) {
                    return Ok(identity.node_id);
                }
            }
        }
    }

    let identity = PersistedNodeIdentity {
        version: 1,
        node_id: format!("node_{}", Uuid::new_v4()),
        created_at: crate::now_iso_string(),
    };
    crate::write_json(&path, &identity)?;
    Ok(identity.node_id)
}

fn build_node_info(state: &BridgeAppState) -> Result<NodeInfoData, String> {
    let machine_name = machine_name();
    let node_id = if state.config.node_id.trim().is_empty() {
        load_or_create_node_id(&state.config.clawy_base_dir)?
    } else {
        state.config.node_id.clone()
    };

    Ok(NodeInfoData {
        node_id,
        node_name: derive_node_name(&machine_name),
        machine_name,
        clawy_version: state
            .app_handle
            .as_ref()
            .map(|app_handle| app_handle.package_info().version.to_string())
            .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string()),
        os: std::env::consts::OS.to_string(),
        arch: normalize_arch(std::env::consts::ARCH),
    })
}

fn node_identity_path(base_dir: &Path) -> PathBuf {
    base_dir.join("bridge").join(BRIDGE_NODE_IDENTITY_FILE_NAME)
}

fn is_valid_node_id(node_id: &str) -> bool {
    node_id.starts_with("node_") && !node_id.trim().eq("node_")
}

fn machine_name() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(read_hostname_command)
        .unwrap_or_else(|| "localhost".into())
}

fn read_hostname_command() -> Option<String> {
    let output = Command::new("hostname").output().ok()?;
    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn derive_node_name(machine_name: &str) -> String {
    let mut normalized = String::new();
    let mut last_was_dash = false;

    for ch in machine_name.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash {
            normalized.push('-');
            last_was_dash = true;
        }
    }

    let normalized = normalized.trim_matches('-').to_string();
    if normalized.is_empty() {
        "node".into()
    } else {
        normalized
    }
}

fn normalize_arch(arch: &str) -> String {
    match arch {
        "aarch64" => "arm64".into(),
        "x86_64" => "x64".into(),
        other => other.to_string(),
    }
}

fn is_gateway_running(status: &crate::GatewayStatus) -> bool {
    matches!(
        status.state.as_str(),
        "starting" | "running" | "reconnecting"
    ) || status.pid.is_some()
}

fn openclaw_store_is_reachable(openclaw_config_dir: &Path) -> Result<(), String> {
    let metadata = fs::metadata(openclaw_config_dir).map_err(|error| {
        format!(
            "Could not read OpenClaw config dir `{}`: {error}",
            openclaw_config_dir.display()
        )
    })?;

    if !metadata.is_dir() {
        return Err(format!(
            "OpenClaw config path `{}` is not a directory",
            openclaw_config_dir.display()
        ));
    }

    let agents_dir = openclaw_config_dir.join("agents");
    if agents_dir.exists() {
        fs::read_dir(&agents_dir).map_err(|error| {
            format!(
                "Could not read OpenClaw agents dir `{}`: {error}",
                agents_dir.display()
            )
        })?;
    }

    Ok(())
}

trait ResultExt<T> {
    fn unwrap_err_or_else(self, fallback: impl FnOnce() -> String) -> String;
}

impl<T, E: ToString> ResultExt<T> for Result<T, E> {
    fn unwrap_err_or_else(self, fallback: impl FnOnce() -> String) -> String {
        match self {
            Ok(_) => fallback(),
            Err(error) => error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::server::{spawn_server, BridgeRuntimeConfig, BridgeRuntimeHandle};
    use reqwest::{Client, StatusCode};
    use serde_json::Value;
    use std::fs;
    use std::net::SocketAddr;
    use std::time::Duration;

    fn test_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("clawy-bridge-{name}-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).expect("test dir should exist");
        path
    }

    fn test_config(clawy_base_dir: PathBuf, openclaw_config_dir: PathBuf) -> BridgeRuntimeConfig {
        BridgeRuntimeConfig {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
            auth_token: "bridge-test-token".into(),
            allowed_origins: Vec::new(),
            clawy_base_dir,
            node_id: "node_test".into(),
            openclaw_config_dir,
        }
    }

    fn test_client() -> Client {
        Client::builder()
            .no_proxy()
            .build()
            .expect("test client should build")
    }

    async fn spawn_test_server(config: BridgeRuntimeConfig) -> BridgeRuntimeHandle {
        let handle = spawn_server(None, crate::BridgeState::default(), config)
            .expect("bridge server should start for tests");
        tokio::time::sleep(Duration::from_millis(50)).await;
        handle
    }

    #[tokio::test]
    async fn node_info_uses_configured_node_id() {
        let clawy_base_dir = test_dir("node-id");
        let openclaw_config_dir = test_dir("node-openclaw");
        let first = spawn_test_server(test_config(
            clawy_base_dir.clone(),
            openclaw_config_dir.clone(),
        ))
        .await;
        let first_body: Value = test_client()
            .get(format!("http://{}/api/node/info", first.local_addr))
            .header("Authorization", "Bearer bridge-test-token")
            .send()
            .await
            .expect("first request should succeed")
            .json()
            .await
            .expect("first response should be json");
        let first_node_id = first_body["data"]["node_id"]
            .as_str()
            .expect("node_id should be present")
            .to_string();

        let second = spawn_test_server(test_config(clawy_base_dir, openclaw_config_dir)).await;
        let second_body: Value = test_client()
            .get(format!("http://{}/api/node/info", second.local_addr))
            .header("Authorization", "Bearer bridge-test-token")
            .send()
            .await
            .expect("second request should succeed")
            .json()
            .await
            .expect("second response should be json");
        let second_node_id = second_body["data"]["node_id"]
            .as_str()
            .expect("node_id should be present")
            .to_string();

        assert!(first_node_id.starts_with("node_"));
        assert_eq!(first_node_id, second_node_id);
    }

    #[tokio::test]
    async fn node_health_reports_gateway_down_with_structured_error() {
        let handle =
            spawn_test_server(test_config(test_dir("health"), test_dir("health-oc"))).await;
        let response = test_client()
            .get(format!("http://{}/api/node/health", handle.local_addr))
            .header("Authorization", "Bearer bridge-test-token")
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::OK);

        let body: Value = response.json().await.expect("json body should parse");
        assert_eq!(body["ok"], true);
        assert_eq!(body["data"]["online"], true);
        assert_eq!(body["data"]["gateway_running"], false);
        assert_eq!(body["data"]["openclaw_reachable"], true);
        assert_eq!(body["data"]["runtime_ready"], false);
        assert_eq!(body["data"]["last_error"]["code"], "GATEWAY_NOT_RUNNING");
    }
}
