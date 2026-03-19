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

    let _ = BRIDGE_RUNTIME.set(runtime_handle);
    super::gateway_adapter::start_gateway_event_adapter(app_handle, bridge_state);
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
