use axum::http::Method;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};

use super::auth::RequestContext;
use super::response::ApiError;
use super::server::BridgeAppState;

const PERMISSIONS_CONFIG_FILE_NAME: &str = "bridge-permissions.json";
const DEFAULT_READ_ONLY_PROFILE: &str = "read_only";
const DEFAULT_READ_WRITE_PROFILE: &str = "read_write";
tokio::task_local! {
    static REQUEST_PERMISSIONS: RequestPermissionSnapshot;
}

#[derive(Debug, Clone, Serialize, Default)]
pub(crate) struct RequestPermissionSnapshot {
    pub(crate) profile: String,
    pub(crate) read: bool,
    pub(crate) write: bool,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) allowed_sessions: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) caller_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PersistedBridgePermissions {
    #[serde(default = "default_default_profile")]
    default_profile: String,
    #[serde(default)]
    caller_profiles: BTreeMap<String, String>,
    #[serde(default)]
    profiles: BTreeMap<String, PermissionProfileConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct PermissionProfileConfig {
    #[serde(default = "default_true")]
    read: bool,
    #[serde(default)]
    write: bool,
    #[serde(default)]
    allowed_sessions: Vec<String>,
    #[serde(default)]
    capability_tags: Vec<String>,
}

pub(crate) async fn scope_request_permissions<F, T>(
    snapshot: RequestPermissionSnapshot,
    fut: F,
) -> T
where
    F: Future<Output = T>,
{
    REQUEST_PERMISSIONS.scope(snapshot, fut).await
}

pub(crate) fn current_request_permissions() -> Option<RequestPermissionSnapshot> {
    REQUEST_PERMISSIONS
        .try_with(|snapshot| snapshot.clone())
        .ok()
}

pub(crate) fn authorize_request(
    state: &BridgeAppState,
    context: &RequestContext,
) -> Result<RequestPermissionSnapshot, ApiError> {
    let permissions = load_permissions_state(&state.config.clawy_base_dir);
    let profile_name = resolve_profile_name(&permissions, context.caller_id.as_deref());
    let profile = resolve_profile(&permissions, &profile_name);
    let route = route_policy(&context.method, &context.path);

    if route.requires_write && !profile.write {
        return Err(ApiError::forbidden_permission(
            context,
            format!(
                "{} {} requires write permission",
                context.method, context.path
            ),
        ));
    }

    if route.requires_read && !profile.read {
        return Err(ApiError::forbidden_permission(
            context,
            format!(
                "{} {} requires read permission",
                context.method, context.path
            ),
        ));
    }

    if let Some(session_id) = route.session_id {
        if !session_is_allowed(&profile.allowed_sessions, &session_id) {
            return Err(ApiError::session_not_allowed(
                context,
                format!("session `{session_id}` is not included in the active permission scope"),
            ));
        }
    }

    let capabilities = resolve_capability_tags(&profile);
    let allowed_sessions = profile.allowed_sessions.clone();

    Ok(RequestPermissionSnapshot {
        profile: profile_name,
        read: profile.read,
        write: profile.write,
        allowed_sessions,
        capabilities,
        caller_id: context.caller_id.clone(),
    })
}

fn load_permissions_state(base_dir: &Path) -> PersistedBridgePermissions {
    let path = permissions_config_path(base_dir);
    let mut permissions: PersistedBridgePermissions = crate::read_json_or_default(&path);
    normalize_permissions_state(&mut permissions);
    permissions
}

fn normalize_permissions_state(permissions: &mut PersistedBridgePermissions) {
    if permissions.default_profile.trim().is_empty() {
        permissions.default_profile = default_default_profile();
    }

    if permissions.profiles.is_empty() {
        permissions.profiles.insert(
            DEFAULT_READ_ONLY_PROFILE.into(),
            default_read_only_profile(),
        );
        permissions.profiles.insert(
            DEFAULT_READ_WRITE_PROFILE.into(),
            default_read_write_profile(),
        );
    } else {
        permissions
            .profiles
            .entry(DEFAULT_READ_ONLY_PROFILE.into())
            .or_insert_with(default_read_only_profile);
        permissions
            .profiles
            .entry(DEFAULT_READ_WRITE_PROFILE.into())
            .or_insert_with(default_read_write_profile);
    }
}

fn resolve_profile_name(
    permissions: &PersistedBridgePermissions,
    caller_id: Option<&str>,
) -> String {
    if let Some(caller_id) = caller_id.map(str::trim).filter(|value| !value.is_empty()) {
        if let Some(profile_name) = permissions.caller_profiles.get(caller_id) {
            let profile_name = profile_name.trim();
            if !profile_name.is_empty() {
                return profile_name.to_string();
            }
        }
    }

    permissions.default_profile.clone()
}

fn resolve_profile<'a>(
    permissions: &'a PersistedBridgePermissions,
    profile_name: &str,
) -> PermissionProfileConfig {
    permissions
        .profiles
        .get(profile_name)
        .cloned()
        .or_else(|| {
            permissions
                .profiles
                .get(DEFAULT_READ_WRITE_PROFILE)
                .cloned()
        })
        .unwrap_or_else(default_read_write_profile)
}

