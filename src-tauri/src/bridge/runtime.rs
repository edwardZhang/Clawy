use serde::Serialize;
use serde_json::{json, Value};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::errors::{BridgeError, BridgeErrorBody, BridgeResult};
use super::server::BridgeAppState;

#[derive(Debug, Clone, Serialize, Default)]
pub(crate) struct ModelSelection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) current_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) current_provider_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) current_model: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) fallback_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RuntimeStatusResponse {
    pub(crate) gateway_running: bool,
    pub(crate) gateway_port: u16,
    pub(crate) gateway_reachable: bool,
    pub(crate) dashboard_reachable: bool,
    pub(crate) runtime_ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) current_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) current_provider: Option<String>,
    pub(crate) config_dir: String,
    pub(crate) openclaw_dir: String,
    pub(crate) connection_status: String,
    pub(crate) gateway_status: RuntimeGatewayStatus,
    pub(crate) openclaw_runtime: OpenclawRuntimeSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_error: Option<BridgeErrorBody>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RuntimeGatewayStatus {
    pub(crate) state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) uptime_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) connected_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reconnect_attempts: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct OpenclawRuntimeSnapshot {
    pub(crate) mode: String,
    pub(crate) full_mode_runtime: bool,
    pub(crate) ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    pub(crate) dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) entry_path: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) diagnostics: Vec<String>,
}

pub(crate) fn build_runtime_status(state: &BridgeAppState) -> BridgeResult<RuntimeStatusResponse> {
    let gateway_status = crate::gateway_status_snapshot(&state.bridge_state).map_err(|error| {
        BridgeError::internal(format!("Failed to snapshot gateway status: {error}"))
    })?;

    let gateway_reachable = tcp_port_is_reachable(gateway_status.port);
    let dashboard_reachable = gateway_reachable && gateway_status.connected_at.is_some();
    let gateway_running = is_gateway_running(&gateway_status.state);

    let runtime_resolution = resolve_openclaw_runtime_for_bridge(state);
    let openclaw_dir = runtime_resolution
        .dir
        .clone()
        .unwrap_or_else(crate::workspace_openclaw_dir);
    let runtime_ready = runtime_resolution.entry_path.is_some();

    let mut last_error = operational_last_error(
        &gateway_status,
        gateway_running,
        gateway_reachable,
        runtime_ready,
        &runtime_resolution,
    );

    let model_selection = match resolve_model_selection(
        &state.config.clawy_base_dir,
        &state.config.openclaw_config_dir,
    ) {
        Ok(selection) => selection,
        Err(error) => {
            if last_error.is_none() {
                last_error = Some(error.body());
            }
            ModelSelection::default()
        }
    };

    Ok(RuntimeStatusResponse {
        gateway_running,
        gateway_port: gateway_status.port,
        gateway_reachable,
        dashboard_reachable,
        runtime_ready,
        current_model: model_selection.current_model.clone(),
        current_provider: model_selection.current_provider.clone(),
        config_dir: state
            .config
            .openclaw_config_dir
            .to_string_lossy()
            .to_string(),
        openclaw_dir: openclaw_dir.to_string_lossy().to_string(),
        connection_status: derive_connection_status(
            &gateway_status.state,
            gateway_reachable,
            dashboard_reachable,
        ),
        gateway_status: RuntimeGatewayStatus {
            state: gateway_status.state,
            pid: gateway_status.pid,
            uptime_ms: gateway_status.uptime,
            error: gateway_status.error,
            connected_at_ms: gateway_status.connected_at,
            version: gateway_status.version,
            reconnect_attempts: gateway_status.reconnect_attempts,
        },
        openclaw_runtime: OpenclawRuntimeSnapshot {
            mode: if crate::full_mode_runtime_enabled() {
                "full".into()
            } else {
                "resolver".into()
            },
            full_mode_runtime: crate::full_mode_runtime_enabled(),
            ready: runtime_ready,
            source: runtime_resolution
                .source
                .map(crate::OpenClawRuntimeSource::as_str)
                .map(str::to_string),
            version: runtime_resolution.version.clone(),
            dir: openclaw_dir.to_string_lossy().to_string(),
            entry_path: runtime_resolution
                .entry_path
                .map(|path| path.to_string_lossy().to_string()),
            diagnostics: runtime_resolution
                .diagnostics
                .iter()
                .map(crate::OpenClawRuntimeDiagnostic::summary)
                .collect(),
        },
        last_error,
    })
}

