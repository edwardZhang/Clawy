use serde::Serialize;
use serde_json::Value;
use sha2::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::errors::{BridgeError, BridgeErrorBody, BridgeResult};
use super::runtime::{self, ModelSelection};
use super::server::BridgeAppState;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RuntimeCapabilitiesResponse {
    pub(crate) agent_identity: AgentIdentitySummary,
    pub(crate) loaded_skills: Vec<LoadedSkillSummary>,
    pub(crate) enabled_tools: Vec<ToolCapabilitySummary>,
    pub(crate) workspace_info: WorkspaceInfoSummary,
    pub(crate) channel_bindings: Vec<ChannelBindingSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) runtime_source: Option<String>,
    pub(crate) model_info: ModelSelection,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) capability_errors: Vec<BridgeErrorBody>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AgentIdentitySummary {
    pub(crate) client_name: String,
    pub(crate) platform: String,
    pub(crate) default_agent_id: String,
    pub(crate) available_agent_ids: Vec<String>,
    pub(crate) device_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct LoadedSkillSummary {
    pub(crate) slug: String,
    pub(crate) installed: bool,
    pub(crate) configured: bool,
    pub(crate) enabled: bool,
    pub(crate) selected: bool,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ToolCapabilitySummary {
    pub(crate) id: String,
    pub(crate) enabled: bool,
    pub(crate) source: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct WorkspaceInfoSummary {
    pub(crate) cwd: String,
    pub(crate) clawy_base_dir: String,
    pub(crate) config_dir: String,
    pub(crate) openclaw_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) runtime_source: Option<String>,
    pub(crate) full_mode_runtime: bool,
    pub(crate) managed_runtime_available: bool,
    pub(crate) workspace_runtime_available: bool,
    pub(crate) bundled_runtime_available: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ChannelBindingSummary {
    pub(crate) channel_type: String,
    pub(crate) configured: bool,
    pub(crate) enabled: bool,
    pub(crate) config_source: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) binding_fields: Vec<String>,
    pub(crate) binding_count: usize,
    pub(crate) plugin: ChannelPluginOverview,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ChannelPluginOverview {
    pub(crate) required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) plugin_id: Option<String>,
    pub(crate) installed: bool,
    pub(crate) enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) origin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) message: Option<String>,
}

pub(crate) fn build_runtime_capabilities(
    state: &BridgeAppState,
) -> BridgeResult<RuntimeCapabilitiesResponse> {
    let config = runtime::read_openclaw_json_from_dir(&state.config.openclaw_config_dir)?;
    let mut capability_errors = Vec::new();
    let model_info = match runtime::resolve_model_selection(
        &state.config.clawy_base_dir,
        &state.config.openclaw_config_dir,
    ) {
        Ok(model_info) => model_info,
        Err(error) => {
            capability_errors.push(error.body());
            ModelSelection::default()
        }
    };

    let runtime_resolution = runtime::resolve_openclaw_runtime_for_bridge(state);
    let runtime_source = runtime_resolution
        .source
        .map(crate::OpenClawRuntimeSource::as_str)
        .map(str::to_string);
    let openclaw_dir = runtime_resolution
        .dir
        .clone()
        .unwrap_or_else(crate::workspace_openclaw_dir);
    let settings = load_settings_from_base_dir(&state.config.clawy_base_dir);

    Ok(RuntimeCapabilitiesResponse {
        agent_identity: build_agent_identity(
            &state.config.clawy_base_dir,
            &state.config.openclaw_config_dir,
        )?,
        loaded_skills: build_loaded_skills(&settings, &config, &state.config.openclaw_config_dir),
        enabled_tools: build_enabled_tools(&config),
        workspace_info: WorkspaceInfoSummary {
            cwd: crate::current_workspace_dir().to_string_lossy().to_string(),
            clawy_base_dir: state.config.clawy_base_dir.to_string_lossy().to_string(),
            config_dir: state
                .config
                .openclaw_config_dir
                .to_string_lossy()
                .to_string(),
            openclaw_dir: openclaw_dir.to_string_lossy().to_string(),
            runtime_source: runtime_source.clone(),
            full_mode_runtime: crate::full_mode_runtime_enabled(),
            managed_runtime_available: crate::managed_openclaw_dir_from_base(
                &state.config.clawy_base_dir,
            )
            .is_some(),
            workspace_runtime_available: crate::workspace_openclaw_dir().exists(),
            bundled_runtime_available: crate::bundled_openclaw_dir().exists(),
        },
        channel_bindings: build_channel_bindings(
            &config,
            &state.config.openclaw_config_dir,
            &mut capability_errors,
        ),
        runtime_source,
        model_info,
        capability_errors,
    })
}

