use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::PathBuf;
use std::sync::OnceLock;
use tauri::AppHandle;
use tokio::sync::broadcast;
use uuid::Uuid;

use super::events::BridgeEventEnvelope;
use super::server::{self, BridgeRuntimeConfig, BridgeRuntimeHandle};

const BRIDGE_CONFIG_FILE_NAME: &str = "bridge-server.json";
const DEFAULT_BRIDGE_PORT_OFFSET: u16 = 1;
const DEFAULT_FORGEAI_REGISTER_PATH: &str = "/api/nodes/register";
const DEFAULT_FORGEAI_HEARTBEAT_PATH: &str = "/api/nodes/heartbeat";
const DEFAULT_FORGEAI_HEARTBEAT_INTERVAL_SECS: u64 = 60;

static BRIDGE_RUNTIME: OnceLock<BridgeRuntimeHandle> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PersistedBridgeConfig {
    #[serde(default)]
    auth_token: String,
    #[serde(default)]
    allowed_origins: Vec<String>,
    #[serde(default)]
    node_id: String,
    #[serde(default)]
    registry_base_url: String,
    #[serde(default)]
    registry_auth_token: String,
    #[serde(default = "default_forgeai_register_path")]
    registry_register_path: String,
    #[serde(default = "default_forgeai_heartbeat_path")]
    registry_heartbeat_path: String,
    #[serde(default = "default_forgeai_heartbeat_interval_secs")]
    registry_heartbeat_interval_secs: u64,
    #[serde(default)]
    public_base_url: String,
}

pub(crate) fn start_bridge_server(
    app_handle: AppHandle,
    bridge_state: crate::BridgeState,
) -> Result<(), String> {
    if BRIDGE_RUNTIME.get().is_some() {
        return Ok(());
    }

    let runtime_handle = server::spawn_server(
        Some(app_handle.clone()),
        bridge_state.clone(),
        BridgeRuntimeConfig {
            listen_addr: resolve_listen_addr(&crate::load_settings())?,
            auth_token: resolve_auth_token()?,
            allowed_origins: resolve_allowed_origins()?,
            clawy_base_dir: crate::clawy_base_dir(),
            node_id: bridge_node_id()?,
            openclaw_config_dir: crate::openclaw_config_dir(),
        },
    )?;

    crate::append_log_line(
        "INFO",
        &format!(
            "Clawy Bridge server listening on http://{}",
            runtime_handle.local_addr
        ),
    );

    let registry_config = resolve_registry_client_config(runtime_handle.local_addr)?;

    let _ = BRIDGE_RUNTIME.set(runtime_handle.clone());
    super::gateway_adapter::start_gateway_event_adapter(app_handle.clone(), bridge_state.clone());
    if let Err(error) = super::registry::start_registry_client(
        app_handle,
        bridge_state,
        runtime_handle,
        registry_config,
    ) {
        crate::append_log_line(
            "WARN",
            &format!("Failed to start Clawy Bridge registry client: {error}"),
        );
    }
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn bridge_runtime() -> Option<&'static BridgeRuntimeHandle> {
    BRIDGE_RUNTIME.get()
}

#[allow(dead_code)]
pub(crate) fn bridge_events_sender() -> Option<broadcast::Sender<BridgeEventEnvelope>> {
    BRIDGE_RUNTIME
        .get()
        .map(|runtime| runtime.events_tx.clone())
}

pub(crate) fn bridge_node_id() -> Result<String, String> {
    Ok(load_or_create_persisted_bridge_config()?.node_id)
}

fn resolve_listen_addr(settings: &crate::Settings) -> Result<SocketAddr, String> {
    let port = match std::env::var("CLAWY_BRIDGE_PORT") {
        Ok(value) => value
            .trim()
            .parse::<u16>()
            .map_err(|error| format!("Invalid CLAWY_BRIDGE_PORT value `{value}`: {error}"))?,
        Err(_) => settings
            .gateway_port
            .saturating_add(DEFAULT_BRIDGE_PORT_OFFSET),
    };

    Ok(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)))
}

fn resolve_auth_token() -> Result<String, String> {
    match std::env::var("CLAWY_BRIDGE_TOKEN") {
        Ok(value) => {
            let value = value.trim().to_string();
            if value.is_empty() {
                return Err("CLAWY_BRIDGE_TOKEN is set but empty".into());
            }
            Ok(value)
        }
        Err(_) => Ok(load_or_create_persisted_bridge_config()?.auth_token),
    }
}

fn resolve_allowed_origins() -> Result<Vec<String>, String> {
    match std::env::var("CLAWY_BRIDGE_ALLOWED_ORIGINS") {
        Ok(value) => Ok(parse_allowed_origins(&value)),
        Err(_) => Ok(load_or_create_persisted_bridge_config()?.allowed_origins),
    }
}