fn resolve_capability_tags(profile: &PermissionProfileConfig) -> Vec<String> {
    if !profile.capability_tags.is_empty() {
        return profile.capability_tags.clone();
    }

    let mut capabilities = Vec::new();
    if profile.read {
        capabilities.push("bridge.read".into());
    }
    if profile.write {
        capabilities.push("bridge.write".into());
    }
    if profile.allowed_sessions.is_empty() {
        capabilities.push("bridge.session.unrestricted".into());
    } else {
        capabilities.push("bridge.session.scoped".into());
    }
    capabilities
}

fn session_is_allowed(allowed_sessions: &[String], session_id: &str) -> bool {
    allowed_sessions.is_empty()
        || allowed_sessions
            .iter()
            .any(|allowed| allowed == "*" || allowed == session_id)
}

fn route_policy(method: &str, path: &str) -> RoutePolicy {
    let session_id = extract_session_id(path);
    let is_get = method.eq_ignore_ascii_case(Method::GET.as_str());
    let is_head = method.eq_ignore_ascii_case(Method::HEAD.as_str());
    let mut requires_write = !(is_get || is_head);
    let mut requires_read = is_get || is_head;

    if path.ends_with("/send") || path.ends_with("/abort") {
        requires_write = true;
        requires_read = false;
    }

    RoutePolicy {
        requires_read,
        requires_write,
        session_id,
    }
}

fn extract_session_id(path: &str) -> Option<String> {
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    let first = segments.next()?;
    let resource = match first {
        "api" => {
            let next = segments.next()?;
            if next == "v1" {
                segments.next()?
            } else {
                next
            }
        }
        "v1" => segments.next()?,
        other => other,
    };
    if resource != "sessions" {
        return None;
    }

    let session_id = segments.next()?;
    if session_id.is_empty() {
        return None;
    }

    match segments.next() {
        None => Some(session_id.to_string()),
        Some("history") | Some("send") | Some("abort") => Some(session_id.to_string()),
        Some(_) => Some(session_id.to_string()),
    }
}

fn permissions_config_path(base_dir: &Path) -> PathBuf {
    base_dir.join("bridge").join(PERMISSIONS_CONFIG_FILE_NAME)
}

fn default_default_profile() -> String {
    DEFAULT_READ_WRITE_PROFILE.into()
}

fn default_true() -> bool {
    true
}

fn default_read_only_profile() -> PermissionProfileConfig {
    PermissionProfileConfig {
        read: true,
        write: false,
        allowed_sessions: Vec::new(),
        capability_tags: vec!["bridge.read".into(), "bridge.session.unrestricted".into()],
    }
}

fn default_read_write_profile() -> PermissionProfileConfig {
    PermissionProfileConfig {
        read: true,
        write: true,
        allowed_sessions: Vec::new(),
        capability_tags: vec![
            "bridge.read".into(),
            "bridge.write".into(),
            "bridge.session.unrestricted".into(),
        ],
    }
}

struct RoutePolicy {
    requires_read: bool,
    requires_write: bool,
    session_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;

    fn test_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "clawy-bridge-permissions-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(path.join("bridge")).expect("test dir should exist");
        path
    }

    #[test]
    fn loads_default_permissions_without_config_file() {
        let base_dir = test_dir("defaults");
        let permissions = load_permissions_state(&base_dir);

        assert!(permissions.profiles.contains_key(DEFAULT_READ_ONLY_PROFILE));
        assert!(permissions
            .profiles
            .contains_key(DEFAULT_READ_WRITE_PROFILE));
        assert_eq!(permissions.default_profile, DEFAULT_READ_WRITE_PROFILE);
    }

    #[test]
    fn resolves_caller_specific_profile_from_local_config() {
        let base_dir = test_dir("caller-profile");
        let config_path = permissions_config_path(&base_dir);
        fs::write(
            &config_path,
            serde_json::to_string(&json!({
                "defaultProfile": "read_write",
                "callerProfiles": {
                    "readonly-client": "read_only"
                },
                "profiles": {
                    "read_only": {
                        "read": true,
                        "write": false,
                        "allowedSessions": ["agent:main:main"],
                        "capabilityTags": ["bridge.read"]
                    }
                }
            }))
            .expect("config json should serialize"),
        )
        .expect("config file should write");

        let permissions = load_permissions_state(&base_dir);
        assert_eq!(
            resolve_profile_name(&permissions, Some("readonly-client")),
            "read_only"
        );
    }

    #[test]
    fn session_scope_validation_is_strict() {
        assert!(session_is_allowed(&Vec::new(), "agent:main:main"));
        assert!(session_is_allowed(&vec!["*".into()], "agent:main:main"));
        assert!(session_is_allowed(
            &vec!["agent:main:main".into()],
            "agent:main:main"
        ));
        assert!(!session_is_allowed(
            &vec!["agent:other:chat".into()],
            "agent:main:main"
        ));
    }

    #[test]
    fn extract_session_id_supports_versioned_session_paths() {
        assert_eq!(
            extract_session_id("/api/sessions/agent:main:main/history").as_deref(),
            Some("agent:main:main")
        );
        assert_eq!(
            extract_session_id("/api/v1/sessions/agent:main:main/send").as_deref(),
            Some("agent:main:main")
        );
        assert_eq!(
            extract_session_id("/sessions/agent:main:main/history").as_deref(),
            Some("agent:main:main")
        );
        assert_eq!(extract_session_id("/api/node/info"), None);
    }
}