fn build_agent_identity(
    clawy_base_dir: &Path,
    openclaw_config_dir: &Path,
) -> BridgeResult<AgentIdentitySummary> {
    let identity = load_or_create_device_identity_at(clawy_base_dir)?;
    let available_agent_ids = discover_agent_ids_in_config_dir(openclaw_config_dir);
    let default_agent_id = if available_agent_ids
        .iter()
        .any(|agent_id| agent_id == "main")
    {
        "main".to_string()
    } else {
        available_agent_ids
            .first()
            .cloned()
            .unwrap_or_else(|| "main".into())
    };

    Ok(AgentIdentitySummary {
        client_name: "Clawy".into(),
        platform: crate::platform_name().into(),
        default_agent_id,
        available_agent_ids,
        device_id: identity.device_id,
    })
}

fn build_loaded_skills(
    settings: &crate::Settings,
    config: &Value,
    openclaw_config_dir: &Path,
) -> Vec<LoadedSkillSummary> {
    let mut skills = BTreeMap::<String, LoadedSkillSummary>::new();

    if let Ok(entries) = std::fs::read_dir(openclaw_config_dir.join("skills")) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let slug = entry.file_name().to_string_lossy().to_string();
            let summary = skills
                .entry(slug.clone())
                .or_insert_with(|| LoadedSkillSummary::new(slug.clone()));
            summary.installed = true;
            summary.push_source("skills_dir");
        }
    }

    if let Some(entries) = config
        .get("skills")
        .and_then(Value::as_object)
        .and_then(|skills| skills.get("entries"))
        .and_then(Value::as_object)
    {
        for slug in entries.keys() {
            let summary = skills
                .entry(slug.clone())
                .or_insert_with(|| LoadedSkillSummary::new(slug.clone()));
            summary.configured = true;
            summary.push_source("openclaw_json");
        }
    }

    for summary in skills.values_mut() {
        summary.selected = settings
            .enabled_skills
            .iter()
            .any(|value| value == &summary.slug);
        summary.enabled = !settings
            .disabled_skills
            .iter()
            .any(|value| value == &summary.slug);

        if summary.selected {
            summary.push_source("settings_enabled");
        }
        if !summary.enabled {
            summary.push_source("settings_disabled");
        }
    }

    skills.into_values().collect()
}

fn build_enabled_tools(config: &Value) -> Vec<ToolCapabilitySummary> {
    vec![ToolCapabilitySummary {
        id: "browser".into(),
        enabled: config
            .get("browser")
            .and_then(Value::as_object)
            .and_then(|browser| browser.get("enabled"))
            .and_then(Value::as_bool)
            .unwrap_or(true),
        source: "openclaw_json".into(),
    }]
}

fn build_channel_bindings(
    config: &Value,
    openclaw_config_dir: &Path,
    capability_errors: &mut Vec<BridgeErrorBody>,
) -> Vec<ChannelBindingSummary> {
    let channels = list_configured_channels(config, openclaw_config_dir);
    let mut bindings = Vec::new();

    for channel_type in channels {
        let channel_config = read_channel_config_from_value(config, &channel_type);
        let enabled = channel_config
            .as_ref()
            .and_then(|value| value.get("enabled"))
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let binding_fields = channel_config
            .as_ref()
            .map(summarize_binding_fields)
            .unwrap_or_default();

        bindings.push(ChannelBindingSummary {
            channel_type: channel_type.clone(),
            configured: true,
            enabled,
            config_source: channel_config_source(config, openclaw_config_dir, &channel_type).into(),
            binding_count: binding_fields.len(),
            binding_fields,
            plugin: plugin_overview(&channel_type, capability_errors),
        });
    }

    bindings
}