pub(crate) fn resolve_model_selection(
    clawy_base_dir: &Path,
    openclaw_config_dir: &Path,
) -> BridgeResult<ModelSelection> {
    let config = read_openclaw_json_from_dir(openclaw_config_dir)?;
    let provider_store: crate::ProviderStore =
        crate::read_json_or_default(&clawy_base_dir.join("providers.json"));

    let current_model = current_model_from_config(&config);
    let fallback_models = config
        .get("agents")
        .and_then(Value::as_object)
        .and_then(|agents| agents.get("defaults"))
        .and_then(Value::as_object)
        .and_then(|defaults| defaults.get("model"))
        .and_then(Value::as_object)
        .and_then(|model| model.get("fallbacks"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let matched_provider =
        resolve_current_provider_config(&provider_store, current_model.as_deref());

    Ok(ModelSelection {
        current_provider: matched_provider
            .as_ref()
            .map(|config| config.id.clone())
            .or_else(|| {
                current_model
                    .as_deref()
                    .and_then(model_ref_prefix)
                    .map(str::to_string)
            }),
        current_provider_type: matched_provider
            .as_ref()
            .map(|config| config.provider_type.clone())
            .or_else(|| {
                current_model
                    .as_deref()
                    .and_then(model_ref_prefix)
                    .map(str::to_string)
            }),
        current_model,
        fallback_models,
    })
}

pub(crate) fn read_openclaw_json_from_dir(openclaw_config_dir: &Path) -> BridgeResult<Value> {
    let path = openclaw_json_path(openclaw_config_dir);
    if !path.exists() {
        return Ok(json!({}));
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|error| BridgeError::config_read_failed("openclaw.json", error.to_string()))?;
    serde_json::from_str(&content)
        .map_err(|error| BridgeError::config_read_failed("openclaw.json", error.to_string()))
}

pub(crate) fn resolve_openclaw_runtime_for_bridge(
    state: &BridgeAppState,
) -> crate::OpenClawRuntimeResolution {
    crate::resolve_openclaw_runtime_with_candidates(
        crate::managed_openclaw_dir_from_base(&state.config.clawy_base_dir),
        crate::workspace_openclaw_dir(),
        crate::bundled_openclaw_dir(),
        crate::full_mode_runtime_enabled(),
    )
}

fn openclaw_json_path(openclaw_config_dir: &Path) -> PathBuf {
    openclaw_config_dir.join("openclaw.json")
}

fn current_model_from_config(config: &Value) -> Option<String> {
    config
        .get("agents")
        .and_then(Value::as_object)
        .and_then(|agents| agents.get("defaults"))
        .and_then(Value::as_object)
        .and_then(|defaults| defaults.get("model"))
        .and_then(Value::as_object)
        .and_then(|model| model.get("primary"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            config
                .get("agents")
                .and_then(Value::as_object)
                .and_then(|agents| agents.get("list"))
                .and_then(Value::as_array)
                .and_then(|list| {
                    list.iter().find_map(|entry| {
                        let entry = entry.as_object()?;
                        if entry.get("id").and_then(Value::as_str) != Some("main") {
                            return None;
                        }
                        entry
                            .get("model")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                })
        })
}

fn resolve_current_provider_config<'a>(
    provider_store: &'a crate::ProviderStore,
    current_model: Option<&str>,
) -> Option<&'a crate::ProviderConfig> {
    if let Some(default_provider_id) = provider_store.default_provider.as_deref() {
        if let Some(config) = provider_store.providers.get(default_provider_id) {
            if current_model
                .map(|model_ref| provider_matches_model_ref(config, model_ref))
                .unwrap_or(true)
            {
                return Some(config);
            }
        }
    }

    let current_model = current_model?;
    provider_store
        .providers
        .values()
        .find(|config| provider_matches_model_ref(config, current_model))
}

fn provider_matches_model_ref(config: &crate::ProviderConfig, model_ref: &str) -> bool {
    crate::get_provider_model_ref(config)
        .as_deref()
        .map(|value| value == model_ref)
        .unwrap_or(false)
        || model_ref.starts_with(&format!(
            "{}/",
            crate::get_openclaw_provider_key(&config.provider_type, &config.id)
        ))
}

fn model_ref_prefix(model_ref: &str) -> Option<&str> {
    model_ref
        .split('/')
        .next()
        .filter(|value| !value.is_empty())
}

fn tcp_port_is_reachable(port: u16) -> bool {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&address, Duration::from_millis(300)).is_ok()
}

fn is_gateway_running(state: &str) -> bool {
    matches!(state, "starting" | "running" | "reconnecting")
}

fn derive_connection_status(
    state: &str,
    gateway_reachable: bool,
    dashboard_reachable: bool,
) -> String {
    if dashboard_reachable {
        return "connected".into();
    }
    if state == "reconnecting" {
        return "reconnecting".into();
    }
    if state == "starting" {
        return "starting".into();
    }
    if gateway_reachable {
        return "reachable".into();
    }
    "disconnected".into()
}

fn operational_last_error(
    gateway_status: &crate::GatewayStatus,
    gateway_running: bool,
    gateway_reachable: bool,
    runtime_ready: bool,
    runtime_resolution: &crate::OpenClawRuntimeResolution,
) -> Option<BridgeErrorBody> {
    if let Some(error) = gateway_status.error.as_deref() {
        return Some(BridgeError::map_message(error.to_string()).body());
    }

    if !gateway_reachable {
        let detail = format!(
            "No listener accepted connections on 127.0.0.1:{}.",
            gateway_status.port
        );
        return Some(if gateway_running {
            BridgeError::openclaw_unreachable(detail).body()
        } else {
            BridgeError::gateway_not_running(detail).body()
        });
    }

    if !runtime_ready {
        return Some(
            BridgeError::openclaw_runtime_not_ready(runtime_resolution.failure_message()).body(),
        );
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_matching_provider_for_current_model() {
        let provider_store = crate::ProviderStore {
            providers: [(
                "anthropic".into(),
                crate::ProviderConfig {
                    id: "anthropic".into(),
                    name: "Anthropic".into(),
                    provider_type: "anthropic".into(),
                    auth_mode: Some("apikey".into()),
                    api_type: None,
                    base_url: None,
                    model: Some("claude-opus-4-6".into()),
                    fallback_models: None,
                    fallback_provider_ids: None,
                    enabled: true,
                    created_at: "now".into(),
                    updated_at: "now".into(),
                },
            )]
            .into_iter()
            .collect(),
            api_keys: Default::default(),
            default_provider: Some("anthropic".into()),
        };

        let selected =
            resolve_current_provider_config(&provider_store, Some("anthropic/claude-opus-4-6"))
                .expect("provider should resolve");

        assert_eq!(selected.id, "anthropic");
    }

    #[test]
    fn derives_connection_states_from_gateway_and_dashboard_reachability() {
        assert_eq!(derive_connection_status("running", true, true), "connected");
        assert_eq!(
            derive_connection_status("reconnecting", false, false),
            "reconnecting"
        );
        assert_eq!(
            derive_connection_status("starting", false, false),
            "starting"
        );
        assert_eq!(
            derive_connection_status("running", true, false),
            "reachable"
        );
        assert_eq!(
            derive_connection_status("stopped", false, false),
            "disconnected"
        );
    }
}
