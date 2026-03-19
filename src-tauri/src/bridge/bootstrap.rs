use ipnet::IpNet;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
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
    lan_enabled: bool,
    #[serde(default)]
    trusted_remote_cidrs: Vec<String>,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BridgeTokenInfo {
    pub(crate) node_id: String,
    pub(crate) token: String,
    pub(crate) token_source: String,
    pub(crate) managed_by_env: bool,
    pub(crate) config_path: String,
    pub(crate) base_url: String,
    pub(crate) api_base_url: String,
    pub(crate) restart_required: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BridgeNetworkConfigInfo {
    pub(crate) lan_enabled: bool,
    pub(crate) listen_host: String,
    pub(crate) trusted_remote_cidrs: Vec<String>,
    pub(crate) allowed_origins: Vec<String>,
    pub(crate) public_base_url: String,
    pub(crate) base_url: String,
    pub(crate) api_base_url: String,
    pub(crate) restart_required: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BridgeNetworkConfigUpdate {
    #[serde(default)]
    pub(crate) lan_enabled: bool,
    #[serde(default)]
    pub(crate) trusted_remote_cidrs: Vec<String>,
    #[serde(default)]
    pub(crate) allowed_origins: Vec<String>,
    #[serde(default)]
    pub(crate) public_base_url: String,
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
            lan_enabled: resolve_lan_enabled()?,
            trusted_remote_cidrs: resolve_trusted_remote_cidrs()?,
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

pub(crate) fn bridge_token_info() -> Result<BridgeTokenInfo, String> {
    let config = load_or_create_persisted_bridge_config()?;
    build_bridge_token_info(&config, false)
}

pub(crate) fn bridge_network_config() -> Result<BridgeNetworkConfigInfo, String> {
    let config = load_or_create_persisted_bridge_config()?;
    build_bridge_network_config_info(&config, false)
}

pub(crate) fn regenerate_bridge_auth_token() -> Result<BridgeTokenInfo, String> {
    if bridge_token_override()?.is_some() {
        return Err(
            "Clawy Bridge token is managed by CLAWY_BRIDGE_TOKEN and cannot be regenerated from Settings."
                .into(),
        );
    }

    let mut config = load_or_create_persisted_bridge_config()?;
    config.auth_token = format!("bridge_{}", Uuid::new_v4().simple());
    crate::write_json(&bridge_config_path(), &config)?;
    crate::append_log_line("INFO", "Clawy Bridge auth token regenerated from Settings");
    build_bridge_token_info(&config, BRIDGE_RUNTIME.get().is_some())
}

pub(crate) fn update_bridge_network_config(
    update: BridgeNetworkConfigUpdate,
) -> Result<BridgeNetworkConfigInfo, String> {
    let mut config = load_or_create_persisted_bridge_config()?;
    config.lan_enabled = update.lan_enabled;
    config.trusted_remote_cidrs = normalize_trusted_remote_cidrs(update.trusted_remote_cidrs)?;
    config.allowed_origins = normalize_allowed_origins(update.allowed_origins);
    config.public_base_url = normalize_public_base_url(update.public_base_url)?;
    crate::write_json(&bridge_config_path(), &config)?;
    crate::append_log_line("INFO", "Clawy Bridge network config updated from Settings");
    build_bridge_network_config_info(&config, BRIDGE_RUNTIME.get().is_some())
}

fn resolve_listen_addr(settings: &crate::Settings) -> Result<SocketAddr, String> {
    let host = match std::env::var("CLAWY_BRIDGE_HOST") {
        Ok(value) => parse_listen_host(&value)?,
        Err(_) => {
            if resolve_lan_enabled()? {
                IpAddr::V4(Ipv4Addr::UNSPECIFIED)
            } else {
                IpAddr::V4(Ipv4Addr::LOCALHOST)
            }
        }
    };
    let port = match std::env::var("CLAWY_BRIDGE_PORT") {
        Ok(value) => value
            .trim()
            .parse::<u16>()
            .map_err(|error| format!("Invalid CLAWY_BRIDGE_PORT value `{value}`: {error}"))?,
        Err(_) => settings
            .gateway_port
            .saturating_add(DEFAULT_BRIDGE_PORT_OFFSET),
    };

    match host {
        IpAddr::V4(host) => Ok(SocketAddr::V4(SocketAddrV4::new(host, port))),
        IpAddr::V6(host) => Ok(SocketAddr::new(IpAddr::V6(host), port)),
    }
}

fn resolve_auth_token() -> Result<String, String> {
    match bridge_token_override()? {
        Some(value) => Ok(value),
        None => Ok(load_or_create_persisted_bridge_config()?.auth_token),
    }
}

fn resolve_lan_enabled() -> Result<bool, String> {
    match std::env::var("CLAWY_BRIDGE_LAN_ENABLED") {
        Ok(value) => parse_bool_env("CLAWY_BRIDGE_LAN_ENABLED", &value),
        Err(_) => Ok(load_or_create_persisted_bridge_config()?.lan_enabled),
    }
}

fn resolve_trusted_remote_cidrs() -> Result<Vec<IpNet>, String> {
    let values = match std::env::var("CLAWY_BRIDGE_TRUSTED_CIDRS") {
        Ok(value) => parse_trusted_remote_cidr_list(&value)?,
        Err(_) => load_or_create_persisted_bridge_config()?.trusted_remote_cidrs,
    };
    values
        .into_iter()
        .map(|value| parse_ip_net(&value))
        .collect::<Result<Vec<_>, _>>()
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
        .unwrap_or_else(|| default_public_base_url(local_addr));

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

    let normalized_trusted_cidrs =
        normalize_trusted_remote_cidrs(config.trusted_remote_cidrs.clone())?;
    if normalized_trusted_cidrs != config.trusted_remote_cidrs {
        config.trusted_remote_cidrs = normalized_trusted_cidrs;
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

    let normalized_public_base_url = normalize_public_base_url(config.public_base_url.clone())?;
    if normalized_public_base_url != config.public_base_url {
        config.public_base_url = normalized_public_base_url;
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

fn build_bridge_token_info(
    config: &PersistedBridgeConfig,
    restart_required: bool,
) -> Result<BridgeTokenInfo, String> {
    let managed_by_env;
    let token_source;
    let token = match bridge_token_override()? {
        Some(value) => {
            managed_by_env = true;
            token_source = "env";
            value
        }
        None => {
            managed_by_env = false;
            token_source = "local";
            config.auth_token.clone()
        }
    };

    let listen_addr = BRIDGE_RUNTIME
        .get()
        .map(|runtime| runtime.local_addr)
        .unwrap_or(resolve_listen_addr(&crate::load_settings())?);
    let base_url = resolve_effective_public_base_url(config, listen_addr)?;

    Ok(BridgeTokenInfo {
        node_id: config.node_id.clone(),
        token,
        token_source: token_source.to_string(),
        managed_by_env,
        config_path: bridge_config_path().to_string_lossy().to_string(),
        api_base_url: format!("{base_url}/api/v1"),
        base_url,
        restart_required,
    })
}

fn build_bridge_network_config_info(
    config: &PersistedBridgeConfig,
    restart_required: bool,
) -> Result<BridgeNetworkConfigInfo, String> {
    let listen_addr = BRIDGE_RUNTIME
        .get()
        .map(|runtime| runtime.local_addr)
        .unwrap_or(resolve_listen_addr(&crate::load_settings())?);
    let base_url = resolve_effective_public_base_url(config, listen_addr)?;
    let lan_enabled = resolve_lan_enabled()?;
    let trusted_remote_cidrs = resolve_trusted_remote_cidrs()?
        .into_iter()
        .map(|cidr| cidr.to_string())
        .collect::<Vec<_>>();
    let allowed_origins = resolve_allowed_origins()?;

    Ok(BridgeNetworkConfigInfo {
        lan_enabled,
        listen_host: listen_addr.ip().to_string(),
        trusted_remote_cidrs,
        allowed_origins,
        public_base_url: if config.public_base_url.trim().is_empty() {
            String::new()
        } else {
            resolve_effective_public_base_url(config, listen_addr)?
        },
        api_base_url: format!("{base_url}/api/v1"),
        base_url,
        restart_required,
    })
}

fn bridge_config_path() -> PathBuf {
    crate::clawy_base_dir()
        .join("bridge")
        .join(BRIDGE_CONFIG_FILE_NAME)
}

fn resolve_effective_public_base_url(
    config: &PersistedBridgeConfig,
    listen_addr: SocketAddr,
) -> Result<String, String> {
    let configured = std::env::var("CLAWY_BRIDGE_PUBLIC_BASE_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            (!config.public_base_url.trim().is_empty()).then(|| config.public_base_url.clone())
        });

    match configured {
        Some(value) => normalize_public_base_url(value),
        None => Ok(default_public_base_url(listen_addr)),
    }
}

fn default_public_base_url(listen_addr: SocketAddr) -> String {
    if listen_addr.ip().is_unspecified() {
        if let Some(local_ip) = detect_preferred_local_ip() {
            return format!("http://{}:{}", local_ip, listen_addr.port());
        }
        return format!("http://127.0.0.1:{}", listen_addr.port());
    }

    format!("http://{listen_addr}")
}

fn bridge_token_override() -> Result<Option<String>, String> {
    match std::env::var("CLAWY_BRIDGE_TOKEN") {
        Ok(value) => {
            let value = value.trim().to_string();
            if value.is_empty() {
                return Err("CLAWY_BRIDGE_TOKEN is set but empty".into());
            }
            Ok(Some(value))
        }
        Err(_) => Ok(None),
    }
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

fn parse_trusted_remote_cidr_list(value: &str) -> Result<Vec<String>, String> {
    normalize_trusted_remote_cidrs(
        value
            .split([',', ';', '\n'])
            .map(str::to_string)
            .collect::<Vec<_>>(),
    )
}

fn normalize_trusted_remote_cidrs(cidrs: Vec<String>) -> Result<Vec<String>, String> {
    let mut normalized = Vec::new();

    for cidr in cidrs {
        let cidr = cidr.trim();
        if cidr.is_empty() {
            continue;
        }
        let parsed = parse_ip_net(cidr)?;
        let canonical = parsed.to_string();
        if normalized.iter().any(|existing| existing == &canonical) {
            continue;
        }
        normalized.push(canonical);
    }

    Ok(normalized)
}

fn parse_ip_net(value: &str) -> Result<IpNet, String> {
    if let Ok(network) = value.parse::<IpNet>() {
        return Ok(network);
    }

    if let Ok(address) = value.parse::<IpAddr>() {
        return IpNet::new(
            address,
            match address {
                IpAddr::V4(_) => 32,
                IpAddr::V6(_) => 128,
            },
        )
        .map_err(|error| format!("Invalid trusted remote CIDR `{value}`: {error}"));
    }

    Err(format!("Invalid trusted remote CIDR or IP `{value}`"))
}

fn parse_listen_host(value: &str) -> Result<IpAddr, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("CLAWY_BRIDGE_HOST is set but empty".into());
    }
    value
        .parse::<IpAddr>()
        .map_err(|error| format!("Invalid CLAWY_BRIDGE_HOST value `{value}`: {error}"))
}

fn parse_bool_env(name: &str, value: &str) -> Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(format!(
            "Invalid {name} value `{value}`: expected true/false"
        )),
    }
}

fn normalize_public_base_url(value: String) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(String::new());
    }

    let mut url =
        Url::parse(value).map_err(|error| format!("Invalid public base URL `{value}`: {error}"))?;

    if url.scheme() != "http" && url.scheme() != "https" {
        return Err("Public base URL must use http or https".into());
    }

    url.set_path("");
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string().trim_end_matches('/').to_string())
}

fn detect_preferred_local_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(8, 8, 8, 8), 80)).ok()?;
    let local_ip = socket.local_addr().ok()?.ip();
    (!local_ip.is_unspecified()).then_some(local_ip)
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