fn resolve_registry_client_config(
    local_addr: SocketAddr,
) -> Result<Option<super::registry::RegistryClientConfig>, String> {
    let config = load_or_create_persisted_bridge_config()?;
    let base_url = std::env::var("CLAWY_FORGEAI_REGISTRY_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| config.registry_base_url.trim().to_string());

    if base_url.is_empty() {
        return Ok(None);
    }

    let auth_token = std::env::var("CLAWY_FORGEAI_REGISTRY_TOKEN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            (!config.registry_auth_token.trim().is_empty())
                .then(|| config.registry_auth_token.trim().to_string())
        });

    let register_path = std::env::var("CLAWY_FORGEAI_REGISTER_PATH")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| config.registry_register_path.clone());
    let heartbeat_path = std::env::var("CLAWY_FORGEAI_HEARTBEAT_PATH")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| config.registry_heartbeat_path.clone());
    let heartbeat_interval_secs = match std::env::var("CLAWY_FORGEAI_HEARTBEAT_INTERVAL_SECS") {
        Ok(value) => value.trim().parse::<u64>().map_err(|error| {
            format!("Invalid CLAWY_FORGEAI_HEARTBEAT_INTERVAL_SECS value `{value}`: {error}")
        })?,
        Err(_) => config.registry_heartbeat_interval_secs,
    };
    let public_base_url = std::env::var("CLAWY_BRIDGE_PUBLIC_BASE_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            (!config.public_base_url.trim().is_empty()).then(|| config.public_base_url.clone())
        })
        .unwrap_or_else(|| format!("http://{local_addr}"));

    super::registry::registry_client_config(
        base_url,
        register_path,
        heartbeat_path,
        heartbeat_interval_secs,
        auth_token,
        public_base_url,
    )
    .map(Some)
}

fn load_or_create_persisted_bridge_config() -> Result<PersistedBridgeConfig, String> {
    let path = bridge_config_path();
    let mut config: PersistedBridgeConfig = crate::read_json_or_default(&path);
    let mut changed = false;

    if config.auth_token.trim().is_empty() {
        config.auth_token = format!("bridge_{}", Uuid::new_v4().simple());
        changed = true;
    }

    if config.node_id.trim().is_empty() {
        config.node_id = format!("node_{}", Uuid::new_v4());
        changed = true;
    }

    let normalized_origins = normalize_allowed_origins(config.allowed_origins.clone());
    if normalized_origins != config.allowed_origins {
        config.allowed_origins = normalized_origins;
        changed = true;
    }

    let register_path = normalize_registry_path(
        &config.registry_register_path,
        DEFAULT_FORGEAI_REGISTER_PATH,
    );
    if register_path != config.registry_register_path {
        config.registry_register_path = register_path;
        changed = true;
    }

    let heartbeat_path = normalize_registry_path(
        &config.registry_heartbeat_path,
        DEFAULT_FORGEAI_HEARTBEAT_PATH,
    );
    if heartbeat_path != config.registry_heartbeat_path {
        config.registry_heartbeat_path = heartbeat_path;
        changed = true;
    }

    let heartbeat_interval_secs =
        normalize_registry_heartbeat_interval(config.registry_heartbeat_interval_secs);
    if heartbeat_interval_secs != config.registry_heartbeat_interval_secs {
        config.registry_heartbeat_interval_secs = heartbeat_interval_secs;
        changed = true;
    }

    if changed {
        crate::write_json(&path, &config)?;
    }

    Ok(config)
}

fn bridge_config_path() -> PathBuf {
    crate::clawy_base_dir()
        .join("bridge")
        .join(BRIDGE_CONFIG_FILE_NAME)
}

fn parse_allowed_origins(value: &str) -> Vec<String> {
    let parts = value
        .split([',', ';', '\n'])
        .map(str::to_string)
        .collect::<Vec<_>>();
    normalize_allowed_origins(parts)
}

fn normalize_allowed_origins(origins: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();

    for origin in origins {
        let origin = origin.trim();
        if origin.is_empty() || normalized.iter().any(|existing| existing == origin) {
            continue;
        }
        normalized.push(origin.to_string());
    }

    normalized
}

fn normalize_registry_path(path: &str, fallback: &str) -> String {
    let path = path.trim();
    if path.is_empty() {
        return fallback.to_string();
    }

    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

fn normalize_registry_heartbeat_interval(value: u64) -> u64 {
    value.clamp(15, 300)
}

fn default_forgeai_register_path() -> String {
    DEFAULT_FORGEAI_REGISTER_PATH.into()
}

fn default_forgeai_heartbeat_path() -> String {
    DEFAULT_FORGEAI_HEARTBEAT_PATH.into()
}

fn default_forgeai_heartbeat_interval_secs() -> u64 {
    DEFAULT_FORGEAI_HEARTBEAT_INTERVAL_SECS
}