fn plugin_overview(
    channel_type: &str,
    capability_errors: &mut Vec<BridgeErrorBody>,
) -> ChannelPluginOverview {
    let Some(policy) = crate::channel_plugin_policy(channel_type) else {
        return ChannelPluginOverview {
            required: false,
            plugin_id: None,
            installed: false,
            enabled: false,
            status: None,
            origin: None,
            message: None,
        };
    };

    let status_result = match policy.install_mode {
        crate::ChannelPluginInstallMode::OpenClawCliManaged => {
            crate::get_openclaw_cli_managed_plugin_status(channel_type, policy.plugin_id)
        }
        crate::ChannelPluginInstallMode::LegacyClawyManaged => {
            Ok(crate::get_legacy_dingtalk_plugin_status())
        }
    };

    match status_result {
        Ok(status) => ChannelPluginOverview {
            required: status.required,
            plugin_id: status.plugin_id,
            installed: status.installed,
            enabled: status.enabled,
            status: status.status,
            origin: status.origin,
            message: status.message,
        },
        Err(error) => {
            capability_errors.push(BridgeError::map_message(error.clone()).body());
            ChannelPluginOverview {
                required: true,
                plugin_id: Some(policy.plugin_id.to_string()),
                installed: false,
                enabled: false,
                status: Some("error".into()),
                origin: None,
                message: Some(error),
            }
        }
    }
}

fn load_settings_from_base_dir(clawy_base_dir: &Path) -> crate::Settings {
    crate::read_json_or_default(&clawy_base_dir.join("settings.json"))
}

fn discover_agent_ids_in_config_dir(openclaw_config_dir: &Path) -> Vec<String> {
    let agents_dir = openclaw_config_dir.join("agents");
    if !agents_dir.exists() {
        return vec!["main".into()];
    }

    let mut result = BTreeSet::new();
    if let Ok(entries) = std::fs::read_dir(&agents_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join("agent").exists() {
                result.insert(entry.file_name().to_string_lossy().to_string());
            }
        }
    }

    if result.is_empty() {
        result.insert("main".into());
    }

    result.into_iter().collect()
}

fn load_or_create_device_identity_at(
    clawy_base_dir: &Path,
) -> BridgeResult<crate::StoredDeviceIdentity> {
    let path = clawy_base_dir.join("gateway-device-identity.json");

    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(identity) = serde_json::from_str::<crate::StoredDeviceIdentity>(&content) {
                let public_key = crate::decode_base64url::<32>(&identity.public_key);
                let secret_key = crate::decode_base64url::<32>(&identity.secret_key);
                if public_key.is_ok() && secret_key.is_ok() {
                    let derived_id =
                        crate::hex_encode(sha2::Sha256::digest(public_key.unwrap()).as_slice());
                    if derived_id == identity.device_id {
                        return Ok(identity);
                    }
                }
            }
        }
    }

    let identity = crate::generate_device_identity();
    crate::write_json(&path, &identity).map_err(|error| {
        BridgeError::internal(format!(
            "Failed to persist device identity at {}: {error}",
            path.to_string_lossy()
        ))
    })?;
    Ok(identity)
}

fn list_configured_channels(config: &Value, openclaw_config_dir: &Path) -> Vec<String> {
    let mut channels = BTreeSet::new();

    if let Some(configured) = config.get("channels").and_then(Value::as_object) {
        for (channel_type, value) in configured {
            let enabled = value
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            if enabled {
                channels.insert(channel_type.clone());
            }
        }
    }

    if let Some(entries) = config
        .get("plugins")
        .and_then(Value::as_object)
        .and_then(|plugins| plugins.get("entries"))
        .and_then(Value::as_object)
    {
        for (channel_type, value) in entries {
            let enabled = value
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            if enabled {
                channels.insert(channel_type.clone());
            }
        }
    }

    let whatsapp_dir = openclaw_config_dir.join("credentials").join("whatsapp");
    if whatsapp_dir.exists() {
        let has_session = std::fs::read_dir(&whatsapp_dir)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .any(|entry| entry.path().is_dir())
            })
            .unwrap_or(false);
        if has_session {
            channels.insert("whatsapp".into());
        }
    }

    channels.into_iter().collect()
}

fn read_channel_config_from_value(config: &Value, channel_type: &str) -> Option<Value> {
    config
        .get("channels")
        .and_then(Value::as_object)
        .and_then(|channels| channels.get(channel_type))
        .cloned()
        .or_else(|| {
            config
                .get("plugins")
                .and_then(Value::as_object)
                .and_then(|plugins| plugins.get("entries"))
                .and_then(Value::as_object)
                .and_then(|entries| entries.get(channel_type))
                .cloned()
        })
}

fn channel_config_source(
    config: &Value,
    openclaw_config_dir: &Path,
    channel_type: &str,
) -> &'static str {
    if config
        .get("channels")
        .and_then(Value::as_object)
        .and_then(|channels| channels.get(channel_type))
        .is_some()
    {
        return "channels";
    }

    if config
        .get("plugins")
        .and_then(Value::as_object)
        .and_then(|plugins| plugins.get("entries"))
        .and_then(Value::as_object)
        .and_then(|entries| entries.get(channel_type))
        .is_some()
    {
        return "plugins";
    }

    if channel_type == "whatsapp"
        && openclaw_config_dir
            .join("credentials")
            .join("whatsapp")
            .exists()
    {
        return "whatsapp_credentials";
    }

    "unknown"
}

fn summarize_binding_fields(config: &Value) -> Vec<String> {
    let Some(object) = config.as_object() else {
        return Vec::new();
    };

    object
        .keys()
        .filter(|key| !is_sensitive_binding_key(key))
        .cloned()
        .collect()
}

fn is_sensitive_binding_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("token")
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("cookie")
        || lower.contains("refresh")
        || lower.contains("access")
        || lower == "apikey"
        || lower.ends_with("key")
        || lower.contains("webhook")
}

impl LoadedSkillSummary {
    fn new(slug: String) -> Self {
        Self {
            slug,
            installed: false,
            configured: false,
            enabled: true,
            selected: false,
            sources: Vec::new(),
        }
    }

    fn push_source(&mut self, source: &str) {
        if !self.sources.iter().any(|existing| existing == source) {
            self.sources.push(source.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn filters_sensitive_channel_fields_from_binding_summary() {
        let fields = summarize_binding_fields(&json!({
            "botToken": "secret",
            "guildId": "123",
            "channelId": "456",
            "webhookUrl": "https://example.invalid",
            "enabled": true
        }));

        assert_eq!(fields, vec!["channelId", "enabled", "guildId"]);
    }

    #[test]
    fn merges_skill_sources_and_settings_flags() {
        let settings = crate::Settings {
            enabled_skills: vec!["beta".into()],
            disabled_skills: vec!["alpha".into()],
            ..crate::Settings::default()
        };
        let config = json!({
            "skills": {
                "entries": {
                    "alpha": {},
                    "beta": {}
                }
            }
        });

        let temp_dir = std::env::temp_dir().join(format!(
            "clawy-bridge-skills-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(temp_dir.join("skills").join("alpha"))
            .expect("create alpha skill dir");
        let loaded = build_loaded_skills(&settings, &config, &temp_dir);
        std::fs::remove_dir_all(&temp_dir).ok();

        let alpha = loaded
            .iter()
            .find(|skill| skill.slug == "alpha")
            .expect("alpha skill");
        let beta = loaded
            .iter()
            .find(|skill| skill.slug == "beta")
            .expect("beta skill");

        assert!(alpha.installed);
        assert!(alpha.configured);
        assert!(!alpha.enabled);
        assert!(beta.selected);
    }
}
