use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use rand_core::OsRng;
use reqwest::{NoProxy, Proxy};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256, Sha512};
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::menu::{MenuBuilder, MenuEvent, SubmenuBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};
use time::OffsetDateTime;
use uuid::Uuid;

const DEFAULT_GATEWAY_PORT: u16 = 18_789;
const DEFAULT_GATEWAY_SCOPES: [&str; 1] = ["operator.admin"];
const VISION_MIME_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/bmp", "image/webp"];
const SUPPORTED_NODE_VERSION_RANGE: &str = ">=24.8.0, <25.0.0";
const RECOMMENDED_MANAGED_NODE_VERSION: &str = "24.8.0";
const NODE_SMOKE_TEST_SCRIPT: &str = "process.stdout.write('clawy-node-smoke')";
const NODE_SMOKE_TEST_OUTPUT: &str = "clawy-node-smoke";
const FULL_MODE_RUNTIME_FLAG: &str = "CLAWY_FULL_MODE_RUNTIME";
#[allow(dead_code)]
const MANAGED_RUNTIME_SCHEMA_VERSION: u32 = 1;
#[allow(dead_code)]
const MANAGED_RUNTIME_DIR_NAME: &str = "runtime";
#[allow(dead_code)]
const MANAGED_RUNTIME_DOWNLOADS_DIR_NAME: &str = "downloads";
#[allow(dead_code)]
const MANAGED_RUNTIME_VERSIONS_DIR_NAME: &str = "versions";
#[allow(dead_code)]
const MANAGED_RUNTIME_STAGING_DIR_NAME: &str = "staging";
#[allow(dead_code)]
const MANAGED_RUNTIME_STATE_FILE_NAME: &str = "runtime-state.json";
#[allow(dead_code)]
const MANAGED_RUNTIME_MANIFEST_FILE_NAME: &str = "manifest.json";
#[allow(dead_code)]
const MANAGED_RUNTIME_CURRENT_POINTER_FILE_NAME: &str = "current";
const OPENCLAW_PACKAGE_NAME: &str = "openclaw";
const OPENCLAW_NPM_REGISTRY_BASE_URL: &str = "https://registry.npmjs.org";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GatewayStatus {
    state: String,
    port: u16,
    pid: Option<u32>,
    uptime: Option<u64>,
    error: Option<String>,
    connected_at: Option<u64>,
    version: Option<String>,
    reconnect_attempts: Option<u32>,
}

impl Default for GatewayStatus {
    fn default() -> Self {
        Self {
            state: "stopped".into(),
            port: DEFAULT_GATEWAY_PORT,
            pid: None,
            uptime: None,
            error: None,
            connected_at: None,
            version: None,
            reconnect_attempts: None,
        }
    }
}

#[derive(Default)]
struct GatewayRuntime {
    child: Option<Child>,
    desired_running: bool,
    started_at_ms: Option<u64>,
}

#[derive(Clone, Default)]
struct BridgeState {
    gateway_status: Arc<Mutex<GatewayStatus>>,
    gateway_runtime: Arc<Mutex<GatewayRuntime>>,
    updater_status: Arc<Mutex<UpdateStatusPayload>>,
    updater_runtime: Arc<Mutex<UpdaterRuntime>>,
    oauth_runtime: Arc<Mutex<OAuthRuntime>>,
    whatsapp_runtime: Arc<Mutex<WhatsAppRuntime>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateInfoPayload {
    version: String,
    release_date: Option<String>,
    release_notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    download_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgressPayload {
    total: u64,
    delta: u64,
    transferred: u64,
    percent: f64,
    bytes_per_second: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeInstallProgressPayload {
    runtime: String,
    phase: String,
    status: String,
    percent: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    progress: Option<DownloadProgressPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateStatusPayload {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    info: Option<UpdateInfoPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    progress: Option<DownloadProgressPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl Default for UpdateStatusPayload {
    fn default() -> Self {
        Self {
            status: "idle".into(),
            info: None,
            progress: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone)]
struct DownloadTarget {
    version: String,
    channel: String,
    release_date: Option<String>,
    release_notes: Option<String>,
    download_url: String,
    file_name: String,
}

#[derive(Default)]
struct UpdaterRuntime {
    auto_install_generation: u64,
    download_target: Option<DownloadTarget>,
    downloaded_file: Option<PathBuf>,
    is_downloading: bool,
}

#[derive(Default)]
struct OAuthRuntime {
    child: Option<Child>,
    provider: Option<String>,
}

#[derive(Default)]
struct WhatsAppRuntime {
    child: Option<Child>,
    account_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ReleaseManifest {
    version: String,
    channel: Option<String>,
    #[serde(rename = "releaseDate")]
    release_date: Option<String>,
    downloads: ReleaseDownloads,
    changelog: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ReleaseDownloads {
    mac: Option<ReleasePlatformDownloads>,
    win: Option<ReleasePlatformDownloads>,
    linux: Option<ReleasePlatformDownloads>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ReleasePlatformDownloads {
    x64: Option<String>,
    arm64: Option<String>,
    deb_amd64: Option<String>,
    deb_arm64: Option<String>,
    appimage_x64: Option<String>,
    appimage_arm64: Option<String>,
    rpm_x64: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum ManagedRuntimeKind {
    Node,
    OpenClaw,
}

#[allow(dead_code)]
impl ManagedRuntimeKind {
    fn dir_name(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::OpenClaw => "openclaw",
        }
    }

    fn event_key(self) -> &'static str {
        match self {
            Self::Node => "nodejs",
            Self::OpenClaw => "openclaw",
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ManagedRuntimeVersionPointer {
    version: String,
    manifest_path: PathBuf,
}

#[allow(dead_code)]
impl ManagedRuntimeVersionPointer {
    fn new(kind: ManagedRuntimeKind, version: impl Into<String>) -> Self {
        let version = version.into();

        Self {
            manifest_path: managed_runtime_manifest_relative_path(kind, &version),
            version,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
struct ManagedRuntimeRegistry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    current: Option<ManagedRuntimeVersionPointer>,
    #[serde(default)]
    versions: Vec<ManagedRuntimeVersionPointer>,
}

#[allow(dead_code)]
impl ManagedRuntimeRegistry {
    fn upsert_version(
        &mut self,
        kind: ManagedRuntimeKind,
        version: impl Into<String>,
    ) -> ManagedRuntimeVersionPointer {
        let version = version.into();

        if let Some(existing) = self
            .versions
            .iter()
            .find(|pointer| pointer.version == version)
        {
            return existing.clone();
        }

        let pointer = ManagedRuntimeVersionPointer::new(kind, version);
        self.versions.push(pointer.clone());
        pointer
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ManagedRuntimeState {
    schema_version: u32,
    #[serde(default)]
    node: ManagedRuntimeRegistry,
    #[serde(default)]
    openclaw: ManagedRuntimeRegistry,
}

#[allow(dead_code)]
impl Default for ManagedRuntimeState {
    fn default() -> Self {
        Self {
            schema_version: MANAGED_RUNTIME_SCHEMA_VERSION,
            node: ManagedRuntimeRegistry::default(),
            openclaw: ManagedRuntimeRegistry::default(),
        }
    }
}

#[allow(dead_code)]
impl ManagedRuntimeState {
    fn registry(&self, kind: ManagedRuntimeKind) -> &ManagedRuntimeRegistry {
        match kind {
            ManagedRuntimeKind::Node => &self.node,
            ManagedRuntimeKind::OpenClaw => &self.openclaw,
        }
    }

    fn registry_mut(&mut self, kind: ManagedRuntimeKind) -> &mut ManagedRuntimeRegistry {
        match kind {
            ManagedRuntimeKind::Node => &mut self.node,
            ManagedRuntimeKind::OpenClaw => &mut self.openclaw,
        }
    }

    fn track_version(
        &mut self,
        kind: ManagedRuntimeKind,
        version: impl Into<String>,
    ) -> ManagedRuntimeVersionPointer {
        self.registry_mut(kind).upsert_version(kind, version)
    }

    fn set_current_version(&mut self, kind: ManagedRuntimeKind, version: impl Into<String>) {
        let pointer = self.track_version(kind, version);
        self.registry_mut(kind).current = Some(pointer);
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ManagedRuntimeManifest {
    schema_version: u32,
    runtime: ManagedRuntimeKind,
    version: String,
    runtime_dir: PathBuf,
    installed_at: String,
}

#[allow(dead_code)]
impl ManagedRuntimeManifest {
    fn new(runtime: ManagedRuntimeKind, version: impl Into<String>) -> Self {
        let version = version.into();

        Self {
            schema_version: MANAGED_RUNTIME_SCHEMA_VERSION,
            runtime,
            runtime_dir: managed_runtime_version_relative_dir(runtime, &version),
            version,
            installed_at: now_iso_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum NodeBinarySource {
    Path,
    Managed,
    Bundled,
}

impl NodeBinarySource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Managed => "managed",
            Self::Bundled => "bundled",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum OpenClawRuntimeSource {
    Managed,
    NodeModules,
    Bundled,
}

impl OpenClawRuntimeSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Managed => "managed",
            Self::NodeModules => "nodeModules",
            Self::Bundled => "bundled",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagedNodeInstallPayload {
    version: String,
    #[serde(alias = "url")]
    archive_url: String,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ManagedNodeInstallResult {
    version: String,
    archive_path: PathBuf,
    runtime_dir: PathBuf,
    manifest_path: PathBuf,
    current_path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagedOpenClawInstallPayload {
    version: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ManagedOpenClawInstallResult {
    version: String,
    archive_path: PathBuf,
    runtime_dir: PathBuf,
    manifest_path: PathBuf,
    current_path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct OpenClawRegistryReleaseMetadata {
    name: String,
    version: String,
    dist: OpenClawRegistryDistMetadata,
}

#[derive(Debug, Deserialize)]
struct OpenClawRegistryDistMetadata {
    tarball: String,
    integrity: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenClawPackageMetadata {
    name: String,
    version: String,
}

#[derive(Debug, Clone)]
struct ManagedOpenClawDownloadTarget {
    version: String,
    archive_url: String,
    file_name: String,
    integrity: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum NodeBinaryDiagnosticStatus {
    Accepted,
    Rejected,
    Missing,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum NodeBinaryDiagnosticReason {
    NotFoundInPath,
    PathDoesNotExist,
    VersionCommandFailed,
    InvalidVersion,
    UnsupportedVersion,
    SmokeTestFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct NodeBinaryDiagnostic {
    source: NodeBinarySource,
    path: Option<PathBuf>,
    status: NodeBinaryDiagnosticStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<NodeBinaryDiagnosticReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
}

impl NodeBinaryDiagnostic {
    fn accepted(source: NodeBinarySource, path: PathBuf, version: String) -> Self {
        Self {
            source,
            path: Some(path),
            status: NodeBinaryDiagnosticStatus::Accepted,
            reason: None,
            detail: None,
            version: Some(version),
        }
    }

    fn rejected(
        source: NodeBinarySource,
        path: Option<PathBuf>,
        reason: NodeBinaryDiagnosticReason,
        detail: impl Into<String>,
        version: Option<String>,
    ) -> Self {
        Self {
            source,
            path,
            status: NodeBinaryDiagnosticStatus::Rejected,
            reason: Some(reason),
            detail: Some(detail.into()),
            version,
        }
    }

    fn missing(
        source: NodeBinarySource,
        reason: NodeBinaryDiagnosticReason,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            source,
            path: None,
            status: NodeBinaryDiagnosticStatus::Missing,
            reason: Some(reason),
            detail: Some(detail.into()),
            version: None,
        }
    }

    fn is_accepted(&self) -> bool {
        self.status == NodeBinaryDiagnosticStatus::Accepted
    }

    fn summary(&self) -> String {
        let location = self
            .path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<unresolved>".into());

        match self.status {
            NodeBinaryDiagnosticStatus::Accepted => format!(
                "{} node accepted at {} ({})",
                self.source.as_str(),
                location,
                self.version.as_deref().unwrap_or("unknown version")
            ),
            NodeBinaryDiagnosticStatus::Rejected | NodeBinaryDiagnosticStatus::Missing => {
                let reason = self
                    .reason
                    .map(|reason| format!("{reason:?}"))
                    .unwrap_or_else(|| "unknown".into());
                let detail = self.detail.as_deref().unwrap_or("no details available");
                format!(
                    "{} node rejected at {} ({}: {})",
                    self.source.as_str(),
                    location,
                    reason,
                    detail
                )
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
struct NodeBinaryResolution {
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<NodeBinarySource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    diagnostics: Vec<NodeBinaryDiagnostic>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum OpenClawRuntimeDiagnosticStatus {
    Accepted,
    Rejected,
    Missing,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum OpenClawRuntimeDiagnosticReason {
    NotFound,
    PathDoesNotExist,
    MissingPackageJson,
    InvalidPackageMetadata,
    MissingEntryScript,
    MissingDistEntry,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct OpenClawRuntimeDiagnostic {
    source: OpenClawRuntimeSource,
    path: Option<PathBuf>,
    status: OpenClawRuntimeDiagnosticStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<OpenClawRuntimeDiagnosticReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
}

impl OpenClawRuntimeDiagnostic {
    fn accepted(source: OpenClawRuntimeSource, path: PathBuf, version: String) -> Self {
        Self {
            source,
            path: Some(path),
            status: OpenClawRuntimeDiagnosticStatus::Accepted,
            reason: None,
            detail: None,
            version: Some(version),
        }
    }

    fn rejected(
        source: OpenClawRuntimeSource,
        path: Option<PathBuf>,
        reason: OpenClawRuntimeDiagnosticReason,
        detail: impl Into<String>,
        version: Option<String>,
    ) -> Self {
        Self {
            source,
            path,
            status: OpenClawRuntimeDiagnosticStatus::Rejected,
            reason: Some(reason),
            detail: Some(detail.into()),
            version,
        }
    }

    fn missing(
        source: OpenClawRuntimeSource,
        reason: OpenClawRuntimeDiagnosticReason,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            source,
            path: None,
            status: OpenClawRuntimeDiagnosticStatus::Missing,
            reason: Some(reason),
            detail: Some(detail.into()),
            version: None,
        }
    }

    fn is_accepted(&self) -> bool {
        self.status == OpenClawRuntimeDiagnosticStatus::Accepted
    }

    fn summary(&self) -> String {
        let location = self
            .path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<unresolved>".into());

        match self.status {
            OpenClawRuntimeDiagnosticStatus::Accepted => format!(
                "{} OpenClaw accepted at {} ({})",
                self.source.as_str(),
                location,
                self.version.as_deref().unwrap_or("unknown version")
            ),
            OpenClawRuntimeDiagnosticStatus::Rejected
            | OpenClawRuntimeDiagnosticStatus::Missing => {
                let reason = self
                    .reason
                    .map(|reason| format!("{reason:?}"))
                    .unwrap_or_else(|| "unknown".into());
                let detail = self.detail.as_deref().unwrap_or("no details available");
                format!(
                    "{} OpenClaw rejected at {} ({}: {})",
                    self.source.as_str(),
                    location,
                    reason,
                    detail
                )
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
struct OpenClawRuntimeResolution {
    #[serde(skip_serializing_if = "Option::is_none")]
    dir: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entry_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<OpenClawRuntimeSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    diagnostics: Vec<OpenClawRuntimeDiagnostic>,
}

impl OpenClawRuntimeResolution {
    fn accepted(
        diagnostics: Vec<OpenClawRuntimeDiagnostic>,
        accepted: &OpenClawRuntimeDiagnostic,
    ) -> Self {
        let dir = accepted.path.clone();
        let entry_path = dir.as_ref().map(|path| path.join("openclaw.mjs"));
        Self {
            dir,
            entry_path,
            source: Some(accepted.source),
            version: accepted.version.clone(),
            diagnostics,
        }
    }

    fn failure_message(&self) -> String {
        if self.diagnostics.is_empty() {
            return "No OpenClaw runtime candidates were probed".into();
        }

        self.diagnostics
            .iter()
            .map(OpenClawRuntimeDiagnostic::summary)
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct RuntimeStatusPayload {
    mode: String,
    full_mode_runtime: bool,
    node: NodeBinaryResolution,
    openclaw: OpenClawRuntimeResolution,
}

impl NodeBinaryResolution {
    fn accepted(diagnostics: Vec<NodeBinaryDiagnostic>, accepted: &NodeBinaryDiagnostic) -> Self {
        Self {
            path: accepted.path.clone(),
            source: Some(accepted.source),
            version: accepted.version.clone(),
            diagnostics,
        }
    }

    fn failure_message(&self) -> String {
        if self.diagnostics.is_empty() {
            return "No Node.js candidates were probed".into();
        }

        self.diagnostics
            .iter()
            .map(NodeBinaryDiagnostic::summary)
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    theme: String,
    language: String,
    start_minimized: bool,
    launch_at_startup: bool,
    gateway_auto_start: bool,
    gateway_port: u16,
    gateway_token: String,
    proxy_enabled: bool,
    proxy_server: String,
    proxy_http_server: String,
    proxy_https_server: String,
    proxy_all_server: String,
    proxy_bypass_rules: String,
    update_channel: String,
    auto_check_update: bool,
    auto_download_update: bool,
    skipped_versions: Vec<String>,
    sidebar_collapsed: bool,
    dev_mode_unlocked: bool,
    selected_bundles: Vec<String>,
    enabled_skills: Vec<String>,
    disabled_skills: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct ResolvedProxySettings {
    http_proxy: String,
    https_proxy: String,
    all_proxy: String,
    bypass_rules: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            language: "en".into(),
            start_minimized: false,
            launch_at_startup: false,
            gateway_auto_start: true,
            gateway_port: DEFAULT_GATEWAY_PORT,
            gateway_token: format!("clawy-{}", Uuid::new_v4().simple()),
            proxy_enabled: false,
            proxy_server: String::new(),
            proxy_http_server: String::new(),
            proxy_https_server: String::new(),
            proxy_all_server: String::new(),
            proxy_bypass_rules: "<local>;localhost;127.0.0.1;::1".into(),
            update_channel: "stable".into(),
            auto_check_update: true,
            auto_download_update: false,
            skipped_versions: Vec::new(),
            sidebar_collapsed: false,
            dev_mode_unlocked: false,
            selected_bundles: vec!["productivity".into(), "developer".into()],
            enabled_skills: Vec::new(),
            disabled_skills: Vec::new(),
        }
    }
}

fn trim_value(value: &str) -> String {
    value.trim().to_string()
}

fn normalize_proxy_server(proxy_server: &str) -> String {
    let value = trim_value(proxy_server);
    if value.is_empty() {
        return value;
    }
    if value.contains("://") {
        return value;
    }
    format!("http://{value}")
}

fn resolve_proxy_settings(settings: &Settings) -> ResolvedProxySettings {
    let legacy_proxy = normalize_proxy_server(&settings.proxy_server);
    let all_proxy = normalize_proxy_server(&settings.proxy_all_server);

    let http_proxy = {
        let value = normalize_proxy_server(&settings.proxy_http_server);
        if !value.is_empty() {
            value
        } else if !legacy_proxy.is_empty() {
            legacy_proxy.clone()
        } else {
            all_proxy.clone()
        }
    };

    let https_proxy = {
        let value = normalize_proxy_server(&settings.proxy_https_server);
        if !value.is_empty() {
            value
        } else if !legacy_proxy.is_empty() {
            legacy_proxy.clone()
        } else {
            all_proxy.clone()
        }
    };

    ResolvedProxySettings {
        http_proxy,
        https_proxy,
        all_proxy: if !all_proxy.is_empty() {
            all_proxy
        } else {
            legacy_proxy
        },
        bypass_rules: trim_value(&settings.proxy_bypass_rules),
    }
}

fn proxy_env_pairs(settings: &Settings) -> Vec<(&'static str, String)> {
    if !settings.proxy_enabled {
        return vec![
            ("HTTP_PROXY", String::new()),
            ("HTTPS_PROXY", String::new()),
            ("ALL_PROXY", String::new()),
            ("http_proxy", String::new()),
            ("https_proxy", String::new()),
            ("all_proxy", String::new()),
            ("NO_PROXY", String::new()),
            ("no_proxy", String::new()),
        ];
    }

    let resolved = resolve_proxy_settings(settings);
    let no_proxy = resolved
        .bypass_rules
        .split([',', '\n', ';'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(",");

    vec![
        ("HTTP_PROXY", resolved.http_proxy.clone()),
        ("HTTPS_PROXY", resolved.https_proxy.clone()),
        ("ALL_PROXY", resolved.all_proxy.clone()),
        ("http_proxy", resolved.http_proxy),
        ("https_proxy", resolved.https_proxy),
        ("all_proxy", resolved.all_proxy),
        ("NO_PROXY", no_proxy.clone()),
        ("no_proxy", no_proxy),
    ]
}

fn apply_proxy_env(command: &mut Command, settings: &Settings) {
    for (key, value) in proxy_env_pairs(settings) {
        command.env(key, value);
    }
}

fn proxy_changed(previous: &Settings, next: &Settings) -> bool {
    previous.proxy_enabled != next.proxy_enabled
        || previous.proxy_server != next.proxy_server
        || previous.proxy_http_server != next.proxy_http_server
        || previous.proxy_https_server != next.proxy_https_server
        || previous.proxy_all_server != next.proxy_all_server
        || previous.proxy_bypass_rules != next.proxy_bypass_rules
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderConfig {
    id: String,
    name: String,
    #[serde(rename = "type")]
    provider_type: String,
    base_url: Option<String>,
    model: Option<String>,
    fallback_models: Option<Vec<String>>,
    fallback_provider_ids: Option<Vec<String>>,
    enabled: bool,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderWithKeyInfo {
    #[serde(flatten)]
    config: ProviderConfig,
    has_key: bool,
    key_masked: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ProviderStore {
    providers: HashMap<String, ProviderConfig>,
    api_keys: HashMap<String, String>,
    default_provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredDeviceIdentity {
    version: u8,
    device_id: String,
    public_key: String,
    secret_key: String,
    created_at_ms: u64,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn now_iso_string() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| now_ms().to_string())
}

fn platform_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        _ => "linux",
    }
}

#[allow(dead_code)]
fn clawy_base_dir_from_home(home_dir: &Path) -> PathBuf {
    home_dir.join(".clawy-tauri")
}

#[allow(dead_code)]
fn openclaw_config_dir_from_home(home_dir: &Path) -> PathBuf {
    home_dir.join(".openclaw")
}

#[allow(dead_code)]
fn managed_runtime_root_dir_from_base(base_dir: &Path) -> PathBuf {
    base_dir.join(MANAGED_RUNTIME_DIR_NAME)
}

#[allow(dead_code)]
fn managed_runtime_root_dir() -> PathBuf {
    managed_runtime_root_dir_from_base(&clawy_base_dir())
}

#[allow(dead_code)]
fn managed_runtime_downloads_dir_from_base(base_dir: &Path) -> PathBuf {
    managed_runtime_root_dir_from_base(base_dir).join(MANAGED_RUNTIME_DOWNLOADS_DIR_NAME)
}

#[allow(dead_code)]
fn managed_runtime_downloads_dir() -> PathBuf {
    managed_runtime_downloads_dir_from_base(&clawy_base_dir())
}

#[allow(dead_code)]
fn managed_runtime_dir_from_base(base_dir: &Path, kind: ManagedRuntimeKind) -> PathBuf {
    managed_runtime_root_dir_from_base(base_dir).join(kind.dir_name())
}

#[allow(dead_code)]
fn managed_runtime_dir(kind: ManagedRuntimeKind) -> PathBuf {
    managed_runtime_dir_from_base(&clawy_base_dir(), kind)
}

#[allow(dead_code)]
fn managed_runtime_versions_dir_from_base(base_dir: &Path, kind: ManagedRuntimeKind) -> PathBuf {
    managed_runtime_dir_from_base(base_dir, kind).join(MANAGED_RUNTIME_VERSIONS_DIR_NAME)
}

#[allow(dead_code)]
fn managed_runtime_versions_dir(kind: ManagedRuntimeKind) -> PathBuf {
    managed_runtime_versions_dir_from_base(&clawy_base_dir(), kind)
}

#[allow(dead_code)]
fn managed_runtime_staging_dir_from_base(base_dir: &Path, kind: ManagedRuntimeKind) -> PathBuf {
    managed_runtime_dir_from_base(base_dir, kind).join(MANAGED_RUNTIME_STAGING_DIR_NAME)
}

#[allow(dead_code)]
fn managed_runtime_staging_dir(kind: ManagedRuntimeKind) -> PathBuf {
    managed_runtime_staging_dir_from_base(&clawy_base_dir(), kind)
}

#[allow(dead_code)]
fn managed_runtime_version_dir_from_base(
    base_dir: &Path,
    kind: ManagedRuntimeKind,
    version: &str,
) -> PathBuf {
    managed_runtime_versions_dir_from_base(base_dir, kind).join(version)
}

#[allow(dead_code)]
fn managed_runtime_version_dir(kind: ManagedRuntimeKind, version: &str) -> PathBuf {
    managed_runtime_version_dir_from_base(&clawy_base_dir(), kind, version)
}

#[allow(dead_code)]
fn managed_runtime_current_pointer_path_from_base(
    base_dir: &Path,
    kind: ManagedRuntimeKind,
) -> PathBuf {
    managed_runtime_dir_from_base(base_dir, kind).join(MANAGED_RUNTIME_CURRENT_POINTER_FILE_NAME)
}

#[allow(dead_code)]
fn managed_runtime_current_pointer_path(kind: ManagedRuntimeKind) -> PathBuf {
    managed_runtime_current_pointer_path_from_base(&clawy_base_dir(), kind)
}

#[allow(dead_code)]
fn managed_runtime_state_path_from_base(base_dir: &Path) -> PathBuf {
    managed_runtime_root_dir_from_base(base_dir).join(MANAGED_RUNTIME_STATE_FILE_NAME)
}

#[allow(dead_code)]
fn managed_runtime_state_path() -> PathBuf {
    managed_runtime_state_path_from_base(&clawy_base_dir())
}

#[allow(dead_code)]
fn managed_runtime_manifest_path_from_base(
    base_dir: &Path,
    kind: ManagedRuntimeKind,
    version: &str,
) -> PathBuf {
    managed_runtime_version_dir_from_base(base_dir, kind, version)
        .join(MANAGED_RUNTIME_MANIFEST_FILE_NAME)
}

#[allow(dead_code)]
fn managed_runtime_manifest_path(kind: ManagedRuntimeKind, version: &str) -> PathBuf {
    managed_runtime_manifest_path_from_base(&clawy_base_dir(), kind, version)
}

#[allow(dead_code)]
fn managed_runtime_version_relative_dir(kind: ManagedRuntimeKind, version: &str) -> PathBuf {
    PathBuf::from(kind.dir_name())
        .join(MANAGED_RUNTIME_VERSIONS_DIR_NAME)
        .join(version)
}

#[allow(dead_code)]
fn managed_runtime_manifest_relative_path(kind: ManagedRuntimeKind, version: &str) -> PathBuf {
    managed_runtime_version_relative_dir(kind, version).join(MANAGED_RUNTIME_MANIFEST_FILE_NAME)
}

fn clawy_base_dir() -> PathBuf {
    clawy_base_dir_from_home(&dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
}

fn logs_dir() -> PathBuf {
    clawy_base_dir().join("logs")
}

fn settings_path() -> PathBuf {
    clawy_base_dir().join("settings.json")
}

fn providers_path() -> PathBuf {
    clawy_base_dir().join("providers.json")
}

fn device_identity_path() -> PathBuf {
    clawy_base_dir().join("gateway-device-identity.json")
}

fn outbound_media_dir() -> PathBuf {
    openclaw_config_dir().join("media").join("outbound")
}

fn openclaw_config_dir() -> PathBuf {
    openclaw_config_dir_from_home(&dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
}

fn openclaw_skills_dir() -> PathBuf {
    openclaw_config_dir().join("skills")
}

fn ensure_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|err| err.to_string())
}

fn read_json_or_default<T>(path: &Path) -> T
where
    T: for<'de> Deserialize<'de> + Default,
{
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

#[allow(dead_code)]
fn load_managed_runtime_state_from_base(base_dir: &Path) -> ManagedRuntimeState {
    read_json_or_default(&managed_runtime_state_path_from_base(base_dir))
}

#[allow(dead_code)]
fn load_managed_runtime_state() -> ManagedRuntimeState {
    load_managed_runtime_state_from_base(&clawy_base_dir())
}

#[allow(dead_code)]
fn save_managed_runtime_state_to_base(
    base_dir: &Path,
    state: &ManagedRuntimeState,
) -> Result<(), String> {
    write_json(&managed_runtime_state_path_from_base(base_dir), state)
}

#[allow(dead_code)]
fn save_managed_runtime_state(state: &ManagedRuntimeState) -> Result<(), String> {
    save_managed_runtime_state_to_base(&clawy_base_dir(), state)
}

#[allow(dead_code)]
fn load_or_create_managed_runtime_state_in_base(
    base_dir: &Path,
) -> Result<ManagedRuntimeState, String> {
    let path = managed_runtime_state_path_from_base(base_dir);
    if path.exists() {
        return Ok(load_managed_runtime_state_from_base(base_dir));
    }

    let state = ManagedRuntimeState::default();
    save_managed_runtime_state_to_base(base_dir, &state)?;
    Ok(state)
}

#[allow(dead_code)]
fn load_or_create_managed_runtime_state() -> Result<ManagedRuntimeState, String> {
    load_or_create_managed_runtime_state_in_base(&clawy_base_dir())
}

#[allow(dead_code)]
fn load_managed_runtime_manifest_from_base(
    base_dir: &Path,
    kind: ManagedRuntimeKind,
    version: &str,
) -> ManagedRuntimeManifest {
    let path = managed_runtime_manifest_path_from_base(base_dir, kind, version);

    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_else(|| ManagedRuntimeManifest::new(kind, version))
}

#[allow(dead_code)]
fn load_managed_runtime_manifest(
    kind: ManagedRuntimeKind,
    version: &str,
) -> ManagedRuntimeManifest {
    load_managed_runtime_manifest_from_base(&clawy_base_dir(), kind, version)
}

#[allow(dead_code)]
fn save_managed_runtime_manifest_to_base(
    base_dir: &Path,
    manifest: &ManagedRuntimeManifest,
) -> Result<(), String> {
    write_json(
        &managed_runtime_manifest_path_from_base(base_dir, manifest.runtime, &manifest.version),
        manifest,
    )
}

#[allow(dead_code)]
fn save_managed_runtime_manifest(manifest: &ManagedRuntimeManifest) -> Result<(), String> {
    save_managed_runtime_manifest_to_base(&clawy_base_dir(), manifest)
}

#[allow(dead_code)]
fn load_or_create_managed_runtime_manifest_in_base(
    base_dir: &Path,
    kind: ManagedRuntimeKind,
    version: &str,
) -> Result<ManagedRuntimeManifest, String> {
    let path = managed_runtime_manifest_path_from_base(base_dir, kind, version);
    if path.exists() {
        return Ok(load_managed_runtime_manifest_from_base(
            base_dir, kind, version,
        ));
    }

    let manifest = ManagedRuntimeManifest::new(kind, version);
    save_managed_runtime_manifest_to_base(base_dir, &manifest)?;
    Ok(manifest)
}

#[allow(dead_code)]
fn load_or_create_managed_runtime_manifest(
    kind: ManagedRuntimeKind,
    version: &str,
) -> Result<ManagedRuntimeManifest, String> {
    load_or_create_managed_runtime_manifest_in_base(&clawy_base_dir(), kind, version)
}

fn write_json<T>(path: &Path, value: &T) -> Result<(), String>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let content = serde_json::to_string_pretty(value).map_err(|err| err.to_string())?;
    fs::write(path, content).map_err(|err| err.to_string())
}

fn remove_path_if_exists(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }

    if path.is_dir() {
        fs::remove_dir_all(path).map_err(|err| err.to_string())
    } else {
        fs::remove_file(path).map_err(|err| err.to_string())
    }
}

fn write_text_atomically(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }

    let file_name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "pointer".into());
    let temp_path = path.with_file_name(format!(".{file_name}.{}", Uuid::new_v4().simple()));

    fs::write(&temp_path, contents).map_err(|err| err.to_string())?;
    if path.exists() {
        remove_path_if_exists(path)?;
    }
    fs::rename(&temp_path, path).map_err(|err| err.to_string())
}

fn normalized_sha256_hex(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.len() != 64 || !normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err("Managed Node archive SHA-256 must be a 64-character hex string".into());
    }

    Ok(normalized)
}

fn sha256_digest_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|err| err.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];

    loop {
        let read = file.read(&mut buffer).map_err(|err| err.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn verify_file_sha256(path: &Path, expected_sha256: &str) -> Result<(), String> {
    let expected = normalized_sha256_hex(expected_sha256)?;
    let actual = sha256_digest_file(path)?;
    if actual == expected {
        return Ok(());
    }

    Err(format!(
        "Managed Node archive checksum mismatch for {}: expected {expected}, got {actual}",
        path.to_string_lossy()
    ))
}

fn verify_file_integrity(path: &Path, integrity: &str) -> Result<(), String> {
    let trimmed = integrity.trim();
    let (algorithm, encoded_expected) = trimmed
        .split_once('-')
        .ok_or_else(|| format!("Unsupported integrity format `{trimmed}`"))?;
    let expected = STANDARD
        .decode(encoded_expected)
        .map_err(|err| format!("Invalid integrity value `{trimmed}`: {err}"))?;

    let actual = match algorithm {
        "sha512" => {
            let mut file = File::open(path).map_err(|err| err.to_string())?;
            let mut hasher = Sha512::new();
            let mut buffer = [0_u8; 8192];

            loop {
                let read = file.read(&mut buffer).map_err(|err| err.to_string())?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }

            hasher.finalize().to_vec()
        }
        "sha256" => {
            let mut file = File::open(path).map_err(|err| err.to_string())?;
            let mut hasher = Sha256::new();
            let mut buffer = [0_u8; 8192];

            loop {
                let read = file.read(&mut buffer).map_err(|err| err.to_string())?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }

            hasher.finalize().to_vec()
        }
        _ => {
            return Err(format!(
                "Unsupported integrity algorithm `{algorithm}` for {}",
                path.to_string_lossy()
            ));
        }
    };

    if actual == expected {
        return Ok(());
    }

    Err(format!(
        "Managed OpenClaw archive integrity mismatch for {}",
        path.to_string_lossy()
    ))
}

fn read_openclaw_package_metadata(root: &Path) -> Result<OpenClawPackageMetadata, String> {
    let package_path = root.join("package.json");
    let content = fs::read_to_string(&package_path).map_err(|err| {
        format!(
            "Failed to read OpenClaw package metadata at {}: {err}",
            package_path.to_string_lossy()
        )
    })?;
    serde_json::from_str::<OpenClawPackageMetadata>(&content).map_err(|err| {
        format!(
            "Failed to parse OpenClaw package metadata at {}: {err}",
            package_path.to_string_lossy()
        )
    })
}

fn validate_managed_openclaw_runtime_dir(
    root: &Path,
    expected_version: Option<&str>,
) -> Result<(), String> {
    let entry_path = root.join("openclaw.mjs");
    if !entry_path.exists() {
        return Err(format!(
            "Managed OpenClaw runtime is missing `openclaw.mjs` under {}",
            root.to_string_lossy()
        ));
    }

    let package = read_openclaw_package_metadata(root)?;
    if package.name != OPENCLAW_PACKAGE_NAME {
        return Err(format!(
            "Managed OpenClaw archive contains package `{}` instead of `{OPENCLAW_PACKAGE_NAME}`",
            package.name
        ));
    }

    if let Some(version) = expected_version {
        let trimmed_version = version.trim();
        if !trimmed_version.is_empty() && package.version != trimmed_version {
            return Err(format!(
                "Managed OpenClaw archive version mismatch: expected {trimmed_version}, got {}",
                package.version
            ));
        }
    }

    let has_dist_entry =
        root.join("dist").join("entry.js").exists() || root.join("dist").join("entry.mjs").exists();
    if !has_dist_entry {
        return Err(format!(
            "Managed OpenClaw runtime is missing `dist/entry.js` or `dist/entry.mjs` under {}",
            root.to_string_lossy()
        ));
    }

    let node_modules_dir = root.join("node_modules");
    if !node_modules_dir.is_dir() {
        return Err(format!(
            "Managed OpenClaw runtime is missing `node_modules` under {}",
            root.to_string_lossy()
        ));
    }

    Ok(())
}

fn file_name_from_url(url: &str) -> Result<String, String> {
    let sanitized = url.split('#').next().unwrap_or(url);
    let without_query = sanitized.split('?').next().unwrap_or(sanitized);
    let file_name = without_query.rsplit('/').next().unwrap_or_default().trim();

    if file_name.is_empty() {
        return Err(format!(
            "Could not derive archive file name from URL `{url}`"
        ));
    }

    Ok(file_name.to_string())
}

fn download_http_response_to_file(
    mut response: reqwest::blocking::Response,
    destination: &Path,
    mut on_progress: Option<&mut dyn FnMut(DownloadProgressPayload)>,
) -> Result<(), String> {
    let total = response.content_length().unwrap_or(0);
    let mut output = File::create(destination).map_err(|err| err.to_string())?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut transferred = 0_u64;
    let started_at = SystemTime::now();

    if let Some(progress_cb) = on_progress.as_deref_mut() {
        progress_cb(DownloadProgressPayload {
            total,
            delta: 0,
            transferred: 0,
            percent: 0.0,
            bytes_per_second: 0,
        });
    }

    loop {
        let read = response.read(&mut buffer).map_err(|err| err.to_string())?;
        if read == 0 {
            break;
        }

        output
            .write_all(&buffer[..read])
            .map_err(|err| err.to_string())?;
        transferred = transferred.saturating_add(read as u64);

        if let Some(progress_cb) = on_progress.as_deref_mut() {
            let elapsed_ms = started_at
                .elapsed()
                .unwrap_or_else(|_| Duration::from_millis(1))
                .as_millis()
                .max(1) as u64;
            let bytes_per_second = transferred.saturating_mul(1000) / elapsed_ms;
            let percent = if total > 0 {
                (transferred as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            progress_cb(DownloadProgressPayload {
                total,
                delta: read as u64,
                transferred,
                percent,
                bytes_per_second,
            });
        }
    }

    output.flush().map_err(|err| err.to_string())
}

fn managed_node_binary_relative_path() -> &'static str {
    if cfg!(windows) {
        "node.exe"
    } else {
        "bin/node"
    }
}

fn managed_node_binary_path_for_version_from_base(base_dir: &Path, version: &str) -> PathBuf {
    managed_runtime_version_dir_from_base(base_dir, ManagedRuntimeKind::Node, version)
        .join(managed_node_binary_relative_path())
}

fn managed_openclaw_dir_for_version_from_base(base_dir: &Path, version: &str) -> PathBuf {
    managed_runtime_version_dir_from_base(base_dir, ManagedRuntimeKind::OpenClaw, version)
}

#[allow(dead_code)]
fn managed_openclaw_entry_path_for_version_from_base(base_dir: &Path, version: &str) -> PathBuf {
    managed_openclaw_dir_for_version_from_base(base_dir, version).join("openclaw.mjs")
}

fn managed_runtime_current_version_from_base(
    base_dir: &Path,
    kind: ManagedRuntimeKind,
) -> Option<String> {
    let pointer_path = managed_runtime_current_pointer_path_from_base(base_dir, kind);
    if let Ok(contents) = fs::read_to_string(&pointer_path) {
        let version = contents.trim();
        if !version.is_empty() {
            return Some(version.to_string());
        }
    }

    load_managed_runtime_state_from_base(base_dir)
        .registry(kind)
        .current
        .as_ref()
        .map(|pointer| pointer.version.clone())
}

fn managed_node_binary_path_from_base(base_dir: &Path) -> Option<PathBuf> {
    let version = managed_runtime_current_version_from_base(base_dir, ManagedRuntimeKind::Node)?;
    let path = managed_node_binary_path_for_version_from_base(base_dir, &version);
    path.exists().then_some(path)
}

fn managed_node_binary_path() -> Option<PathBuf> {
    managed_node_binary_path_from_base(&clawy_base_dir())
}

fn managed_openclaw_dir_from_base(base_dir: &Path) -> Option<PathBuf> {
    let version =
        managed_runtime_current_version_from_base(base_dir, ManagedRuntimeKind::OpenClaw)?;
    let path = managed_openclaw_dir_for_version_from_base(base_dir, &version);
    validate_managed_openclaw_runtime_dir(&path, Some(&version))
        .ok()
        .map(|_| path)
}

fn managed_openclaw_dir() -> Option<PathBuf> {
    managed_openclaw_dir_from_base(&clawy_base_dir())
}

fn emit_runtime_install_progress(
    app: &AppHandle,
    kind: ManagedRuntimeKind,
    phase: &str,
    status: &str,
    percent: f64,
    version: Option<&str>,
    detail: Option<String>,
    progress: Option<DownloadProgressPayload>,
    error: Option<String>,
) {
    let payload = RuntimeInstallProgressPayload {
        runtime: kind.event_key().into(),
        phase: phase.into(),
        status: status.into(),
        percent,
        version: version
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string()),
        detail,
        progress,
        error,
    };
    let _ = app.emit("runtime:install-progress", payload);
}

fn format_download_progress_detail(progress: &DownloadProgressPayload) -> String {
    if progress.total > 0 {
        format!(
            "{} / {} at {}/s",
            human_bytes(progress.transferred),
            human_bytes(progress.total),
            human_bytes(progress.bytes_per_second)
        )
    } else if progress.transferred > 0 {
        format!(
            "{} downloaded at {}/s",
            human_bytes(progress.transferred),
            human_bytes(progress.bytes_per_second)
        )
    } else {
        "Waiting for download data...".into()
    }
}

fn runtime_stage_percent(base: f64, span: f64, progress: &DownloadProgressPayload) -> f64 {
    if progress.total == 0 {
        return base;
    }
    (base + ((progress.percent / 100.0) * span)).clamp(0.0, 100.0)
}

fn human_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".into();
    }

    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut index = 0usize;
    while value >= 1024.0 && index < UNITS.len() - 1 {
        value /= 1024.0;
        index += 1;
    }

    if index == 0 {
        format!("{} {}", bytes, UNITS[index])
    } else {
        format!("{value:.1} {}", UNITS[index])
    }
}

#[cfg(unix)]
fn ensure_unix_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .map_err(|err| err.to_string())?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).map_err(|err| err.to_string())
}

#[cfg(not(unix))]
fn ensure_unix_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn extract_zip_archive(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let archive_file = File::open(archive_path).map_err(|err| err.to_string())?;
    let mut archive =
        zip::ZipArchive::new(archive_file).map_err(|err| format!("Invalid zip archive: {err}"))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| format!("Failed to read zip entry #{index}: {err}"))?;
        let enclosed_name = entry
            .enclosed_name()
            .ok_or_else(|| format!("Zip archive contains an unsafe path: {}", entry.name()))?;
        let output_path = destination.join(enclosed_name);

        if entry.is_dir() {
            ensure_dir(&output_path)?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            ensure_dir(parent)?;
        }

        let mut output = File::create(&output_path).map_err(|err| err.to_string())?;
        std::io::copy(&mut entry, &mut output).map_err(|err| err.to_string())?;

        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;

            fs::set_permissions(&output_path, fs::Permissions::from_mode(mode))
                .map_err(|err| err.to_string())?;
        }
    }

    Ok(())
}

fn extract_tar_gz_archive(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let archive_file = File::open(archive_path).map_err(|err| err.to_string())?;
    let decoder = flate2::read::GzDecoder::new(archive_file);
    let mut archive = tar::Archive::new(decoder);

    for entry in archive.entries().map_err(|err| err.to_string())? {
        let mut entry = entry.map_err(|err| err.to_string())?;
        entry
            .unpack_in(destination)
            .map_err(|err| err.to_string())?;
    }

    Ok(())
}

fn extract_tar_xz_archive(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let archive_file = File::open(archive_path).map_err(|err| err.to_string())?;
    let decoder = xz2::read::XzDecoder::new(archive_file);
    let mut archive = tar::Archive::new(decoder);

    for entry in archive.entries().map_err(|err| err.to_string())? {
        let mut entry = entry.map_err(|err| err.to_string())?;
        entry
            .unpack_in(destination)
            .map_err(|err| err.to_string())?;
    }

    Ok(())
}

fn extract_archive_to_dir(archive_path: &Path, destination: &Path) -> Result<(), String> {
    ensure_dir(destination)?;
    let archive_name = file_name_from_path(archive_path).to_ascii_lowercase();

    if archive_name.ends_with(".zip") {
        return extract_zip_archive(archive_path, destination);
    }
    if archive_name.ends_with(".tar.gz") || archive_name.ends_with(".tgz") {
        return extract_tar_gz_archive(archive_path, destination);
    }
    if archive_name.ends_with(".tar.xz") {
        return extract_tar_xz_archive(archive_path, destination);
    }

    Err(format!(
        "Unsupported managed runtime archive format: {}",
        archive_path.to_string_lossy()
    ))
}

fn collapse_single_extracted_root_directory(root: &Path) -> Result<(), String> {
    let entries = fs::read_dir(root)
        .map_err(|err| err.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.to_string())?;

    if entries.len() != 1 {
        return Ok(());
    }

    let nested = entries[0].path();
    if !nested.is_dir() {
        return Ok(());
    }

    for child in fs::read_dir(&nested).map_err(|err| err.to_string())? {
        let child = child.map_err(|err| err.to_string())?;
        fs::rename(child.path(), root.join(child.file_name())).map_err(|err| err.to_string())?;
    }

    fs::remove_dir(&nested).map_err(|err| err.to_string())
}

fn prepare_managed_node_runtime_dir(root: &Path) -> Result<(), String> {
    collapse_single_extracted_root_directory(root)?;

    let node_binary = root.join(managed_node_binary_relative_path());
    if !node_binary.exists() {
        return Err(format!(
            "Managed Node archive did not produce `{}` under {}",
            managed_node_binary_relative_path(),
            root.to_string_lossy()
        ));
    }

    ensure_unix_executable(&node_binary)
}

fn prepare_managed_openclaw_runtime_dir(root: &Path, expected_version: &str) -> Result<(), String> {
    collapse_single_extracted_root_directory(root)?;
    validate_managed_openclaw_runtime_dir(root, Some(expected_version))
}

fn recommended_openclaw_version() -> Result<String, String> {
    static VERSION: OnceLock<Option<String>> = OnceLock::new();

    VERSION
        .get_or_init(|| {
            let package = serde_json::from_str::<Value>(include_str!("../../package.json")).ok()?;
            let version = package
                .get("devDependencies")?
                .get(OPENCLAW_PACKAGE_NAME)?
                .as_str()?
                .trim();

            if version.is_empty() {
                return None;
            }

            Some(version.trim_start_matches(['^', '~']).to_string())
        })
        .clone()
        .ok_or_else(|| "Unable to determine the recommended OpenClaw version".into())
}

fn node_distribution_os() -> Result<&'static str, String> {
    match std::env::consts::OS {
        "macos" => Ok("darwin"),
        "windows" => Ok("win"),
        "linux" => Ok("linux"),
        other => Err(format!("Managed Node downloads are not supported on `{other}`")),
    }
}

fn node_distribution_arch() -> Result<&'static str, String> {
    match std::env::consts::ARCH {
        "aarch64" => Ok("arm64"),
        "x86_64" => Ok("x64"),
        other => Err(format!("Managed Node downloads are not supported on `{other}`")),
    }
}

fn recommended_node_archive_name(version: &str) -> Result<String, String> {
    let os = node_distribution_os()?;
    let arch = node_distribution_arch()?;
    let extension = if cfg!(windows) { "zip" } else { "tar.gz" };

    Ok(format!("node-v{version}-{os}-{arch}.{extension}"))
}

fn resolve_recommended_managed_node_payload_with_client(
    client: &reqwest::blocking::Client,
) -> Result<ManagedNodeInstallPayload, String> {
    let version = RECOMMENDED_MANAGED_NODE_VERSION;
    let archive_name = recommended_node_archive_name(version)?;
    let shasums_url = format!("https://nodejs.org/dist/v{version}/SHASUMS256.txt");

    let shasums = client
        .get(&shasums_url)
        .send()
        .map_err(|err| format!("Failed to resolve managed Node {version} checksums: {err}"))?
        .error_for_status()
        .map_err(|err| format!("Failed to fetch managed Node {version} checksums: {err}"))?
        .text()
        .map_err(|err| format!("Failed to read managed Node {version} checksums: {err}"))?;

    let sha256 = shasums
        .lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            let digest = parts.next()?;
            let file_name = parts.next()?;
            (file_name == archive_name).then_some(digest.to_string())
        })
        .ok_or_else(|| {
            format!(
                "Managed Node checksums did not include `{archive_name}` for version {version}"
            )
        })?;

    Ok(ManagedNodeInstallPayload {
        version: version.into(),
        archive_url: format!("https://nodejs.org/dist/v{version}/{archive_name}"),
        sha256,
    })
}

fn openclaw_registry_release_url(registry_base_url: &str, version: &str) -> String {
    format!(
        "{}/{}/{}",
        registry_base_url.trim_end_matches('/'),
        OPENCLAW_PACKAGE_NAME,
        version
    )
}

fn resolve_managed_openclaw_download_target_with_client_and_registry(
    client: &reqwest::blocking::Client,
    version: &str,
    registry_base_url: &str,
) -> Result<ManagedOpenClawDownloadTarget, String> {
    let trimmed_version = version.trim();
    if trimmed_version.is_empty() {
        return Err("Managed OpenClaw version is required".into());
    }

    let response = client
        .get(openclaw_registry_release_url(
            registry_base_url,
            trimmed_version,
        ))
        .send()
        .map_err(|err| format!("Failed to resolve managed OpenClaw {trimmed_version}: {err}"))?
        .error_for_status()
        .map_err(|err| {
            format!("Managed OpenClaw {trimmed_version} metadata request failed: {err}")
        })?;
    let metadata = response
        .json::<OpenClawRegistryReleaseMetadata>()
        .map_err(|err| {
            format!("Failed to parse managed OpenClaw {trimmed_version} metadata: {err}")
        })?;

    if metadata.name != OPENCLAW_PACKAGE_NAME {
        return Err(format!(
            "Resolved `{}` instead of `{OPENCLAW_PACKAGE_NAME}` for managed OpenClaw {trimmed_version}",
            metadata.name
        ));
    }

    if metadata.version != trimmed_version {
        return Err(format!(
            "Managed OpenClaw metadata version mismatch: expected {trimmed_version}, got {}",
            metadata.version
        ));
    }

    let integrity = metadata.dist.integrity.ok_or_else(|| {
        format!("Managed OpenClaw {trimmed_version} is missing tarball integrity metadata")
    })?;
    let file_name = file_name_from_url(&metadata.dist.tarball)?;

    Ok(ManagedOpenClawDownloadTarget {
        version: trimmed_version.to_string(),
        archive_url: metadata.dist.tarball,
        file_name,
        integrity,
    })
}

fn download_managed_openclaw_archive_with_client_in_base_and_registry(
    base_dir: &Path,
    client: &reqwest::blocking::Client,
    version: &str,
    registry_base_url: &str,
    on_progress: Option<&mut dyn FnMut(DownloadProgressPayload)>,
) -> Result<PathBuf, String> {
    let target = resolve_managed_openclaw_download_target_with_client_and_registry(
        client,
        version,
        registry_base_url,
    )?;
    let downloads_dir = managed_runtime_downloads_dir_from_base(base_dir);
    ensure_dir(&downloads_dir)?;

    let destination = downloads_dir.join(&target.file_name);
    if destination.exists() {
        match verify_file_integrity(&destination, &target.integrity) {
            Ok(()) => return Ok(destination),
            Err(_) => remove_path_if_exists(&destination)?,
        }
    }

    let temp_path =
        downloads_dir.join(format!(".{}.{}", target.file_name, Uuid::new_v4().simple()));
    let response = client
        .get(&target.archive_url)
        .send()
        .map_err(|err| {
            format!(
                "Failed to download managed OpenClaw {}: {err}",
                target.version
            )
        })?
        .error_for_status()
        .map_err(|err| format!("Managed OpenClaw {} download failed: {err}", target.version))?;

    let download_result = download_http_response_to_file(response, &temp_path, on_progress);

    if let Err(err) = download_result {
        let _ = remove_path_if_exists(&temp_path);
        return Err(err);
    }

    if let Err(err) = verify_file_integrity(&temp_path, &target.integrity) {
        let _ = remove_path_if_exists(&temp_path);
        return Err(err);
    }

    if destination.exists() {
        remove_path_if_exists(&destination)?;
    }
    fs::rename(&temp_path, &destination).map_err(|err| err.to_string())?;

    Ok(destination)
}

fn download_managed_openclaw_archive_with_client_in_base(
    base_dir: &Path,
    client: &reqwest::blocking::Client,
    version: &str,
) -> Result<PathBuf, String> {
    download_managed_openclaw_archive_with_client_in_base_and_registry(
        base_dir,
        client,
        version,
        OPENCLAW_NPM_REGISTRY_BASE_URL,
        None,
    )
}

fn download_managed_openclaw_archive_in_base(
    base_dir: &Path,
    version: &str,
) -> Result<PathBuf, String> {
    let client = reqwest_client_with_timeout(Duration::from_secs(300))?;
    download_managed_openclaw_archive_with_client_in_base(base_dir, &client, version)
}

fn download_managed_node_archive_with_client_in_base(
    base_dir: &Path,
    client: &reqwest::blocking::Client,
    payload: &ManagedNodeInstallPayload,
    on_progress: Option<&mut dyn FnMut(DownloadProgressPayload)>,
) -> Result<PathBuf, String> {
    let version = payload.version.trim();
    if version.is_empty() {
        return Err("Managed Node version is required".into());
    }

    let file_name = file_name_from_url(&payload.archive_url)?;
    let downloads_dir = managed_runtime_downloads_dir_from_base(base_dir);
    ensure_dir(&downloads_dir)?;

    let destination = downloads_dir.join(&file_name);
    if destination.exists() {
        match verify_file_sha256(&destination, &payload.sha256) {
            Ok(()) => return Ok(destination),
            Err(_) => remove_path_if_exists(&destination)?,
        }
    }

    let temp_path = downloads_dir.join(format!(".{file_name}.{}", Uuid::new_v4().simple()));
    let response = client
        .get(&payload.archive_url)
        .send()
        .map_err(|err| format!("Failed to download managed Node archive: {err}"))?
        .error_for_status()
        .map_err(|err| format!("Managed Node archive download failed: {err}"))?;

    let download_result = download_http_response_to_file(response, &temp_path, on_progress);

    if let Err(err) = download_result {
        let _ = remove_path_if_exists(&temp_path);
        return Err(err);
    }

    if let Err(err) = verify_file_sha256(&temp_path, &payload.sha256) {
        let _ = remove_path_if_exists(&temp_path);
        return Err(err);
    }

    if destination.exists() {
        remove_path_if_exists(&destination)?;
    }
    fs::rename(&temp_path, &destination).map_err(|err| err.to_string())?;

    Ok(destination)
}

fn download_managed_node_archive_in_base(
    base_dir: &Path,
    payload: &ManagedNodeInstallPayload,
) -> Result<PathBuf, String> {
    let client = reqwest_client_with_timeout(Duration::from_secs(300))?;
    download_managed_node_archive_with_client_in_base(base_dir, &client, payload, None)
}

fn activate_managed_runtime_version_in_base(
    base_dir: &Path,
    kind: ManagedRuntimeKind,
    version: &str,
) -> Result<ManagedRuntimeVersionPointer, String> {
    let trimmed_version = version.trim();
    if trimmed_version.is_empty() {
        return Err("Managed runtime version is required".into());
    }

    let runtime_dir = managed_runtime_version_dir_from_base(base_dir, kind, trimmed_version);
    if !runtime_dir.exists() {
        return Err(format!(
            "Managed runtime version {} is not installed at {}",
            trimmed_version,
            runtime_dir.to_string_lossy()
        ));
    }

    match kind {
        ManagedRuntimeKind::Node => {
            let node_binary =
                managed_node_binary_path_for_version_from_base(base_dir, trimmed_version);
            if !node_binary.exists() {
                return Err(format!(
                    "Managed Node binary is missing at {}",
                    node_binary.to_string_lossy()
                ));
            }
            ensure_unix_executable(&node_binary)?;
        }
        ManagedRuntimeKind::OpenClaw => {
            validate_managed_openclaw_runtime_dir(&runtime_dir, Some(trimmed_version))?;
        }
    }

    load_or_create_managed_runtime_manifest_in_base(base_dir, kind, trimmed_version)?;

    let mut state = load_or_create_managed_runtime_state_in_base(base_dir)?;
    state.set_current_version(kind, trimmed_version);
    save_managed_runtime_state_to_base(base_dir, &state)?;

    write_text_atomically(
        &managed_runtime_current_pointer_path_from_base(base_dir, kind),
        &format!("{trimmed_version}\n"),
    )?;

    Ok(ManagedRuntimeVersionPointer::new(kind, trimmed_version))
}

fn install_managed_node_archive_in_base(
    base_dir: &Path,
    version: &str,
    archive_path: &Path,
) -> Result<ManagedNodeInstallResult, String> {
    let trimmed_version = version.trim();
    if trimmed_version.is_empty() {
        return Err("Managed Node version is required".into());
    }

    let runtime_dir =
        managed_runtime_version_dir_from_base(base_dir, ManagedRuntimeKind::Node, trimmed_version);
    if runtime_dir.exists() {
        load_or_create_managed_runtime_manifest_in_base(
            base_dir,
            ManagedRuntimeKind::Node,
            trimmed_version,
        )?;
        activate_managed_runtime_version_in_base(
            base_dir,
            ManagedRuntimeKind::Node,
            trimmed_version,
        )?;

        return Ok(ManagedNodeInstallResult {
            version: trimmed_version.to_string(),
            archive_path: archive_path.to_path_buf(),
            runtime_dir: runtime_dir.clone(),
            manifest_path: managed_runtime_manifest_path_from_base(
                base_dir,
                ManagedRuntimeKind::Node,
                trimmed_version,
            ),
            current_path: managed_runtime_current_pointer_path_from_base(
                base_dir,
                ManagedRuntimeKind::Node,
            ),
        });
    }

    let staging_dir = managed_runtime_staging_dir_from_base(base_dir, ManagedRuntimeKind::Node)
        .join(format!("{trimmed_version}-{}", Uuid::new_v4().simple()));
    ensure_dir(&staging_dir)?;

    if let Err(err) = extract_archive_to_dir(archive_path, &staging_dir)
        .and_then(|_| prepare_managed_node_runtime_dir(&staging_dir))
        .and_then(|_| {
            write_json(
                &staging_dir.join(MANAGED_RUNTIME_MANIFEST_FILE_NAME),
                &ManagedRuntimeManifest::new(ManagedRuntimeKind::Node, trimmed_version),
            )
        })
    {
        let _ = remove_path_if_exists(&staging_dir);
        return Err(err);
    }

    ensure_dir(&managed_runtime_versions_dir_from_base(
        base_dir,
        ManagedRuntimeKind::Node,
    ))?;

    match fs::rename(&staging_dir, &runtime_dir) {
        Ok(()) => {}
        Err(err) if runtime_dir.exists() => {
            let _ = remove_path_if_exists(&staging_dir);
            if !managed_node_binary_path_for_version_from_base(base_dir, trimmed_version).exists() {
                return Err(err.to_string());
            }
        }
        Err(err) => {
            let _ = remove_path_if_exists(&staging_dir);
            return Err(err.to_string());
        }
    }

    activate_managed_runtime_version_in_base(base_dir, ManagedRuntimeKind::Node, trimmed_version)?;

    Ok(ManagedNodeInstallResult {
        version: trimmed_version.to_string(),
        archive_path: archive_path.to_path_buf(),
        runtime_dir: runtime_dir.clone(),
        manifest_path: managed_runtime_manifest_path_from_base(
            base_dir,
            ManagedRuntimeKind::Node,
            trimmed_version,
        ),
        current_path: managed_runtime_current_pointer_path_from_base(
            base_dir,
            ManagedRuntimeKind::Node,
        ),
    })
}

fn install_managed_node_release_in_base(
    base_dir: &Path,
    payload: &ManagedNodeInstallPayload,
) -> Result<ManagedNodeInstallResult, String> {
    let archive_path = download_managed_node_archive_in_base(base_dir, payload)?;
    install_managed_node_archive_in_base(base_dir, &payload.version, &archive_path)
}

fn install_managed_node_release(
    payload: &ManagedNodeInstallPayload,
) -> Result<ManagedNodeInstallResult, String> {
    install_managed_node_release_in_base(&clawy_base_dir(), payload)
}

#[allow(dead_code)]
fn install_recommended_managed_node_release() -> Result<ManagedNodeInstallResult, String> {
    let client = reqwest_client()?;
    let payload = resolve_recommended_managed_node_payload_with_client(&client)?;
    install_managed_node_release(&payload)
}

fn install_recommended_managed_node_release_with_progress(
    app: &AppHandle,
) -> Result<ManagedNodeInstallResult, String> {
    let kind = ManagedRuntimeKind::Node;
    emit_runtime_install_progress(
        app,
        kind,
        "preparing",
        "running",
        5.0,
        Some(RECOMMENDED_MANAGED_NODE_VERSION),
        Some("Preparing managed Node.js runtime download.".into()),
        None,
        None,
    );

    let client = reqwest_client()?;
    let payload = resolve_recommended_managed_node_payload_with_client(&client)?;
    let version = payload.version.clone();

    emit_runtime_install_progress(
        app,
        kind,
        "downloading",
        "running",
        12.0,
        Some(&version),
        Some("Downloading managed Node.js runtime.".into()),
        None,
        None,
    );

    let mut download_progress = |progress: DownloadProgressPayload| {
        emit_runtime_install_progress(
            app,
            kind,
            "downloading",
            "running",
            runtime_stage_percent(12.0, 56.0, &progress),
            Some(&version),
            Some(format_download_progress_detail(&progress)),
            Some(progress),
            None,
        );
    };

    let archive_path = download_managed_node_archive_with_client_in_base(
        &clawy_base_dir(),
        &client,
        &payload,
        Some(&mut download_progress),
    )?;

    emit_runtime_install_progress(
        app,
        kind,
        "installing",
        "running",
        82.0,
        Some(&version),
        Some("Installing managed Node.js runtime.".into()),
        None,
        None,
    );

    let result = install_managed_node_archive_in_base(&clawy_base_dir(), &version, &archive_path)?;

    emit_runtime_install_progress(
        app,
        kind,
        "completed",
        "completed",
        100.0,
        Some(&version),
        Some("Managed Node.js runtime installed.".into()),
        None,
        None,
    );

    Ok(result)
}

fn install_managed_openclaw_archive_in_base(
    base_dir: &Path,
    version: &str,
    archive_path: &Path,
) -> Result<ManagedOpenClawInstallResult, String> {
    let trimmed_version = version.trim();
    if trimmed_version.is_empty() {
        return Err("Managed OpenClaw version is required".into());
    }

    let runtime_dir = managed_runtime_version_dir_from_base(
        base_dir,
        ManagedRuntimeKind::OpenClaw,
        trimmed_version,
    );
    if runtime_dir.exists() {
        validate_managed_openclaw_runtime_dir(&runtime_dir, Some(trimmed_version))?;
        load_or_create_managed_runtime_manifest_in_base(
            base_dir,
            ManagedRuntimeKind::OpenClaw,
            trimmed_version,
        )?;
        activate_managed_runtime_version_in_base(
            base_dir,
            ManagedRuntimeKind::OpenClaw,
            trimmed_version,
        )?;

        return Ok(ManagedOpenClawInstallResult {
            version: trimmed_version.to_string(),
            archive_path: archive_path.to_path_buf(),
            runtime_dir: runtime_dir.clone(),
            manifest_path: managed_runtime_manifest_path_from_base(
                base_dir,
                ManagedRuntimeKind::OpenClaw,
                trimmed_version,
            ),
            current_path: managed_runtime_current_pointer_path_from_base(
                base_dir,
                ManagedRuntimeKind::OpenClaw,
            ),
        });
    }

    let staging_dir = managed_runtime_staging_dir_from_base(base_dir, ManagedRuntimeKind::OpenClaw)
        .join(format!("{trimmed_version}-{}", Uuid::new_v4().simple()));
    ensure_dir(&staging_dir)?;

    if let Err(err) = extract_archive_to_dir(archive_path, &staging_dir)
        .and_then(|_| prepare_managed_openclaw_runtime_dir(&staging_dir, trimmed_version))
        .and_then(|_| {
            write_json(
                &staging_dir.join(MANAGED_RUNTIME_MANIFEST_FILE_NAME),
                &ManagedRuntimeManifest::new(ManagedRuntimeKind::OpenClaw, trimmed_version),
            )
        })
    {
        let _ = remove_path_if_exists(&staging_dir);
        return Err(err);
    }

    ensure_dir(&managed_runtime_versions_dir_from_base(
        base_dir,
        ManagedRuntimeKind::OpenClaw,
    ))?;

    match fs::rename(&staging_dir, &runtime_dir) {
        Ok(()) => {}
        Err(err) if runtime_dir.exists() => {
            let _ = remove_path_if_exists(&staging_dir);
            validate_managed_openclaw_runtime_dir(&runtime_dir, Some(trimmed_version))
                .map_err(|_| err.to_string())?;
        }
        Err(err) => {
            let _ = remove_path_if_exists(&staging_dir);
            return Err(err.to_string());
        }
    }

    activate_managed_runtime_version_in_base(
        base_dir,
        ManagedRuntimeKind::OpenClaw,
        trimmed_version,
    )?;

    Ok(ManagedOpenClawInstallResult {
        version: trimmed_version.to_string(),
        archive_path: archive_path.to_path_buf(),
        runtime_dir: runtime_dir.clone(),
        manifest_path: managed_runtime_manifest_path_from_base(
            base_dir,
            ManagedRuntimeKind::OpenClaw,
            trimmed_version,
        ),
        current_path: managed_runtime_current_pointer_path_from_base(
            base_dir,
            ManagedRuntimeKind::OpenClaw,
        ),
    })
}

fn install_managed_openclaw_release_in_base(
    base_dir: &Path,
    payload: &ManagedOpenClawInstallPayload,
) -> Result<ManagedOpenClawInstallResult, String> {
    let archive_path = download_managed_openclaw_archive_in_base(base_dir, &payload.version)?;
    install_managed_openclaw_archive_in_base(base_dir, &payload.version, &archive_path)
}

fn install_managed_openclaw_release(
    payload: &ManagedOpenClawInstallPayload,
) -> Result<ManagedOpenClawInstallResult, String> {
    install_managed_openclaw_release_in_base(&clawy_base_dir(), payload)
}

#[allow(dead_code)]
fn install_recommended_managed_openclaw_release() -> Result<ManagedOpenClawInstallResult, String> {
    let payload = ManagedOpenClawInstallPayload {
        version: recommended_openclaw_version()?,
    };
    install_managed_openclaw_release(&payload)
}

fn install_managed_openclaw_release_with_progress(
    app: &AppHandle,
    payload: &ManagedOpenClawInstallPayload,
) -> Result<ManagedOpenClawInstallResult, String> {
    let kind = ManagedRuntimeKind::OpenClaw;
    let version = payload.version.trim().to_string();
    if version.is_empty() {
        return Err("Managed OpenClaw version is required".into());
    }

    emit_runtime_install_progress(
        app,
        kind,
        "preparing",
        "running",
        5.0,
        Some(&version),
        Some("Preparing OpenClaw download.".into()),
        None,
        None,
    );

    let client = reqwest_client()?;
    emit_runtime_install_progress(
        app,
        kind,
        "downloading",
        "running",
        12.0,
        Some(&version),
        Some("Downloading OpenClaw package.".into()),
        None,
        None,
    );

    let mut download_progress = |progress: DownloadProgressPayload| {
        emit_runtime_install_progress(
            app,
            kind,
            "downloading",
            "running",
            runtime_stage_percent(12.0, 56.0, &progress),
            Some(&version),
            Some(format_download_progress_detail(&progress)),
            Some(progress),
            None,
        );
    };

    let archive_path = download_managed_openclaw_archive_with_client_in_base_and_registry(
        &clawy_base_dir(),
        &client,
        &version,
        OPENCLAW_NPM_REGISTRY_BASE_URL,
        Some(&mut download_progress),
    )?;

    emit_runtime_install_progress(
        app,
        kind,
        "installing",
        "running",
        82.0,
        Some(&version),
        Some("Installing OpenClaw runtime.".into()),
        None,
        None,
    );

    let result =
        install_managed_openclaw_archive_in_base(&clawy_base_dir(), &version, &archive_path)?;

    emit_runtime_install_progress(
        app,
        kind,
        "completed",
        "completed",
        100.0,
        Some(&version),
        Some("OpenClaw runtime installed.".into()),
        None,
        None,
    );

    Ok(result)
}

fn install_recommended_managed_openclaw_release_with_progress(
    app: &AppHandle,
) -> Result<ManagedOpenClawInstallResult, String> {
    let payload = ManagedOpenClawInstallPayload {
        version: recommended_openclaw_version()?,
    };
    install_managed_openclaw_release_with_progress(app, &payload)
}

fn switch_managed_node_version_in_base(
    base_dir: &Path,
    version: &str,
) -> Result<ManagedRuntimeVersionPointer, String> {
    activate_managed_runtime_version_in_base(base_dir, ManagedRuntimeKind::Node, version)
}

fn switch_managed_node_version(version: &str) -> Result<ManagedRuntimeVersionPointer, String> {
    switch_managed_node_version_in_base(&clawy_base_dir(), version)
}

fn switch_managed_openclaw_version_in_base(
    base_dir: &Path,
    version: &str,
) -> Result<ManagedRuntimeVersionPointer, String> {
    activate_managed_runtime_version_in_base(base_dir, ManagedRuntimeKind::OpenClaw, version)
}

fn switch_managed_openclaw_version(version: &str) -> Result<ManagedRuntimeVersionPointer, String> {
    switch_managed_openclaw_version_in_base(&clawy_base_dir(), version)
}

fn load_settings() -> Settings {
    let path = settings_path();

    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(settings) = serde_json::from_str::<Settings>(&content) {
            return settings;
        }

        append_log_line(
            "WARN",
            &format!(
                "Failed to parse settings file at {}; recreating defaults.",
                path.to_string_lossy()
            ),
        );
    }

    let settings = Settings::default();
    let _ = write_json(&path, &settings);
    settings
}

fn save_settings(settings: &Settings) -> Result<(), String> {
    write_json(&settings_path(), settings)?;
    if let Err(error) = sync_gateway_settings_to_openclaw(settings) {
        append_log_line(
            "WARN",
            &format!("Failed to sync gateway settings to openclaw.json: {error}"),
        );
    }
    Ok(())
}

fn load_provider_store() -> ProviderStore {
    read_json_or_default(&providers_path())
}

fn save_provider_store(store: &ProviderStore) -> Result<(), String> {
    write_json(&providers_path(), store)
}

fn current_log_file_path() -> PathBuf {
    logs_dir().join("clawy.log")
}

fn append_log_line(level: &str, message: &str) {
    let log_path = current_log_file_path();
    if let Some(parent) = log_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let line = format!("[{}] {}\n", level, message);
    let existing = fs::read_to_string(&log_path).unwrap_or_default();
    let _ = fs::write(&log_path, format!("{existing}{line}"));
}

fn read_log_tail(tail_lines: usize) -> String {
    let log_path = current_log_file_path();
    match fs::read_to_string(log_path) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let start = lines.len().saturating_sub(tail_lines);
            lines[start..].join("\n")
        }
        Err(err) => format!("(Failed to read log file: {err})"),
    }
}

fn list_log_files() -> Result<Vec<Value>, String> {
    let dir = logs_dir();
    ensure_dir(&dir)?;
    let mut items = Vec::new();
    for entry in fs::read_dir(dir).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        let metadata = entry.metadata().map_err(|err| err.to_string())?;
        if !metadata.is_file() {
            continue;
        }
        items.push(json!({
            "name": entry.file_name().to_string_lossy().to_string(),
            "path": entry.path(),
            "size": metadata.len(),
            "modified": metadata
                .modified()
                .ok()
                .and_then(|m| m.elapsed().ok())
                .map(|age| format!("{age:?}")),
        }));
    }
    Ok(items)
}

fn app_get_path(name: &str) -> String {
    match name {
        "home" => dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .to_string_lossy()
            .to_string(),
        "downloads" => dirs::download_dir()
            .unwrap_or_else(clawy_base_dir)
            .to_string_lossy()
            .to_string(),
        "desktop" => dirs::desktop_dir()
            .unwrap_or_else(clawy_base_dir)
            .to_string_lossy()
            .to_string(),
        "documents" => dirs::document_dir()
            .unwrap_or_else(clawy_base_dir)
            .to_string_lossy()
            .to_string(),
        "appData" | "userData" => clawy_base_dir().to_string_lossy().to_string(),
        _ => clawy_base_dir().to_string_lossy().to_string(),
    }
}

fn current_workspace_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn packaged_resource_candidates() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(executable) = std::env::current_exe() {
        if let Some(exe_dir) = executable.parent() {
            roots.push(exe_dir.join("resources"));
            roots.push(exe_dir.join("../Resources"));
            if let Some(parent) = exe_dir.parent() {
                roots.push(parent.join("Resources"));
                roots.push(parent.join("resources"));
            }
        }
    }

    let mut candidates = Vec::new();
    for root in roots {
        candidates.push(root.clone());
        candidates.push(root.join("_up_"));
        candidates.push(root.join("_up_").join("resources"));
        candidates.push(root.join("_up_").join("build"));
        candidates.push(root.join("_up_").join("scripts"));
    }
    candidates
}

fn packaged_resource_path(relative: &Path) -> Option<PathBuf> {
    for candidate in packaged_resource_candidates() {
        let path = candidate.join(relative);
        if path.exists() {
            return Some(path);
        }
    }
    None
}

fn env_flag_enabled(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn full_mode_runtime_enabled() -> bool {
    option_env!("CLAWY_FULL_MODE_RUNTIME")
        .map(env_flag_enabled)
        .unwrap_or(false)
        || std::env::var(FULL_MODE_RUNTIME_FLAG)
            .map(|value| env_flag_enabled(&value))
            .unwrap_or(false)
}

fn openclaw_dist_entry_exists(root: &Path) -> bool {
    root.join("dist").join("entry.js").exists() || root.join("dist").join("entry.mjs").exists()
}

fn workspace_openclaw_dir() -> PathBuf {
    current_workspace_dir()
        .join("node_modules")
        .join("openclaw")
}

fn bundled_openclaw_dir() -> PathBuf {
    if let Some(path) = packaged_resource_path(Path::new("openclaw")) {
        return path;
    }

    current_workspace_dir().join("build").join("openclaw")
}

fn inspect_openclaw_runtime_candidate(
    source: OpenClawRuntimeSource,
    dir: Option<PathBuf>,
) -> OpenClawRuntimeDiagnostic {
    let Some(dir) = dir else {
        let detail = match source {
            OpenClawRuntimeSource::Managed => "No managed OpenClaw runtime is active".to_string(),
            OpenClawRuntimeSource::NodeModules => format!(
                "No OpenClaw package found under {}",
                workspace_openclaw_dir().to_string_lossy()
            ),
            OpenClawRuntimeSource::Bundled => format!(
                "No bundled OpenClaw runtime found under {}",
                bundled_openclaw_dir().to_string_lossy()
            ),
        };
        return OpenClawRuntimeDiagnostic::missing(
            source,
            OpenClawRuntimeDiagnosticReason::NotFound,
            detail,
        );
    };

    if !dir.exists() {
        return OpenClawRuntimeDiagnostic::rejected(
            source,
            Some(dir.clone()),
            OpenClawRuntimeDiagnosticReason::PathDoesNotExist,
            format!(
                "OpenClaw runtime path does not exist: {}",
                dir.to_string_lossy()
            ),
            None,
        );
    }

    let package_path = dir.join("package.json");
    if !package_path.exists() {
        return OpenClawRuntimeDiagnostic::rejected(
            source,
            Some(dir.clone()),
            OpenClawRuntimeDiagnosticReason::MissingPackageJson,
            format!(
                "OpenClaw package metadata is missing at {}",
                package_path.to_string_lossy()
            ),
            None,
        );
    }

    let package = match read_openclaw_package_metadata(&dir) {
        Ok(package) => package,
        Err(err) => {
            return OpenClawRuntimeDiagnostic::rejected(
                source,
                Some(dir),
                OpenClawRuntimeDiagnosticReason::InvalidPackageMetadata,
                err,
                None,
            );
        }
    };

    if package.name != OPENCLAW_PACKAGE_NAME {
        return OpenClawRuntimeDiagnostic::rejected(
            source,
            Some(dir.clone()),
            OpenClawRuntimeDiagnosticReason::InvalidPackageMetadata,
            format!(
                "Resolved `{}` instead of `{OPENCLAW_PACKAGE_NAME}` at {}",
                package.name,
                dir.to_string_lossy()
            ),
            Some(package.version),
        );
    }

    let entry_path = dir.join("openclaw.mjs");
    if !entry_path.exists() {
        return OpenClawRuntimeDiagnostic::rejected(
            source,
            Some(dir.clone()),
            OpenClawRuntimeDiagnosticReason::MissingEntryScript,
            format!(
                "OpenClaw entry script not found at {}",
                entry_path.to_string_lossy()
            ),
            Some(package.version),
        );
    }

    if !openclaw_dist_entry_exists(&dir) {
        return OpenClawRuntimeDiagnostic::rejected(
            source,
            Some(dir.clone()),
            OpenClawRuntimeDiagnosticReason::MissingDistEntry,
            format!(
                "OpenClaw dist entry not found under {}",
                dir.join("dist").to_string_lossy()
            ),
            Some(package.version),
        );
    }

    OpenClawRuntimeDiagnostic::accepted(source, dir, package.version)
}

fn resolve_openclaw_runtime_with_candidates(
    managed_dir: Option<PathBuf>,
    workspace_dir: PathBuf,
    bundled_dir: PathBuf,
    prefer_bundled: bool,
) -> OpenClawRuntimeResolution {
    let mut diagnostics = Vec::new();

    let candidate_order = if prefer_bundled {
        vec![
            (OpenClawRuntimeSource::Bundled, Some(bundled_dir)),
            (OpenClawRuntimeSource::Managed, managed_dir),
            (OpenClawRuntimeSource::NodeModules, Some(workspace_dir)),
        ]
    } else {
        vec![
            (OpenClawRuntimeSource::Managed, managed_dir),
            (OpenClawRuntimeSource::NodeModules, Some(workspace_dir)),
            (OpenClawRuntimeSource::Bundled, Some(bundled_dir)),
        ]
    };

    for (source, path) in candidate_order {
        let diagnostic = inspect_openclaw_runtime_candidate(source, path);
        diagnostics.push(diagnostic.clone());
        if diagnostic.is_accepted() {
            return OpenClawRuntimeResolution::accepted(diagnostics, &diagnostic);
        }
    }

    OpenClawRuntimeResolution {
        diagnostics,
        ..OpenClawRuntimeResolution::default()
    }
}

fn resolve_openclaw_runtime() -> OpenClawRuntimeResolution {
    resolve_openclaw_runtime_with_candidates(
        managed_openclaw_dir(),
        workspace_openclaw_dir(),
        bundled_openclaw_dir(),
        full_mode_runtime_enabled(),
    )
}

fn runtime_status_payload() -> RuntimeStatusPayload {
    RuntimeStatusPayload {
        mode: if full_mode_runtime_enabled() {
            "full".into()
        } else {
            "resolver".into()
        },
        full_mode_runtime: full_mode_runtime_enabled(),
        node: resolve_node_binary(),
        openclaw: resolve_openclaw_runtime(),
    }
}

fn get_openclaw_dir() -> PathBuf {
    resolve_openclaw_runtime()
        .dir
        .unwrap_or_else(workspace_openclaw_dir)
}

fn openclaw_status() -> Value {
    let OpenClawRuntimeResolution {
        dir,
        entry_path,
        source,
        version,
        diagnostics,
    } = resolve_openclaw_runtime();
    let dir = dir.unwrap_or_else(workspace_openclaw_dir);
    let entry_path = entry_path.unwrap_or_else(|| dir.join("openclaw.mjs"));
    let package_path = dir.join("package.json");

    json!({
        "packageExists": package_path.exists(),
        "isBuilt": openclaw_dist_entry_exists(&dir),
        "entryPath": entry_path,
        "dir": dir,
        "source": source.map(OpenClawRuntimeSource::as_str),
        "version": version,
        "diagnostics": diagnostics,
    })
}

fn run_openclaw_cli_json(args: &[&str]) -> Result<Value, String> {
    let mut command = openclaw_command()?;
    command
        .args(args)
        .arg("--json")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = command
        .output()
        .map_err(|err| format!("Failed to run OpenClaw CLI: {err}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() {
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("exit code {:?}", output.status.code())
        };
        return Err(format!("OpenClaw CLI command failed: {detail}"));
    }

    serde_json::from_str::<Value>(&stdout)
        .map_err(|err| format!("Failed to parse OpenClaw CLI JSON output: {err}. Raw: {stdout}"))
}

fn openclaw_update_status() -> Result<Value, String> {
    let resolution = resolve_openclaw_runtime();
    let current_version = resolution.version.clone();
    let current_source = resolution.source.map(OpenClawRuntimeSource::as_str);
    let current_dir = resolution.dir.clone();
    let managed_version =
        managed_runtime_current_version_from_base(&clawy_base_dir(), ManagedRuntimeKind::OpenClaw);

    let update_status = run_openclaw_cli_json(&["update", "status"])?;
    let dry_run = run_openclaw_cli_json(&["update", "--dry-run", "--yes"]).ok();

    let latest_version = update_status
        .pointer("/availability/latestVersion")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| recommended_openclaw_version().ok());

    let comparison_version = managed_version
        .clone()
        .or_else(|| current_version.clone());
    let update_available = latest_version
        .as_deref()
        .zip(comparison_version.as_deref())
        .map(|(latest, current)| latest != current)
        .unwrap_or(false);

    Ok(json!({
        "success": true,
        "currentVersion": current_version,
        "currentSource": current_source,
        "currentDir": current_dir,
        "managedVersion": managed_version,
        "latestVersion": latest_version,
        "updateAvailable": update_available,
        "channel": update_status.get("channel").cloned().unwrap_or(Value::Null),
        "availability": update_status.get("availability").cloned().unwrap_or(Value::Null),
        "dryRun": dry_run.unwrap_or(Value::Null),
        "status": update_status,
        "diagnostics": resolution.diagnostics,
    }))
}

fn install_openclaw_update(
    app: &AppHandle,
    state: &BridgeState,
    payload: Option<ManagedOpenClawInstallPayload>,
) -> Result<Value, String> {
    let resolved_payload = match payload {
        Some(payload) if !payload.version.trim().is_empty() => payload,
        _ => ManagedOpenClawInstallPayload {
            version: recommended_openclaw_version()?,
        },
    };

    let result = install_managed_openclaw_release_with_progress(app, &resolved_payload)?;
    let gateway_restart = gateway_restart_internal(app, state)?;
    let latest_status = openclaw_update_status().ok();

    Ok(json!({
        "success": true,
        "result": result,
        "targetVersion": resolved_payload.version,
        "gatewayRestart": gateway_restart,
        "status": latest_status,
    }))
}

fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        return format!("{}***", &key[..key.len().min(2)]);
    }
    format!("{}***{}", &key[..4], &key[key.len() - 4..])
}

fn list_providers(store: &ProviderStore) -> Vec<ProviderWithKeyInfo> {
    store
        .providers
        .values()
        .cloned()
        .map(|config| {
            let key = store.api_keys.get(&config.id);
            ProviderWithKeyInfo {
                config,
                has_key: key.is_some(),
                key_masked: key.map(|value| mask_key(value)),
            }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthProfilesStore {
    version: u8,
    profiles: HashMap<String, Value>,
    order: HashMap<String, Vec<String>>,
    last_good: HashMap<String, String>,
}

impl Default for AuthProfilesStore {
    fn default() -> Self {
        Self {
            version: 1,
            profiles: HashMap::new(),
            order: HashMap::new(),
            last_good: HashMap::new(),
        }
    }
}

fn ensure_object(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value.as_object_mut().expect("value must be object")
}

fn openclaw_json_path() -> PathBuf {
    openclaw_config_dir().join("openclaw.json")
}

fn read_openclaw_json() -> Result<Value, String> {
    let path = openclaw_json_path();
    if !path.exists() {
        return Ok(json!({}));
    }
    let content = fs::read_to_string(path).map_err(|err| err.to_string())?;
    serde_json::from_str(&content).map_err(|err| err.to_string())
}

fn write_openclaw_json(config: &Value) -> Result<(), String> {
    write_json(&openclaw_json_path(), config)
}

fn sync_proxy_settings_to_openclaw(settings: &Settings) -> Result<(), String> {
    let mut config = read_openclaw_json()?;
    let Some(channels) = config.get_mut("channels").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    let Some(telegram_config) = channels.get_mut("telegram").and_then(Value::as_object_mut) else {
        return Ok(());
    };

    let resolved = resolve_proxy_settings(settings);
    let next_proxy = if settings.proxy_enabled {
        if !resolved.all_proxy.is_empty() {
            resolved.all_proxy
        } else if !resolved.https_proxy.is_empty() {
            resolved.https_proxy
        } else {
            resolved.http_proxy
        }
    } else {
        String::new()
    };

    let current_proxy = telegram_config
        .get("proxy")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();

    if next_proxy.is_empty() && current_proxy.is_empty() {
        return Ok(());
    }

    if next_proxy.is_empty() {
        telegram_config.remove("proxy");
    } else {
        telegram_config.insert("proxy".into(), Value::String(next_proxy.clone()));
    }

    append_log_line(
        "INFO",
        &format!(
            "Synced Telegram proxy to OpenClaw config ({})",
            if next_proxy.is_empty() {
                "disabled"
            } else {
                next_proxy.as_str()
            }
        ),
    );

    write_openclaw_json(&config)
}

fn sync_gateway_settings_to_openclaw(settings: &Settings) -> Result<(), String> {
    let mut config = read_openclaw_json()?;
    let root = ensure_object(&mut config);
    let gateway = root
        .entry("gateway")
        .or_insert_with(|| Value::Object(Map::new()));
    let gateway_obj = ensure_object(gateway);
    gateway_obj.insert("mode".into(), Value::String("local".into()));

    let auth = gateway_obj
        .entry("auth")
        .or_insert_with(|| Value::Object(Map::new()));
    let auth_obj = ensure_object(auth);
    auth_obj.insert("mode".into(), Value::String("token".into()));
    auth_obj.insert(
        "token".into(),
        Value::String(settings.gateway_token.clone()),
    );

    let browser = root
        .entry("browser")
        .or_insert_with(|| Value::Object(Map::new()));
    let browser_obj = ensure_object(browser);
    browser_obj
        .entry("enabled")
        .or_insert_with(|| Value::Bool(true));
    browser_obj
        .entry("defaultProfile")
        .or_insert_with(|| Value::String("openclaw".into()));

    write_openclaw_json(&config)?;
    sync_proxy_settings_to_openclaw(settings)
}

fn auth_profiles_path(agent_id: &str) -> PathBuf {
    openclaw_config_dir()
        .join("agents")
        .join(agent_id)
        .join("agent")
        .join("auth-profiles.json")
}

fn discover_agent_ids() -> Vec<String> {
    let agents_dir = openclaw_config_dir().join("agents");
    if !agents_dir.exists() {
        return vec!["main".into()];
    }

    let mut result = Vec::new();
    if let Ok(entries) = fs::read_dir(agents_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join("agent").exists() {
                result.push(entry.file_name().to_string_lossy().to_string());
            }
        }
    }

    if result.is_empty() {
        result.push("main".into());
    }
    result
}

fn read_auth_profiles(agent_id: &str) -> AuthProfilesStore {
    read_json_or_default(&auth_profiles_path(agent_id))
}

fn write_auth_profiles(agent_id: &str, store: &AuthProfilesStore) -> Result<(), String> {
    write_json(&auth_profiles_path(agent_id), store)
}

fn provider_default_model(provider_type: &str) -> Option<&'static str> {
    match provider_type {
        "anthropic" => Some("anthropic/claude-opus-4-6"),
        "openai" => Some("openai/gpt-5.2"),
        "google" => Some("google/gemini-3.1-pro-preview"),
        "openrouter" => Some("openrouter/anthropic/claude-opus-4.6"),
        "moonshot" => Some("moonshot/kimi-k2.5"),
        "siliconflow" => Some("siliconflow/deepseek-ai/DeepSeek-V3"),
        "minimax-portal" => Some("minimax-portal/MiniMax-M2.5"),
        "minimax-portal-cn" => Some("minimax-portal/MiniMax-M2.5"),
        "qwen-portal" => Some("qwen-portal/coder-model"),
        _ => None,
    }
}

fn provider_api(provider_type: &str) -> Option<&'static str> {
    match provider_type {
        "openai" => Some("openai-responses"),
        "openrouter" => Some("openai-completions"),
        "ark" => Some("openai-completions"),
        "moonshot" => Some("openai-completions"),
        "siliconflow" => Some("openai-completions"),
        "minimax-portal" => Some("anthropic-messages"),
        "minimax-portal-cn" => Some("anthropic-messages"),
        "qwen-portal" => Some("openai-completions"),
        "custom" => Some("openai-completions"),
        "ollama" => Some("openai-completions"),
        _ => None,
    }
}

fn provider_base_url(provider_type: &str) -> Option<&'static str> {
    match provider_type {
        "openai" => Some("https://api.openai.com/v1"),
        "openrouter" => Some("https://openrouter.ai/api/v1"),
        "ark" => Some("https://ark.cn-beijing.volces.com/api/v3"),
        "moonshot" => Some("https://api.moonshot.cn/v1"),
        "siliconflow" => Some("https://api.siliconflow.cn/v1"),
        "minimax-portal" => Some("https://api.minimax.io/anthropic"),
        "minimax-portal-cn" => Some("https://api.minimaxi.com/anthropic"),
        "qwen-portal" => Some("https://portal.qwen.ai/v1"),
        _ => None,
    }
}

fn provider_headers(provider_type: &str) -> Option<Map<String, Value>> {
    if provider_type != "openrouter" {
        return None;
    }

    let mut headers = Map::new();
    headers.insert(
        "HTTP-Referer".into(),
        Value::String("https://claw-x.com".into()),
    );
    headers.insert("X-Title".into(), Value::String("Clawy".into()));
    Some(headers)
}

fn get_openclaw_provider_key(provider_type: &str, provider_id: &str) -> String {
    if provider_type == "custom" || provider_type == "ollama" {
        return format!(
            "{}-{}",
            provider_type,
            provider_id
                .replace('-', "")
                .chars()
                .take(8)
                .collect::<String>()
        );
    }
    if provider_type == "minimax-portal-cn" {
        return "minimax-portal".into();
    }
    provider_type.to_string()
}

fn get_provider_model_ref(config: &ProviderConfig) -> Option<String> {
    let provider_key = get_openclaw_provider_key(&config.provider_type, &config.id);
    if let Some(model) = &config.model {
        if model.starts_with(&format!("{provider_key}/")) {
            return Some(model.clone());
        }
        return Some(format!("{provider_key}/{model}"));
    }
    provider_default_model(&config.provider_type).map(ToString::to_string)
}

fn get_provider_fallback_model_refs(config: &ProviderConfig, store: &ProviderStore) -> Vec<String> {
    let provider_key = get_openclaw_provider_key(&config.provider_type, &config.id);
    let mut seen = HashMap::<String, bool>::new();
    let mut result = Vec::new();

    for model in config.fallback_models.clone().unwrap_or_default() {
        let trimmed = model.trim();
        if trimmed.is_empty() {
            continue;
        }
        let model_ref = if trimmed.starts_with(&format!("{provider_key}/")) {
            trimmed.to_string()
        } else {
            format!("{provider_key}/{trimmed}")
        };
        if seen.insert(model_ref.clone(), true).is_none() {
            result.push(model_ref);
        }
    }

    for fallback_provider_id in config.fallback_provider_ids.clone().unwrap_or_default() {
        if fallback_provider_id == config.id {
            continue;
        }
        if let Some(provider) = store.providers.get(&fallback_provider_id) {
            if let Some(model_ref) = get_provider_model_ref(provider) {
                if seen.insert(model_ref.clone(), true).is_none() {
                    result.push(model_ref);
                }
            }
        }
    }

    result
}

fn save_provider_key_to_openclaw(provider_key: &str, api_key: &str) -> Result<(), String> {
    for agent_id in discover_agent_ids() {
        let mut store = read_auth_profiles(&agent_id);
        let profile_id = format!("{provider_key}:default");
        store.profiles.insert(
            profile_id.clone(),
            json!({
                "type": "api_key",
                "provider": provider_key,
                "key": api_key,
            }),
        );
        store
            .order
            .entry(provider_key.to_string())
            .or_default()
            .retain(|value| value != &profile_id);
        store
            .order
            .entry(provider_key.to_string())
            .or_default()
            .push(profile_id.clone());
        store.last_good.insert(provider_key.to_string(), profile_id);
        write_auth_profiles(&agent_id, &store)?;
    }

    Ok(())
}

fn save_oauth_token_to_openclaw(
    provider_key: &str,
    access: &str,
    refresh: &str,
    expires: u64,
) -> Result<(), String> {
    for agent_id in discover_agent_ids() {
        let mut store = read_auth_profiles(&agent_id);
        let profile_id = format!("{provider_key}:default");
        store.profiles.insert(
            profile_id.clone(),
            json!({
                "type": "oauth",
                "provider": provider_key,
                "access": access,
                "refresh": refresh,
                "expires": expires,
            }),
        );
        store
            .order
            .entry(provider_key.to_string())
            .or_default()
            .retain(|value| value != &profile_id);
        store
            .order
            .entry(provider_key.to_string())
            .or_default()
            .push(profile_id.clone());
        store.last_good.insert(provider_key.to_string(), profile_id);
        write_auth_profiles(&agent_id, &store)?;
    }

    Ok(())
}

fn ensure_oauth_plugin_enabled(config: &mut Value, provider_key: &str) {
    let plugin_key = match provider_key {
        "minimax-portal" => Some("minimax-portal-auth"),
        "qwen-portal" => Some("qwen-portal-auth"),
        _ => None,
    };

    let Some(plugin_key) = plugin_key else {
        return;
    };

    let root = ensure_object(config);
    let plugins = root
        .entry("plugins")
        .or_insert_with(|| Value::Object(Map::new()));
    let plugins_obj = ensure_object(plugins);
    let entries = plugins_obj
        .entry("entries")
        .or_insert_with(|| Value::Object(Map::new()));
    let entries_obj = ensure_object(entries);
    entries_obj.insert(plugin_key.into(), json!({ "enabled": true }));
}

fn upsert_openclaw_provider_config(
    config: &mut Value,
    provider_key: &str,
    model_refs: &[String],
    base_url: &str,
    api: &str,
    api_key: Option<&str>,
    api_key_env: Option<&str>,
    headers: Option<Map<String, Value>>,
    auth_header: Option<bool>,
) {
    let root = ensure_object(config);
    let models = root
        .entry("models")
        .or_insert_with(|| Value::Object(Map::new()));
    let models_obj = ensure_object(models);
    let providers = models_obj
        .entry("providers")
        .or_insert_with(|| Value::Object(Map::new()));
    let providers_obj = ensure_object(providers);

    let mut provider_entry = Map::new();
    provider_entry.insert("baseUrl".into(), Value::String(base_url.to_string()));
    provider_entry.insert("api".into(), Value::String(api.to_string()));
    if let Some(api_key) = api_key {
        provider_entry.insert("apiKey".into(), Value::String(api_key.to_string()));
    } else if let Some(api_key_env) = api_key_env {
        provider_entry.insert("apiKey".into(), Value::String(api_key_env.to_string()));
    }
    if let Some(headers) = headers {
        provider_entry.insert("headers".into(), Value::Object(headers));
    }
    if let Some(auth_header) = auth_header {
        provider_entry.insert("authHeader".into(), Value::Bool(auth_header));
    }

    if !model_refs.is_empty() {
        let models = model_refs
            .iter()
            .filter_map(|model_ref| {
                let model_id = model_ref
                    .strip_prefix(&format!("{provider_key}/"))
                    .unwrap_or(model_ref)
                    .trim();
                if model_id.is_empty() {
                    return None;
                }
                Some(json!({ "id": model_id, "name": model_id }))
            })
            .collect::<Vec<Value>>();
        provider_entry.insert("models".into(), Value::Array(models));
    }

    providers_obj.insert(provider_key.to_string(), Value::Object(provider_entry));
    ensure_oauth_plugin_enabled(config, provider_key);
}

fn sync_provider_config_to_openclaw(
    provider_key: &str,
    model_ref: Option<&str>,
    base_url: Option<&str>,
    api: Option<&str>,
    api_key: Option<&str>,
    headers: Option<Map<String, Value>>,
) -> Result<(), String> {
    if base_url.is_none() || api.is_none() {
        return Ok(());
    }

    let mut config = read_openclaw_json()?;
    let model_refs = model_ref
        .map(|value| vec![value.to_string()])
        .unwrap_or_default();
    upsert_openclaw_provider_config(
        &mut config,
        provider_key,
        &model_refs,
        base_url.unwrap_or_default(),
        api.unwrap_or_default(),
        api_key,
        None,
        headers,
        None,
    );
    write_openclaw_json(&config)
}

fn set_openclaw_default_model(
    _provider_key: &str,
    model_ref: &str,
    fallback_models: &[String],
) -> Result<(), String> {
    let mut config = read_openclaw_json()?;
    let root = ensure_object(&mut config);

    let agents = root
        .entry("agents")
        .or_insert_with(|| Value::Object(Map::new()));
    let agents_obj = ensure_object(agents);
    let defaults = agents_obj
        .entry("defaults")
        .or_insert_with(|| Value::Object(Map::new()));
    let defaults_obj = ensure_object(defaults);
    defaults_obj.insert(
        "model".into(),
        json!({
            "primary": model_ref,
            "fallbacks": fallback_models,
        }),
    );

    let gateway = root
        .entry("gateway")
        .or_insert_with(|| Value::Object(Map::new()));
    let gateway_obj = ensure_object(gateway);
    gateway_obj
        .entry("mode")
        .or_insert_with(|| Value::String("local".into()));

    write_openclaw_json(&config)
}

fn set_openclaw_default_model_with_override(
    provider_key: &str,
    model_ref: &str,
    fallback_models: &[String],
    base_url: Option<&str>,
    api: Option<&str>,
    api_key: Option<&str>,
    api_key_env: Option<&str>,
    headers: Option<Map<String, Value>>,
    auth_header: Option<bool>,
) -> Result<(), String> {
    let mut config = read_openclaw_json()?;
    let root = ensure_object(&mut config);

    let agents = root
        .entry("agents")
        .or_insert_with(|| Value::Object(Map::new()));
    let agents_obj = ensure_object(agents);
    let defaults = agents_obj
        .entry("defaults")
        .or_insert_with(|| Value::Object(Map::new()));
    let defaults_obj = ensure_object(defaults);
    defaults_obj.insert(
        "model".into(),
        json!({
            "primary": model_ref,
            "fallbacks": fallback_models,
        }),
    );

    let gateway = root
        .entry("gateway")
        .or_insert_with(|| Value::Object(Map::new()));
    let gateway_obj = ensure_object(gateway);
    gateway_obj
        .entry("mode")
        .or_insert_with(|| Value::String("local".into()));

    if let (Some(base_url), Some(api)) = (base_url, api) {
        let mut model_refs = vec![model_ref.to_string()];
        for fallback in fallback_models {
            if !model_refs.iter().any(|candidate| candidate == fallback) {
                model_refs.push(fallback.clone());
            }
        }
        upsert_openclaw_provider_config(
            &mut config,
            provider_key,
            &model_refs,
            base_url,
            api,
            api_key,
            api_key_env,
            headers,
            auth_header,
        );
    }

    write_openclaw_json(&config)
}

fn remove_provider_from_openclaw(provider_key: &str) -> Result<(), String> {
    for agent_id in discover_agent_ids() {
        let mut store = read_auth_profiles(&agent_id);
        let profile_id = format!("{provider_key}:default");
        store.profiles.remove(&profile_id);
        if let Some(order) = store.order.get_mut(provider_key) {
            order.retain(|value| value != &profile_id);
            if order.is_empty() {
                store.order.remove(provider_key);
            }
        }
        if store.last_good.get(provider_key) == Some(&profile_id) {
            store.last_good.remove(provider_key);
        }
        write_auth_profiles(&agent_id, &store)?;
    }

    let mut config = read_openclaw_json()?;
    if let Some(root) = config.as_object_mut() {
        if let Some(models) = root.get_mut("models") {
            if let Some(models_obj) = models.as_object_mut() {
                if let Some(providers) = models_obj.get_mut("providers") {
                    if let Some(providers_obj) = providers.as_object_mut() {
                        providers_obj.remove(provider_key);
                    }
                }
            }
        }
    }
    write_openclaw_json(&config)
}

fn maybe_restart_gateway(app: &AppHandle, state: &BridgeState) {
    if let Ok(status) = gateway_status_snapshot(state) {
        if status.state != "stopped" {
            let _ = gateway_restart_internal(app, state);
        }
    }
}

fn sync_provider_state_to_openclaw(
    store: &ProviderStore,
    config: &ProviderConfig,
    api_key: Option<&str>,
) -> Result<(), String> {
    let provider_key = get_openclaw_provider_key(&config.provider_type, &config.id);
    let resolved_api_key = api_key
        .filter(|value| !value.trim().is_empty())
        .or_else(|| store.api_keys.get(&config.id).map(String::as_str));
    let model_ref = get_provider_model_ref(config);
    let fallback_models = get_provider_fallback_model_refs(config, store);
    let base_url = config
        .base_url
        .as_deref()
        .or_else(|| provider_base_url(&config.provider_type));
    let api = provider_api(&config.provider_type);
    let headers = provider_headers(&config.provider_type);

    if let Some(api_key) = resolved_api_key {
        save_provider_key_to_openclaw(&provider_key, api_key)?;
    }

    if base_url.is_some() && api.is_some() {
        sync_provider_config_to_openclaw(
            &provider_key,
            model_ref.as_deref(),
            base_url,
            api,
            resolved_api_key,
            headers,
        )?;
    }

    if store.default_provider.as_deref() == Some(&config.id) {
        if let Some(model_ref) = model_ref {
            set_openclaw_default_model(&provider_key, &model_ref, &fallback_models)?;
            if base_url.is_some() && api.is_some() {
                sync_provider_config_to_openclaw(
                    &provider_key,
                    Some(&model_ref),
                    base_url,
                    api,
                    resolved_api_key,
                    provider_headers(&config.provider_type),
                )?;
            }
        }
    }

    Ok(())
}

fn skill_entries_mut(config: &mut Value) -> &mut Map<String, Value> {
    let root = ensure_object(config);
    let skills = root
        .entry("skills")
        .or_insert_with(|| Value::Object(Map::new()));
    let skills_obj = ensure_object(skills);
    let entries = skills_obj
        .entry("entries")
        .or_insert_with(|| Value::Object(Map::new()));
    ensure_object(entries)
}

fn get_skill_config_value(skill_key: &str) -> Result<Value, String> {
    let config = read_openclaw_json()?;
    Ok(config
        .get("skills")
        .and_then(Value::as_object)
        .and_then(|skills| skills.get("entries"))
        .and_then(Value::as_object)
        .and_then(|entries| entries.get(skill_key))
        .cloned()
        .unwrap_or(Value::Null))
}

fn get_all_skill_configs_value() -> Result<Value, String> {
    let config = read_openclaw_json()?;
    Ok(config
        .get("skills")
        .and_then(Value::as_object)
        .and_then(|skills| skills.get("entries"))
        .and_then(Value::as_object)
        .cloned()
        .map(Value::Object)
        .unwrap_or_else(|| Value::Object(Map::new())))
}

fn update_skill_config_value(params: &Value) -> Result<Value, String> {
    let payload = params
        .as_object()
        .ok_or_else(|| "skill:updateConfig expects an object payload".to_string())?;
    let skill_key = payload
        .get("skillKey")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if skill_key.is_empty() {
        return Ok(json!({ "success": false, "error": "skillKey is required" }));
    }

    let mut config = read_openclaw_json()?;
    let entries = skill_entries_mut(&mut config);
    let entry = entries
        .entry(skill_key)
        .or_insert_with(|| Value::Object(Map::new()));
    let entry_obj = ensure_object(entry);

    if let Some(api_key) = payload.get("apiKey").and_then(Value::as_str) {
        let trimmed = api_key.trim();
        if trimmed.is_empty() {
            entry_obj.remove("apiKey");
        } else {
            entry_obj.insert("apiKey".into(), Value::String(trimmed.to_string()));
        }
    }

    if let Some(env) = payload.get("env").and_then(Value::as_object) {
        let mut next_env = Map::new();
        for (key, value) in env {
            let trimmed_key = key.trim();
            let trimmed_value = value.as_str().unwrap_or_default().trim();
            if trimmed_key.is_empty() || trimmed_value.is_empty() {
                continue;
            }
            next_env.insert(
                trimmed_key.to_string(),
                Value::String(trimmed_value.to_string()),
            );
        }

        if next_env.is_empty() {
            entry_obj.remove("env");
        } else {
            entry_obj.insert("env".into(), Value::Object(next_env));
        }
    }

    write_openclaw_json(&config)?;
    Ok(json!({ "success": true }))
}

fn openclaw_command() -> Result<Command, String> {
    let node_resolution = resolve_node_binary();
    let Some(node_binary) = node_resolution.path.clone() else {
        return Err(format!(
            "No compatible Node.js runtime available: {}",
            node_resolution.failure_message()
        ));
    };

    let openclaw_resolution = resolve_openclaw_runtime();
    let Some(entry) = openclaw_resolution.entry_path.clone() else {
        return Err(format!(
            "No compatible OpenClaw runtime available: {}",
            openclaw_resolution.failure_message()
        ));
    };

    let mut command = Command::new(node_binary);
    apply_proxy_env(&mut command, &load_settings());
    command
        .arg(entry)
        .current_dir(openclaw_resolution.dir.unwrap_or_else(workspace_openclaw_dir));
    Ok(command)
}

fn run_openclaw_command(args: &[&str], label: &str) -> Result<String, String> {
    let settings = load_settings();
    let gateway_url = format!("ws://127.0.0.1:{}", settings.gateway_port);
    let mut command = openclaw_command()?;
    command
        .args(args)
        .env("OPENCLAW_GATEWAY_TOKEN", settings.gateway_token)
        .env("OPENCLAW_GATEWAY_URL", gateway_url)
        .current_dir(get_openclaw_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    run_command_capture(&mut command, label)
}

fn list_openclaw_devices() -> Result<Value, String> {
    let settings = load_settings();
    let url = format!("ws://127.0.0.1:{}", settings.gateway_port);
    let output = run_openclaw_command(
        &[
            "devices",
            "list",
            "--json",
            "--url",
            &url,
            "--token",
            &settings.gateway_token,
        ],
        "openclaw devices list",
    )?;
    serde_json::from_str::<Value>(&output)
        .map_err(|err| format!("Failed to parse `openclaw devices list --json` output: {err}"))
}

fn auto_approve_local_device_pairing() -> Result<Value, String> {
    let settings = load_settings();
    sync_gateway_settings_to_openclaw(&settings)?;
    let identity = load_or_create_device_identity()?;
    let devices = list_openclaw_devices()?;
    let pending = devices
        .get("pending")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let matching_request = pending
        .iter()
        .filter(|entry| {
            entry
                .get("deviceId")
                .and_then(Value::as_str)
                .map(|device_id| device_id == identity.device_id)
                .unwrap_or(false)
        })
        .max_by_key(|entry| entry.get("ts").and_then(Value::as_u64).unwrap_or(0))
        .cloned();

    if let Some(device) = devices
        .get("paired")
        .and_then(Value::as_array)
        .and_then(|paired| {
            paired.iter().find(|entry| {
                entry
                    .get("deviceId")
                    .and_then(Value::as_str)
                    .map(|device_id| device_id == identity.device_id)
                    .unwrap_or(false)
            })
        })
    {
        return Ok(json!({
            "success": true,
            "approved": true,
            "alreadyPaired": true,
            "device": device,
        }));
    }

    let request_id = matching_request
        .as_ref()
        .and_then(|entry| entry.get("requestId"))
        .and_then(Value::as_str)
        .ok_or_else(|| "No pending pairing request found for the local Clawy device".to_string())?;

    let output = run_openclaw_command(
        &[
            "devices",
            "approve",
            request_id,
            "--json",
            "--url",
            &format!("ws://127.0.0.1:{}", settings.gateway_port),
            "--token",
            &settings.gateway_token,
        ],
        "openclaw devices approve",
    )?;
    let approved =
        serde_json::from_str::<Value>(&output).unwrap_or_else(|_| json!({ "raw": output }));

    Ok(json!({
        "success": true,
        "approved": true,
        "requestId": request_id,
        "deviceId": identity.device_id,
        "result": approved,
    }))
}

fn split_columns(line: &str) -> Vec<String> {
    let mut columns = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = line.chars().collect();
    let mut index = 0;

    while index < chars.len() {
        if chars[index].is_whitespace() {
            let start = index;
            while index < chars.len() && chars[index].is_whitespace() {
                index += 1;
            }
            let gap = index - start;
            if gap >= 2 {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    columns.push(trimmed.to_string());
                    current.clear();
                }
                continue;
            }
            if !current.is_empty() {
                current.push(' ');
            }
            continue;
        }

        current.push(chars[index]);
        index += 1;
    }

    let trimmed = current.trim();
    if !trimmed.is_empty() {
        columns.push(trimmed.to_string());
    }

    columns
}

fn clawhub_cli_entry_path() -> PathBuf {
    let cwd = current_workspace_dir();
    if let Some(path) = packaged_resource_path(
        Path::new("clawhub")
            .join("bin")
            .join("clawdhub.js")
            .as_path(),
    ) {
        return path;
    }

    let bundled = cwd
        .join("build")
        .join("clawhub")
        .join("bin")
        .join("clawdhub.js");
    if bundled.exists() {
        return bundled;
    }

    let dev_path = cwd
        .join("node_modules")
        .join("clawhub")
        .join("bin")
        .join("clawdhub.js");
    if dev_path.exists() {
        return dev_path;
    }

    cwd.join("build")
        .join("clawhub")
        .join("bin")
        .join("clawdhub.js")
}

fn run_command_capture(command: &mut Command, label: &str) -> Result<String, String> {
    let output = command
        .output()
        .map_err(|err| format!("Failed to run {label}: {err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() {
        let message = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("{label} exited with {}", output.status)
        };
        return Err(message);
    }

    Ok(stdout)
}

fn run_clawhub_command(args: &[String]) -> Result<String, String> {
    let work_dir = openclaw_config_dir();
    ensure_dir(&work_dir)?;
    let settings = load_settings();

    let cli_entry = clawhub_cli_entry_path();
    let mut command = if cli_entry.exists() {
        let mut cmd = node_command()?;
        cmd.arg(cli_entry);
        cmd
    } else {
        return Err("ClawHub CLI entry is not available in this workspace".into());
    };

    command
        .args(args)
        .current_dir(&work_dir)
        .env("CLAWHUB_WORKDIR", &work_dir)
        .env("CI", "true")
        .env("FORCE_COLOR", "0")
        .env_remove("NODE_OPTIONS")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_proxy_env(&mut command, &settings);

    run_command_capture(&mut command, "clawhub")
}

fn parse_clawhub_list_results(output: &str) -> Vec<Value> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty() && !line.starts_with('-') && *line != "No installed skills."
        })
        .filter_map(|line| {
            let columns = split_columns(line);
            if columns.len() < 2 {
                return None;
            }
            Some(json!({
                "slug": columns[0],
                "version": columns[1].trim_start_matches('v'),
            }))
        })
        .collect()
}

fn parse_clawhub_search_results(output: &str) -> Vec<Value> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty() && !line.starts_with('-') && !line.starts_with("No skills found")
        })
        .filter_map(|line| {
            let columns = split_columns(line);
            if columns.is_empty() {
                return None;
            }
            let slug = columns[0].clone();
            let description = columns.get(1).cloned().unwrap_or_else(|| slug.clone());
            Some(json!({
                "slug": slug,
                "name": columns[0],
                "description": description,
                "version": "latest",
            }))
        })
        .collect()
}

fn parse_clawhub_explore_results(output: &str) -> Vec<Value> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('-'))
        .filter_map(|line| {
            let columns = split_columns(line);
            if columns.len() < 3 {
                return None;
            }
            let slug = columns[0].clone();
            let version = columns
                .get(1)
                .map(|value| value.trim_start_matches('v').to_string())
                .unwrap_or_else(|| "latest".into());
            let description = if columns.len() >= 4 {
                columns[3..].join(" ")
            } else {
                columns[2].clone()
            };
            Some(json!({
                "slug": slug,
                "name": columns[0],
                "description": description,
                "version": version,
            }))
        })
        .collect()
}

fn uninstall_clawhub_skill(slug: &str) -> Result<(), String> {
    let skill_dir = openclaw_skills_dir().join(slug);
    if skill_dir.exists() {
        fs::remove_dir_all(&skill_dir).map_err(|err| err.to_string())?;
    }

    let lock_path = openclaw_config_dir().join(".clawhub").join("lock.json");
    if !lock_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&lock_path).map_err(|err| err.to_string())?;
    let mut lock_json: Value = serde_json::from_str(&content).map_err(|err| err.to_string())?;
    if let Some(skills) = lock_json.get_mut("skills").and_then(Value::as_object_mut) {
        skills.remove(slug);
    }
    fs::write(
        &lock_path,
        serde_json::to_string_pretty(&lock_json).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;

    Ok(())
}

fn find_skill_readme_path(slug: &str) -> Result<PathBuf, String> {
    let skill_dir = openclaw_skills_dir().join(slug);
    let candidates = ["SKILL.md", "README.md", "skill.md", "readme.md"];

    for candidate in candidates {
        let path = skill_dir.join(candidate);
        if path.exists() {
            return Ok(path);
        }
    }

    if skill_dir.exists() {
        return Ok(skill_dir);
    }

    Err("Skill directory not found".into())
}

fn open_path_with_system(path: &Path) -> Result<(), String> {
    let mut command = if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(path);
        command
    } else if cfg!(target_os = "windows") {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", &path.to_string_lossy()]);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(path);
        command
    };

    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    command.spawn().map(|_| ()).map_err(|err| err.to_string())
}

fn open_url_with_system(url: &str) -> Result<(), String> {
    let mut command = if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(url);
        command
    } else if cfg!(target_os = "windows") {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };

    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    command.spawn().map(|_| ()).map_err(|err| err.to_string())
}

fn runner_script_path(script_name: &str) -> PathBuf {
    let cwd_path = current_workspace_dir()
        .join("scripts")
        .join("tauri")
        .join(script_name);
    if cwd_path.exists() {
        return cwd_path;
    }

    for candidate in packaged_resource_candidates() {
        let path = candidate.join("scripts").join("tauri").join(script_name);
        if path.exists() {
            return path;
        }
        let flat_path = candidate.join("tauri").join(script_name);
        if flat_path.exists() {
            return flat_path;
        }
    }

    cwd_path
}

fn emit_navigate(app: &AppHandle, path: &str) {
    let _ = app.emit("navigate", path.to_string());
}

fn with_main_window(
    app: &AppHandle,
    op: impl FnOnce(&tauri::WebviewWindow) -> Result<(), String>,
) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window is not available".to_string())?;
    op(&window)
}

fn show_main_window(app: &AppHandle) -> Result<(), String> {
    with_main_window(app, |window| {
        window.show().map_err(|err| err.to_string())?;
        window.set_focus().map_err(|err| err.to_string())?;
        Ok(())
    })
}

fn toggle_main_window(app: &AppHandle) -> Result<(), String> {
    with_main_window(app, |window| {
        let visible = window.is_visible().map_err(|err| err.to_string())?;
        if visible {
            window.hide().map_err(|err| err.to_string())?;
        } else {
            window.show().map_err(|err| err.to_string())?;
            window.set_focus().map_err(|err| err.to_string())?;
        }
        Ok(())
    })
}

fn update_tray_tooltip(app: &AppHandle, status: &str) {
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(format!("Clawy - {}", status)));
    }
}

fn read_channel_config_value(channel_type: &str) -> Result<Option<Value>, String> {
    let config = read_openclaw_json()?;
    let from_channels = config
        .get("channels")
        .and_then(Value::as_object)
        .and_then(|channels| channels.get(channel_type))
        .cloned();
    if from_channels.is_some() {
        return Ok(from_channels);
    }

    Ok(config
        .get("plugins")
        .and_then(Value::as_object)
        .and_then(|plugins| plugins.get("entries"))
        .and_then(Value::as_object)
        .and_then(|entries| entries.get(channel_type))
        .cloned())
}

fn ensure_channel_map(config: &mut Value) -> &mut Map<String, Value> {
    let root = ensure_object(config);
    let channels = root
        .entry("channels")
        .or_insert_with(|| Value::Object(Map::new()));
    ensure_object(channels)
}

fn ensure_plugin_entries_map(config: &mut Value) -> &mut Map<String, Value> {
    let root = ensure_object(config);
    let plugins = root
        .entry("plugins")
        .or_insert_with(|| Value::Object(Map::new()));
    let plugins_obj = ensure_object(plugins);
    let entries = plugins_obj
        .entry("entries")
        .or_insert_with(|| Value::Object(Map::new()));
    ensure_object(entries)
}

fn ensure_dingtalk_plugin_installed() -> Result<(bool, Option<String>), String> {
    let target_dir = openclaw_config_dir().join("extensions").join("dingtalk");
    let target_manifest = target_dir.join("openclaw.plugin.json");
    if target_manifest.exists() {
        return Ok((true, None));
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut candidate_sources = Vec::new();
    if let Some(packaged) =
        packaged_resource_path(Path::new("openclaw-plugins").join("dingtalk").as_path())
    {
        candidate_sources.push(packaged);
    }
    candidate_sources.extend([
        cwd.join("build").join("openclaw-plugins").join("dingtalk"),
        cwd.join("openclaw-plugins").join("dingtalk"),
    ]);
    let source_dir = candidate_sources
        .into_iter()
        .find(|dir| dir.join("openclaw.plugin.json").exists());

    let Some(source_dir) = source_dir else {
        return Ok((
            false,
            Some("Bundled DingTalk plugin mirror not found in this workspace.".into()),
        ));
    };

    let extensions_dir = openclaw_config_dir().join("extensions");
    ensure_dir(&extensions_dir)?;
    let _ = fs::remove_dir_all(&target_dir);
    copy_dir_recursive(&source_dir, &target_dir)?;

    if target_manifest.exists() {
        Ok((true, None))
    } else {
        Ok((
            false,
            Some("Failed to install bundled DingTalk plugin mirror.".into()),
        ))
    }
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<(), String> {
    ensure_dir(destination)?;
    for entry in fs::read_dir(source).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        let file_type = entry.file_type().map_err(|err| err.to_string())?;
        let src_path = entry.path();
        let dst_path = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            if let Some(parent) = dst_path.parent() {
                ensure_dir(parent)?;
            }
            fs::copy(&src_path, &dst_path).map_err(|err| err.to_string())?;
        }
    }
    Ok(())
}

fn transform_channel_config(
    channel_type: &str,
    raw: &Map<String, Value>,
    existing: Option<&Value>,
) -> Map<String, Value> {
    let mut transformed = Map::new();

    match channel_type {
        "discord" => {
            for (key, value) in raw {
                if key != "guildId" && key != "channelId" {
                    transformed.insert(key.clone(), value.clone());
                }
            }

            transformed.insert("groupPolicy".into(), Value::String("allowlist".into()));
            transformed.insert("dm".into(), json!({ "enabled": false }));
            transformed.insert(
                "retry".into(),
                json!({
                    "attempts": 3,
                    "minDelayMs": 500,
                    "maxDelayMs": 30000,
                    "jitter": 0.1,
                }),
            );

            let guild_id = raw
                .get("guildId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            let channel_id = raw
                .get("channelId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();

            if !guild_id.is_empty() {
                let channels_value = if !channel_id.is_empty() {
                    json!({
                        channel_id: { "allow": true, "requireMention": true }
                    })
                } else {
                    json!({
                        "*": { "allow": true, "requireMention": true }
                    })
                };

                transformed.insert(
                    "guilds".into(),
                    json!({
                        guild_id: {
                            "users": ["*"],
                            "requireMention": true,
                            "channels": channels_value,
                        }
                    }),
                );
            }
        }
        "telegram" => {
            for (key, value) in raw {
                if key != "allowedUsers" {
                    transformed.insert(key.clone(), value.clone());
                }
            }

            let allowed_users = raw
                .get("allowedUsers")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let values: Vec<Value> = allowed_users
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| Value::String(value.to_string()))
                .collect();
            if !values.is_empty() {
                transformed.insert("allowFrom".into(), Value::Array(values));
            }
        }
        "feishu" => {
            transformed.extend(raw.clone());

            let existing_obj = existing.and_then(Value::as_object);
            let dm_policy = raw
                .get("dmPolicy")
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| {
                    existing_obj
                        .and_then(|value| value.get("dmPolicy"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })
                .unwrap_or_else(|| "open".into());
            transformed.insert("dmPolicy".into(), Value::String(dm_policy.clone()));

            let mut allow_from = raw
                .get("allowFrom")
                .and_then(Value::as_array)
                .cloned()
                .or_else(|| {
                    existing_obj
                        .and_then(|value| value.get("allowFrom"))
                        .and_then(Value::as_array)
                        .cloned()
                })
                .unwrap_or_else(|| vec![Value::String("*".into())]);

            if dm_policy == "open"
                && !allow_from
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|value| value == "*")
            {
                allow_from.push(Value::String("*".into()));
            }
            transformed.insert("allowFrom".into(), Value::Array(allow_from));
        }
        _ => {
            transformed.extend(raw.clone());
        }
    }

    transformed
}

fn save_channel_config_value(
    channel_type: &str,
    raw_config: &Map<String, Value>,
) -> Result<Value, String> {
    let mut config = read_openclaw_json()?;
    let existing = read_channel_config_value(channel_type)?;

    let mut plugin_installed = None;
    let mut warning = None;
    if channel_type == "dingtalk" {
        let (installed, install_warning) = ensure_dingtalk_plugin_installed()?;
        if !installed {
            return Ok(json!({
                "success": false,
                "error": install_warning.unwrap_or_else(|| "DingTalk plugin install failed".into()),
            }));
        }
        plugin_installed = Some(installed);
        warning = install_warning;

        let root = ensure_object(&mut config);
        let plugins = root
            .entry("plugins")
            .or_insert_with(|| Value::Object(Map::new()));
        let plugins_obj = ensure_object(plugins);
        plugins_obj.insert("enabled".into(), Value::Bool(true));

        let allow = plugins_obj
            .entry("allow")
            .or_insert_with(|| Value::Array(Vec::new()));
        if !allow.is_array() {
            *allow = Value::Array(Vec::new());
        }
        let allow_array = allow.as_array_mut().expect("allow must be array");
        if !allow_array
            .iter()
            .filter_map(Value::as_str)
            .any(|value| value == "dingtalk")
        {
            allow_array.push(Value::String("dingtalk".into()));
        }
    }

    let transformed = transform_channel_config(channel_type, raw_config, existing.as_ref());
    let enabled = transformed
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    {
        let channels = ensure_channel_map(&mut config);
        let current = channels
            .entry(channel_type.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        let current_obj = ensure_object(current);
        merge_objects(current_obj, &transformed);
        current_obj.insert("enabled".into(), Value::Bool(enabled));
    }

    if channel_type == "whatsapp" {
        let plugins = ensure_plugin_entries_map(&mut config);
        let current = plugins
            .entry(channel_type.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        let current_obj = ensure_object(current);
        current_obj.insert("enabled".into(), Value::Bool(enabled));
    }

    write_openclaw_json(&config)?;

    let mut response = Map::new();
    response.insert("success".into(), Value::Bool(true));
    if let Some(installed) = plugin_installed {
        response.insert("pluginInstalled".into(), Value::Bool(installed));
    }
    if let Some(warning) = warning {
        response.insert("warning".into(), Value::String(warning));
    }
    Ok(Value::Object(response))
}

fn get_channel_form_values_value(channel_type: &str) -> Result<Value, String> {
    let saved = read_channel_config_value(channel_type)?;
    let Some(saved) = saved else {
        return Ok(Value::Null);
    };
    let saved_obj = saved
        .as_object()
        .ok_or_else(|| "Channel config is not an object".to_string())?;

    let mut values = Map::new();
    if channel_type == "discord" {
        if let Some(token) = saved_obj.get("token").and_then(Value::as_str) {
            values.insert("token".into(), Value::String(token.to_string()));
        }
        if let Some(guilds) = saved_obj.get("guilds").and_then(Value::as_object) {
            if let Some((guild_id, guild_config)) = guilds.iter().next() {
                values.insert("guildId".into(), Value::String(guild_id.to_string()));
                if let Some(channels) = guild_config.get("channels").and_then(Value::as_object) {
                    if let Some((channel_id, _)) =
                        channels.iter().find(|(id, _)| id.as_str() != "*")
                    {
                        values.insert("channelId".into(), Value::String(channel_id.to_string()));
                    }
                }
            }
        }
    } else if channel_type == "telegram" {
        if let Some(allow_from) = saved_obj.get("allowFrom").and_then(Value::as_array) {
            let users = allow_from
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<&str>>()
                .join(", ");
            if !users.is_empty() {
                values.insert("allowedUsers".into(), Value::String(users));
            }
        }
        for (key, value) in saved_obj {
            if key == "enabled" {
                continue;
            }
            if let Some(text) = value.as_str() {
                values.insert(key.clone(), Value::String(text.to_string()));
            }
        }
    } else {
        for (key, value) in saved_obj {
            if key == "enabled" {
                continue;
            }
            if let Some(text) = value.as_str() {
                values.insert(key.clone(), Value::String(text.to_string()));
            }
        }
    }

    if values.is_empty() {
        Ok(Value::Null)
    } else {
        Ok(Value::Object(values))
    }
}

fn delete_channel_config_value(channel_type: &str) -> Result<(), String> {
    let mut config = read_openclaw_json()?;

    if let Some(channels) = config.get_mut("channels").and_then(Value::as_object_mut) {
        channels.remove(channel_type);
    }

    if let Some(entries) = config
        .get_mut("plugins")
        .and_then(Value::as_object_mut)
        .and_then(|plugins| plugins.get_mut("entries"))
        .and_then(Value::as_object_mut)
    {
        entries.remove(channel_type);
    }

    if channel_type == "whatsapp" {
        let credentials_dir = openclaw_config_dir().join("credentials").join("whatsapp");
        if credentials_dir.exists() {
            let _ = fs::remove_dir_all(credentials_dir);
        }
    }

    write_openclaw_json(&config)
}

fn list_configured_channels_value() -> Result<Vec<String>, String> {
    let config = read_openclaw_json()?;
    let mut channels = Vec::new();

    if let Some(configured) = config.get("channels").and_then(Value::as_object) {
        for (channel_type, value) in configured {
            let enabled = value
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            if enabled {
                channels.push(channel_type.clone());
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
            if enabled && !channels.iter().any(|item| item == channel_type) {
                channels.push(channel_type.clone());
            }
        }
    }

    let whatsapp_dir = openclaw_config_dir().join("credentials").join("whatsapp");
    if whatsapp_dir.exists() {
        let has_session = fs::read_dir(&whatsapp_dir)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .any(|entry| entry.path().is_dir())
            })
            .unwrap_or(false);
        if has_session && !channels.iter().any(|item| item == "whatsapp") {
            channels.push("whatsapp".into());
        }
    }

    Ok(channels)
}

fn set_channel_enabled_value(channel_type: &str, enabled: bool) -> Result<(), String> {
    let mut config = read_openclaw_json()?;

    {
        let channels = ensure_channel_map(&mut config);
        let current = channels
            .entry(channel_type.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        let current_obj = ensure_object(current);
        current_obj.insert("enabled".into(), Value::Bool(enabled));
    }

    if channel_type == "whatsapp" {
        let entries = ensure_plugin_entries_map(&mut config);
        let current = entries
            .entry(channel_type.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        let current_obj = ensure_object(current);
        current_obj.insert("enabled".into(), Value::Bool(enabled));
    }

    write_openclaw_json(&config)
}

fn run_openclaw_doctor() -> Result<String, String> {
    let mut command = openclaw_command()?;
    command
        .arg("doctor")
        .arg("--non-interactive")
        .current_dir(get_openclaw_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    run_command_capture(&mut command, "openclaw doctor")
}

fn validate_channel_config_value(channel_type: &str) -> Result<Value, String> {
    let mut valid = true;
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    if let Ok(output) = run_openclaw_doctor() {
        for line in output.lines() {
            let trimmed = line.trim();
            let lower = trimmed.to_lowercase();
            if lower.contains(channel_type) && lower.contains("error") {
                errors.push(trimmed.to_string());
                valid = false;
            } else if lower.contains(channel_type) && lower.contains("warning") {
                warnings.push(trimmed.to_string());
            } else if lower.contains("unrecognized key") && lower.contains(channel_type) {
                errors.push(trimmed.to_string());
                valid = false;
            }
        }
    }

    let saved = read_channel_config_value(channel_type)?;
    let Some(saved) = saved else {
        return Ok(json!({
            "valid": false,
            "errors": [format!("Channel {channel_type} is not configured")],
            "warnings": warnings,
        }));
    };

    let saved_obj = saved.as_object().cloned().unwrap_or_default();
    let enabled = saved_obj
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if !enabled {
        warnings.push(format!("Channel {channel_type} is disabled"));
    }

    if channel_type == "discord"
        && saved_obj
            .get("token")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .is_empty()
    {
        errors.push("Discord: Bot token is required".into());
        valid = false;
    }

    if channel_type == "telegram" {
        if saved_obj
            .get("botToken")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            errors.push("Telegram: Bot token is required".into());
            valid = false;
        }

        let allow_from_len = saved_obj
            .get("allowFrom")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        if allow_from_len == 0 {
            errors.push("Telegram: Allowed User IDs are required".into());
            valid = false;
        }
    }

    if errors.is_empty() {
        valid = true;
    }

    Ok(json!({
        "valid": valid,
        "errors": errors,
        "warnings": warnings,
    }))
}

fn reqwest_client_with_timeout(timeout: Duration) -> Result<reqwest::blocking::Client, String> {
    let settings = load_settings();
    let mut builder = reqwest::blocking::Client::builder().timeout(timeout);

    if !settings.proxy_enabled {
        return builder.no_proxy().build().map_err(|err| err.to_string());
    }

    let resolved = resolve_proxy_settings(&settings);
    let no_proxy = NoProxy::from_string(
        &resolved
            .bypass_rules
            .split([',', '\n', ';'])
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join(","),
    );

    if !resolved.all_proxy.is_empty() {
        builder = builder.proxy(
            Proxy::all(&resolved.all_proxy)
                .map_err(|err| err.to_string())?
                .no_proxy(no_proxy.clone()),
        );
    } else {
        if !resolved.http_proxy.is_empty() {
            builder = builder.proxy(
                Proxy::http(&resolved.http_proxy)
                    .map_err(|err| err.to_string())?
                    .no_proxy(no_proxy.clone()),
            );
        }
        if !resolved.https_proxy.is_empty() {
            builder = builder.proxy(
                Proxy::https(&resolved.https_proxy)
                    .map_err(|err| err.to_string())?
                    .no_proxy(no_proxy.clone()),
            );
        }
    }

    builder.build().map_err(|err| err.to_string())
}

fn reqwest_client() -> Result<reqwest::blocking::Client, String> {
    reqwest_client_with_timeout(Duration::from_secs(15))
}

fn update_channel_directory(channel: &str) -> &'static str {
    match channel {
        "beta" => "beta",
        "dev" => "alpha",
        _ => "latest",
    }
}

fn update_platform_download_url(manifest: &ReleaseManifest) -> Option<String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => manifest
            .downloads
            .mac
            .as_ref()
            .and_then(|downloads| downloads.arm64.clone().or_else(|| downloads.x64.clone())),
        ("macos", _) => manifest
            .downloads
            .mac
            .as_ref()
            .and_then(|downloads| downloads.x64.clone().or_else(|| downloads.arm64.clone())),
        ("windows", "aarch64") => manifest
            .downloads
            .win
            .as_ref()
            .and_then(|downloads| downloads.arm64.clone().or_else(|| downloads.x64.clone())),
        ("windows", _) => manifest
            .downloads
            .win
            .as_ref()
            .and_then(|downloads| downloads.x64.clone().or_else(|| downloads.arm64.clone())),
        ("linux", "aarch64") => manifest.downloads.linux.as_ref().and_then(|downloads| {
            downloads
                .appimage_arm64
                .clone()
                .or_else(|| downloads.deb_arm64.clone())
                .or_else(|| downloads.rpm_x64.clone())
        }),
        ("linux", _) => manifest.downloads.linux.as_ref().and_then(|downloads| {
            downloads
                .appimage_x64
                .clone()
                .or_else(|| downloads.deb_amd64.clone())
                .or_else(|| downloads.rpm_x64.clone())
        }),
        _ => None,
    }
}

fn guess_file_name_from_url(url: &str, version: &str) -> String {
    url.rsplit('/')
        .next()
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("Clawy-{version}-update"))
}

fn fetch_release_notes(changelog_url: Option<&str>) -> Option<String> {
    let changelog_url = changelog_url?;
    let tag = changelog_url
        .split("/releases/tag/")
        .nth(1)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    let client = reqwest_client_with_timeout(Duration::from_secs(10)).ok()?;
    let response = client
        .get(format!(
            "https://api.github.com/repos/edwardZhang/Clawy/releases/tags/{tag}"
        ))
        .header("User-Agent", "Clawy-Tauri-Updater")
        .send()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }

    response
        .json::<Value>()
        .ok()
        .and_then(|body| {
            body.get("body")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .filter(|value| !value.trim().is_empty())
}

fn normalize_version(value: &str) -> Option<Version> {
    Version::parse(value.trim_start_matches('v')).ok()
}

fn is_remote_version_newer(current_version: &str, remote_version: &str) -> bool {
    match (
        normalize_version(current_version),
        normalize_version(remote_version),
    ) {
        (Some(current), Some(remote)) => remote > current,
        _ => remote_version != current_version,
    }
}

fn update_status_snapshot(state: &BridgeState) -> Result<UpdateStatusPayload, String> {
    state
        .updater_status
        .lock()
        .map_err(|_| "Updater status lock poisoned".to_string())
        .map(|guard| guard.clone())
}

fn set_update_status(
    app: &AppHandle,
    state: &BridgeState,
    mut update: impl FnMut(&mut UpdateStatusPayload),
) -> Result<UpdateStatusPayload, String> {
    let snapshot = {
        let mut guard = state
            .updater_status
            .lock()
            .map_err(|_| "Updater status lock poisoned".to_string())?;
        update(&mut guard);
        guard.clone()
    };

    let _ = app.emit("update:status-changed", &snapshot);
    Ok(snapshot)
}

fn release_to_download_target(
    manifest: ReleaseManifest,
    requested_channel: &str,
) -> Result<DownloadTarget, String> {
    let download_url = update_platform_download_url(&manifest)
        .ok_or_else(|| "No update artifact is available for this platform.".to_string())?;
    let file_name = guess_file_name_from_url(&download_url, &manifest.version);
    let release_notes = fetch_release_notes(manifest.changelog.as_deref()).or_else(|| {
        manifest
            .changelog
            .clone()
            .map(|url| format!("Release notes: {url}"))
    });

    Ok(DownloadTarget {
        version: manifest.version,
        channel: manifest
            .channel
            .unwrap_or_else(|| update_channel_directory(requested_channel).to_string()),
        release_date: manifest.release_date,
        release_notes,
        download_url,
        file_name,
    })
}

fn start_auto_install_countdown(app: &AppHandle, state: &BridgeState) {
    let generation = {
        let mut runtime = match state.updater_runtime.lock() {
            Ok(runtime) => runtime,
            Err(_) => return,
        };
        runtime.auto_install_generation = runtime.auto_install_generation.saturating_add(1);
        runtime.auto_install_generation
    };

    let app_handle = app.clone();
    let state_handle = state.clone();
    std::thread::spawn(move || {
        let mut seconds = 5_i64;
        while seconds >= 0 {
            let current_generation = match state_handle.updater_runtime.lock() {
                Ok(runtime) => runtime.auto_install_generation,
                Err(_) => return,
            };
            if current_generation != generation {
                return;
            }

            let _ = app_handle.emit(
                "update:auto-install-countdown",
                json!({ "seconds": seconds }),
            );

            if seconds == 0 {
                let path = match state_handle.updater_runtime.lock() {
                    Ok(runtime) => runtime.downloaded_file.clone(),
                    Err(_) => None,
                };
                if let Some(path) = path {
                    let _ = open_path_with_system(&path);
                }
                return;
            }

            seconds -= 1;
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

fn cancel_auto_install_countdown(app: &AppHandle, state: &BridgeState) {
    if let Ok(mut runtime) = state.updater_runtime.lock() {
        runtime.auto_install_generation = runtime.auto_install_generation.saturating_add(1);
    }
    let _ = app.emit(
        "update:auto-install-countdown",
        json!({ "seconds": -1, "cancelled": true }),
    );
}

fn update_check_internal(
    app: &AppHandle,
    state: &BridgeState,
) -> Result<UpdateStatusPayload, String> {
    let settings = load_settings();
    let channel = settings.update_channel.clone();

    set_update_status(app, state, |status| {
        status.status = "checking".into();
        status.error = None;
        status.progress = None;
    })?;

    let manifest_url = format!(
        "https://oss.intelli-spectrum.com/{}/release-info.json",
        update_channel_directory(&channel)
    );
    let client = reqwest_client()?;
    let response = client
        .get(&manifest_url)
        .send()
        .map_err(|err| format!("Failed to fetch update manifest: {err}"))?;
    if !response.status().is_success() {
        return set_update_status(app, state, |status| {
            status.status = "error".into();
            status.error = Some(format!(
                "Update manifest request failed with status {}",
                response.status()
            ));
            status.progress = None;
        });
    }

    let manifest = response
        .json::<ReleaseManifest>()
        .map_err(|err| format!("Failed to parse update manifest: {err}"))?;
    let target = release_to_download_target(manifest, &channel)?;
    let info = UpdateInfoPayload {
        version: target.version.clone(),
        release_date: target.release_date.clone(),
        release_notes: target.release_notes.clone(),
        channel: Some(target.channel.clone()),
        download_url: Some(target.download_url.clone()),
    };

    {
        let mut runtime = state
            .updater_runtime
            .lock()
            .map_err(|_| "Updater runtime lock poisoned".to_string())?;
        runtime.download_target = Some(target.clone());
        runtime.downloaded_file = None;
        runtime.is_downloading = false;
    }

    if is_remote_version_newer(&app.package_info().version.to_string(), &target.version) {
        let snapshot = set_update_status(app, state, |status| {
            status.status = "available".into();
            status.info = Some(info.clone());
            status.progress = None;
            status.error = None;
        })?;

        if settings.auto_download_update {
            let app_handle = app.clone();
            let state_handle = state.clone();
            std::thread::spawn(move || {
                let _ = download_update_internal(&app_handle, &state_handle);
            });
        }

        Ok(snapshot)
    } else {
        set_update_status(app, state, |status| {
            status.status = "not-available".into();
            status.info = Some(info.clone());
            status.progress = None;
            status.error = None;
        })
    }
}

fn download_update_internal(app: &AppHandle, state: &BridgeState) -> Result<(), String> {
    let target = {
        let mut runtime = state
            .updater_runtime
            .lock()
            .map_err(|_| "Updater runtime lock poisoned".to_string())?;
        if runtime.is_downloading {
            return Ok(());
        }
        let target = runtime.download_target.clone().ok_or_else(|| {
            "No update metadata is available. Run update:check first.".to_string()
        })?;
        runtime.is_downloading = true;
        target
    };

    let info = UpdateInfoPayload {
        version: target.version.clone(),
        release_date: target.release_date.clone(),
        release_notes: target.release_notes.clone(),
        channel: Some(target.channel.clone()),
        download_url: Some(target.download_url.clone()),
    };

    set_update_status(app, state, |status| {
        status.status = "downloading".into();
        status.info = Some(info.clone());
        status.progress = Some(DownloadProgressPayload {
            total: 0,
            delta: 0,
            transferred: 0,
            percent: 0.0,
            bytes_per_second: 0,
        });
        status.error = None;
    })?;

    let updates_dir = clawy_base_dir().join("updates");
    ensure_dir(&updates_dir)?;
    let destination = updates_dir.join(&target.file_name);
    let client = reqwest_client()?;
    let mut response = client
        .get(&target.download_url)
        .send()
        .map_err(|err| format!("Failed to download update: {err}"))?;
    if !response.status().is_success() {
        if let Ok(mut runtime) = state.updater_runtime.lock() {
            runtime.is_downloading = false;
        }
        return Err(format!(
            "Update download failed with status {}",
            response.status()
        ));
    }

    let total = response.content_length().unwrap_or(0);
    let mut file = fs::File::create(&destination).map_err(|err| err.to_string())?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut transferred = 0_u64;
    let started_at = SystemTime::now();

    loop {
        let read = response.read(&mut buffer).map_err(|err| err.to_string())?;
        if read == 0 {
            break;
        }

        file.write_all(&buffer[..read])
            .map_err(|err| err.to_string())?;
        transferred = transferred.saturating_add(read as u64);
        let elapsed_ms = started_at
            .elapsed()
            .unwrap_or_else(|_| Duration::from_millis(1))
            .as_millis()
            .max(1) as u64;
        let bytes_per_second = transferred.saturating_mul(1000) / elapsed_ms;
        let percent = if total > 0 {
            (transferred as f64 / total as f64) * 100.0
        } else {
            0.0
        };
        let progress = DownloadProgressPayload {
            total,
            delta: read as u64,
            transferred,
            percent,
            bytes_per_second,
        };
        let _ = set_update_status(app, state, |status| {
            status.status = "downloading".into();
            status.info = Some(info.clone());
            status.progress = Some(progress.clone());
            status.error = None;
        });
    }

    if let Ok(mut runtime) = state.updater_runtime.lock() {
        runtime.is_downloading = false;
        runtime.downloaded_file = Some(destination.clone());
    }

    set_update_status(app, state, |status| {
        status.status = "downloaded".into();
        status.info = Some(info.clone());
        status.progress = None;
        status.error = None;
    })?;

    if load_settings().auto_download_update {
        start_auto_install_countdown(app, state);
    }

    Ok(())
}

fn uv_target_dir_name() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    };
    format!("{os}-{arch}")
}

fn bundled_binary_path(binary_name: &str) -> PathBuf {
    if let Some(packaged_path) = packaged_resource_path(
        Path::new("bin")
            .join(uv_target_dir_name())
            .join(binary_name)
            .as_path(),
    ) {
        return packaged_path;
    }

    let workspace_path = current_workspace_dir()
        .join("resources")
        .join("bin")
        .join(uv_target_dir_name())
        .join(binary_name);
    if workspace_path.exists() {
        return workspace_path;
    }

    workspace_path
}

fn bundled_uv_path() -> PathBuf {
    let binary_name = if cfg!(windows) { "uv.exe" } else { "uv" };
    bundled_binary_path(binary_name)
}

fn bundled_node_path() -> PathBuf {
    let binary_name = if cfg!(windows) { "node.exe" } else { "node" };
    bundled_binary_path(binary_name)
}

fn find_command_path(command: &str) -> Option<PathBuf> {
    let command_path = Path::new(command);
    if command_path.components().count() > 1 || command_path.is_absolute() {
        return command_path.is_file().then(|| command_path.to_path_buf());
    }

    let search_path = std::env::var_os("PATH")?;
    let candidate_suffixes = if cfg!(windows) && command_path.extension().is_none() {
        std::env::var("PATHEXT")
            .ok()
            .map(|value| {
                value
                    .split(';')
                    .filter(|suffix| !suffix.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .filter(|suffixes| !suffixes.is_empty())
            .unwrap_or_else(|| vec![".exe".into(), ".cmd".into(), ".bat".into(), ".com".into()])
    } else {
        vec![String::new()]
    };

    for directory in std::env::split_paths(&search_path) {
        for suffix in &candidate_suffixes {
            let candidate = if suffix.is_empty() {
                directory.join(command)
            } else {
                directory.join(format!("{command}{suffix}"))
            };

            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

fn resolve_uv_binary() -> (PathBuf, &'static str) {
    let bundled = bundled_uv_path();
    if bundled.exists() {
        return (bundled, "bundled");
    }
    if let Some(path) = find_command_path("uv") {
        return (path, "path");
    }
    (bundled, "missing")
}

fn supported_node_version_req() -> VersionReq {
    VersionReq::parse(SUPPORTED_NODE_VERSION_RANGE).expect("supported Node.js range is valid")
}

fn resolve_node_binary_version(path: &Path) -> Result<String, String> {
    let mut command = Command::new(path);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let raw_version = run_command_capture(&mut command, "node --version")?;
    let normalized = raw_version.trim().trim_start_matches('v');
    let parsed = Version::parse(normalized)
        .map_err(|err| format!("Unable to parse Node.js version `{raw_version}`: {err}"))?;

    Ok(parsed.to_string())
}

fn run_node_smoke_test(path: &Path) -> Result<(), String> {
    let mut command = Command::new(path);
    command
        .args(["-e", NODE_SMOKE_TEST_SCRIPT])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = run_command_capture(&mut command, "node smoke test")?;
    if output == NODE_SMOKE_TEST_OUTPUT {
        return Ok(());
    }

    Err(format!(
        "Node.js smoke test returned `{output}` instead of `{NODE_SMOKE_TEST_OUTPUT}`"
    ))
}

fn inspect_node_binary_candidate(
    source: NodeBinarySource,
    path: Option<PathBuf>,
) -> NodeBinaryDiagnostic {
    let Some(path) = path else {
        let detail = match source {
            NodeBinarySource::Path => "Node.js was not found in PATH",
            NodeBinarySource::Managed => {
                "Managed Node.js is not installed or no current version is selected"
            }
            NodeBinarySource::Bundled => "Bundled Node.js binary is unavailable",
        };
        return NodeBinaryDiagnostic::missing(
            source,
            NodeBinaryDiagnosticReason::NotFoundInPath,
            detail,
        );
    };

    if !path.exists() {
        return NodeBinaryDiagnostic::rejected(
            source,
            Some(path),
            NodeBinaryDiagnosticReason::PathDoesNotExist,
            "Node.js candidate path does not exist",
            None,
        );
    }

    let version = match resolve_node_binary_version(&path) {
        Ok(version) => version,
        Err(err) => {
            let reason = if err.contains("Unable to parse Node.js version") {
                NodeBinaryDiagnosticReason::InvalidVersion
            } else {
                NodeBinaryDiagnosticReason::VersionCommandFailed
            };
            return NodeBinaryDiagnostic::rejected(source, Some(path), reason, err, None);
        }
    };

    if !supported_node_version_req()
        .matches(&Version::parse(&version).expect("validated Node.js version should parse"))
    {
        return NodeBinaryDiagnostic::rejected(
            source,
            Some(path),
            NodeBinaryDiagnosticReason::UnsupportedVersion,
            format!(
                "Detected Node.js {version}, but supported range is {SUPPORTED_NODE_VERSION_RANGE}"
            ),
            Some(version),
        );
    }

    if let Err(err) = run_node_smoke_test(&path) {
        return NodeBinaryDiagnostic::rejected(
            source,
            Some(path),
            NodeBinaryDiagnosticReason::SmokeTestFailed,
            err,
            Some(version),
        );
    }

    NodeBinaryDiagnostic::accepted(source, path, version)
}

#[cfg(test)]
fn resolve_node_binary_with_candidates(
    system_path: Option<PathBuf>,
    bundled_path: PathBuf,
) -> NodeBinaryResolution {
    resolve_node_binary_with_candidates_and_preference(system_path, bundled_path, false)
}

#[cfg(test)]
fn resolve_node_binary_with_candidates_and_preference(
    system_path: Option<PathBuf>,
    bundled_path: PathBuf,
    prefer_bundled: bool,
) -> NodeBinaryResolution {
    let mut diagnostics = Vec::new();

    let candidate_order = if prefer_bundled {
        vec![
            (NodeBinarySource::Bundled, Some(bundled_path)),
            (NodeBinarySource::Path, system_path),
        ]
    } else {
        vec![
            (NodeBinarySource::Path, system_path),
            (NodeBinarySource::Bundled, Some(bundled_path)),
        ]
    };

    for (source, path) in candidate_order {
        let diagnostic = inspect_node_binary_candidate(source, path);
        diagnostics.push(diagnostic.clone());
        if diagnostic.is_accepted() {
            return NodeBinaryResolution::accepted(diagnostics, &diagnostic);
        }
    }

    NodeBinaryResolution {
        diagnostics,
        ..NodeBinaryResolution::default()
    }
}

#[cfg(test)]
fn resolve_node_binary_with_candidates_and_managed(
    system_path: Option<PathBuf>,
    managed_path: Option<PathBuf>,
    bundled_path: PathBuf,
) -> NodeBinaryResolution {
    resolve_node_binary_with_candidates_and_managed_and_preference(
        system_path,
        managed_path,
        bundled_path,
        false,
    )
}

fn resolve_node_binary_with_candidates_and_managed_and_preference(
    system_path: Option<PathBuf>,
    managed_path: Option<PathBuf>,
    bundled_path: PathBuf,
    prefer_bundled: bool,
) -> NodeBinaryResolution {
    let mut diagnostics = Vec::new();

    let candidate_order = if prefer_bundled {
        vec![
            (NodeBinarySource::Bundled, Some(bundled_path)),
            (NodeBinarySource::Path, system_path),
            (NodeBinarySource::Managed, managed_path),
        ]
    } else {
        vec![
            (NodeBinarySource::Path, system_path),
            (NodeBinarySource::Managed, managed_path),
            (NodeBinarySource::Bundled, Some(bundled_path)),
        ]
    };

    for (source, path) in candidate_order {
        let diagnostic = inspect_node_binary_candidate(source, path);
        diagnostics.push(diagnostic.clone());
        if diagnostic.is_accepted() {
            return NodeBinaryResolution::accepted(diagnostics, &diagnostic);
        }
    }

    NodeBinaryResolution {
        diagnostics,
        ..NodeBinaryResolution::default()
    }
}

fn resolve_node_binary() -> NodeBinaryResolution {
    resolve_node_binary_with_candidates_and_managed_and_preference(
        find_command_path("node"),
        managed_node_binary_path(),
        bundled_node_path(),
        full_mode_runtime_enabled(),
    )
}

fn node_command() -> Result<Command, String> {
    let resolution = resolve_node_binary();
    let Some(node_binary) = resolution.path else {
        return Err(format!(
            "No compatible Node.js runtime available: {}",
            resolution.failure_message()
        ));
    };

    let mut command = Command::new(node_binary);
    apply_proxy_env(&mut command, &load_settings());
    Ok(command)
}

fn should_optimize_network() -> bool {
    let timezone = std::env::var("TZ").unwrap_or_default();
    let locale = std::env::var("LANG").unwrap_or_default();
    if timezone.contains("Asia/Shanghai") || locale.starts_with("zh_CN") {
        return true;
    }

    reqwest_client_with_timeout(Duration::from_secs(2))
        .ok()
        .and_then(|client| {
            client
                .get("https://www.google.com/generate_204")
                .send()
                .ok()
        })
        .map(|response| !response.status().is_success())
        .unwrap_or(true)
}

fn uv_install_all() -> Result<Value, String> {
    let (uv_binary, source) = resolve_uv_binary();
    if source == "missing" || (!uv_binary.exists() && uv_binary != PathBuf::from("uv")) {
        return Ok(json!({
            "success": false,
            "error": format!(
                "uv not found in system PATH and bundled binary missing at {}",
                uv_binary.to_string_lossy()
            ),
        }));
    }

    let mut command = Command::new(&uv_binary);
    command
        .args(["python", "install", "3.12"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_proxy_env(&mut command, &load_settings());

    if should_optimize_network() {
        command.env(
            "UV_PYTHON_INSTALL_MIRROR",
            "https://registry.npmmirror.com/-/binary/python-build-standalone/",
        );
        command.env("UV_INDEX_URL", "https://pypi.tuna.tsinghua.edu.cn/simple/");
    }

    let output = command.output().map_err(|err| {
        format!(
            "Failed to launch uv from {}: {err}",
            uv_binary.to_string_lossy()
        )
    })?;

    if output.status.success() {
        return Ok(json!({ "success": true }));
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("uv exited with {}", output.status)
    };
    Ok(json!({
        "success": false,
        "error": format!("Python installation failed [{source}]: {detail}"),
    }))
}

fn kill_child_process(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn oauth_provider_name(provider_type: &str) -> String {
    match provider_type {
        "minimax-portal" => "MiniMax (Global)".into(),
        "minimax-portal-cn" => "MiniMax (CN)".into(),
        "qwen-portal" => "Qwen".into(),
        other => other.to_string(),
    }
}

fn normalize_oauth_base_url(provider_type: &str, resource_url: Option<&str>) -> String {
    let default_base_url = match provider_type {
        "minimax-portal" => "https://api.minimax.io/anthropic",
        "minimax-portal-cn" => "https://api.minimaxi.com/anthropic",
        "qwen-portal" => "https://portal.qwen.ai/v1",
        _ => "https://portal.qwen.ai/v1",
    };

    let mut base_url = resource_url
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(default_base_url)
        .trim()
        .to_string();
    if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
        base_url = format!("https://{base_url}");
    }

    if provider_type.starts_with("minimax-portal") {
        return format!(
            "{}/anthropic",
            base_url
                .trim_end_matches('/')
                .trim_end_matches("/anthropic")
                .trim_end_matches("/v1")
        );
    }

    if provider_type == "qwen-portal" && !base_url.ends_with("/v1") {
        return format!("{}/v1", base_url.trim_end_matches('/'));
    }

    base_url
}

fn persist_oauth_provider_success(
    app: &AppHandle,
    state: &BridgeState,
    provider_type: &str,
    token: &Value,
) -> Result<(), String> {
    let access = token
        .get("access")
        .and_then(Value::as_str)
        .ok_or_else(|| "OAuth access token is missing".to_string())?;
    let refresh = token
        .get("refresh")
        .and_then(Value::as_str)
        .ok_or_else(|| "OAuth refresh token is missing".to_string())?;
    let expires = token
        .get("expires")
        .and_then(Value::as_u64)
        .ok_or_else(|| "OAuth expiry is missing".to_string())?;
    let api = token
        .get("api")
        .and_then(Value::as_str)
        .unwrap_or_else(|| match provider_type {
            "qwen-portal" => "openai-completions",
            _ => "anthropic-messages",
        });
    let resource_url = token.get("resourceUrl").and_then(Value::as_str);
    let base_url = normalize_oauth_base_url(provider_type, resource_url);
    let token_provider_key = if provider_type.starts_with("minimax-portal") {
        "minimax-portal"
    } else {
        provider_type
    };
    let api_key_env = if token_provider_key == "minimax-portal" {
        "minimax-oauth"
    } else {
        "qwen-oauth"
    };
    let auth_header = if token_provider_key == "minimax-portal" {
        Some(true)
    } else {
        None
    };

    save_oauth_token_to_openclaw(token_provider_key, access, refresh, expires)?;

    let mut store = load_provider_store();
    let existing = store.providers.get(provider_type).cloned();
    let updated_at = now_iso_string();
    let created_at = existing
        .as_ref()
        .map(|config| config.created_at.clone())
        .unwrap_or_else(|| updated_at.clone());

    let config = ProviderConfig {
        id: provider_type.to_string(),
        name: oauth_provider_name(provider_type),
        provider_type: provider_type.to_string(),
        base_url: Some(base_url.clone()),
        model: existing
            .as_ref()
            .and_then(|config| config.model.clone())
            .or_else(|| provider_default_model(provider_type).map(ToString::to_string)),
        fallback_models: existing
            .as_ref()
            .and_then(|config| config.fallback_models.clone()),
        fallback_provider_ids: existing
            .as_ref()
            .and_then(|config| config.fallback_provider_ids.clone()),
        enabled: existing
            .as_ref()
            .map(|config| config.enabled)
            .unwrap_or(true),
        created_at,
        updated_at,
    };

    store
        .providers
        .insert(provider_type.to_string(), config.clone());
    save_provider_store(&store)?;

    let model_ref = get_provider_model_ref(&config)
        .ok_or_else(|| format!("No default model configured for {provider_type}"))?;
    let fallback_models = get_provider_fallback_model_refs(&config, &store);
    set_openclaw_default_model_with_override(
        token_provider_key,
        &model_ref,
        &fallback_models,
        Some(&base_url),
        Some(api),
        None,
        Some(api_key_env),
        None,
        auth_header,
    )?;

    maybe_restart_gateway(app, state);
    let _ = app.emit(
        "oauth:success",
        json!({ "provider": provider_type, "success": true }),
    );
    Ok(())
}

fn handle_oauth_runner_stdout(
    app: AppHandle,
    state: BridgeState,
    provider: String,
    reader: impl std::io::Read + Send + 'static,
) {
    std::thread::spawn(move || {
        let buffered = BufReader::new(reader);
        for line in buffered.lines().map_while(Result::ok) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(message) = serde_json::from_str::<Value>(trimmed) else {
                append_log_line("INFO", &format!("[oauth-runner] {trimmed}"));
                continue;
            };

            match message
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
            {
                "open-url" => {
                    if let Some(url) = message.get("url").and_then(Value::as_str) {
                        let _ = open_url_with_system(url);
                    }
                }
                "code" => {
                    let payload = json!({
                        "provider": provider.as_str(),
                        "verificationUri": message.get("verificationUri").cloned().unwrap_or(Value::Null),
                        "userCode": message.get("userCode").cloned().unwrap_or(Value::Null),
                        "expiresIn": message.get("expiresIn").cloned().unwrap_or(json!(300)),
                    });
                    let _ = app.emit("oauth:code", payload);
                }
                "success" => {
                    let resolved_provider = message
                        .get("provider")
                        .and_then(Value::as_str)
                        .unwrap_or(&provider)
                        .to_string();
                    let token = message.get("token").cloned().unwrap_or(Value::Null);
                    if let Err(error) =
                        persist_oauth_provider_success(&app, &state, &resolved_provider, &token)
                    {
                        let _ = app.emit("oauth:error", json!({ "message": error }));
                    }
                }
                "error" => {
                    let message = message
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("OAuth flow failed")
                        .to_string();
                    let _ = app.emit("oauth:error", json!({ "message": message }));
                }
                other => {
                    append_log_line(
                        "INFO",
                        &format!("[oauth-runner:{}] unhandled event {other}", provider),
                    );
                }
            }
        }
    });
}

fn start_oauth_flow(
    app: &AppHandle,
    state: &BridgeState,
    provider_type: &str,
) -> Result<Value, String> {
    cancel_oauth_flow(state)?;

    let script_path = runner_script_path("oauth-runner.mjs");
    if !script_path.exists() {
        return Ok(json!({
            "success": false,
            "error": format!("OAuth runner script not found at {}", script_path.to_string_lossy()),
        }));
    }

    let mut command = node_command()?;
    command
        .arg(&script_path)
        .arg(provider_type)
        .current_dir(current_workspace_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if provider_type == "minimax-portal-cn" {
        command.arg("cn");
    }

    let mut child = command
        .spawn()
        .map_err(|err| format!("Failed to launch OAuth runner: {err}"))?;
    if let Some(stdout) = child.stdout.take() {
        handle_oauth_runner_stdout(
            app.clone(),
            state.clone(),
            provider_type.to_string(),
            stdout,
        );
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_log_thread(stderr, "WARN");
    }

    let mut runtime = state
        .oauth_runtime
        .lock()
        .map_err(|_| "OAuth runtime lock poisoned".to_string())?;
    runtime.child = Some(child);
    runtime.provider = Some(provider_type.to_string());
    Ok(json!({ "success": true }))
}

fn cancel_oauth_flow(state: &BridgeState) -> Result<(), String> {
    let mut runtime = state
        .oauth_runtime
        .lock()
        .map_err(|_| "OAuth runtime lock poisoned".to_string())?;
    if let Some(child) = runtime.child.as_mut() {
        kill_child_process(child);
    }
    runtime.child = None;
    runtime.provider = None;
    Ok(())
}

fn handle_whatsapp_runner_stdout(app: AppHandle, reader: impl std::io::Read + Send + 'static) {
    std::thread::spawn(move || {
        let buffered = BufReader::new(reader);
        for line in buffered.lines().map_while(Result::ok) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(message) = serde_json::from_str::<Value>(trimmed) else {
                append_log_line("INFO", &format!("[whatsapp-runner] {trimmed}"));
                continue;
            };

            match message
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
            {
                "qr" => {
                    let _ = app.emit(
                        "channel:whatsapp-qr",
                        json!({
                            "qr": message.get("qr").cloned().unwrap_or(Value::Null),
                            "raw": message.get("raw").cloned().unwrap_or(Value::Null),
                        }),
                    );
                }
                "success" => {
                    let _ = app.emit(
                        "channel:whatsapp-success",
                        json!({
                            "accountId": message.get("accountId").cloned().unwrap_or(Value::Null),
                        }),
                    );
                }
                "error" => {
                    let error = message
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("WhatsApp login failed")
                        .to_string();
                    let _ = app.emit("channel:whatsapp-error", error);
                }
                other => {
                    append_log_line(
                        "INFO",
                        &format!("[whatsapp-runner] unhandled event {other}"),
                    );
                }
            }
        }
    });
}

fn request_whatsapp_qr(
    app: &AppHandle,
    state: &BridgeState,
    account_id: &str,
) -> Result<Value, String> {
    cancel_whatsapp_qr(state)?;

    let script_path = runner_script_path("whatsapp-runner.mjs");
    if !script_path.exists() {
        return Ok(json!({
            "success": false,
            "error": format!("WhatsApp runner script not found at {}", script_path.to_string_lossy()),
        }));
    }

    let mut child = node_command()?
        .arg(&script_path)
        .arg(account_id)
        .current_dir(current_workspace_dir())
        .env("OPENCLAW_DIR", get_openclaw_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("Failed to launch WhatsApp runner: {err}"))?;
    if let Some(stdout) = child.stdout.take() {
        handle_whatsapp_runner_stdout(app.clone(), stdout);
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_log_thread(stderr, "WARN");
    }

    let mut runtime = state
        .whatsapp_runtime
        .lock()
        .map_err(|_| "WhatsApp runtime lock poisoned".to_string())?;
    runtime.child = Some(child);
    runtime.account_id = Some(account_id.to_string());
    Ok(json!({ "success": true }))
}

fn cancel_whatsapp_qr(state: &BridgeState) -> Result<(), String> {
    let mut runtime = state
        .whatsapp_runtime
        .lock()
        .map_err(|_| "WhatsApp runtime lock poisoned".to_string())?;
    if let Some(child) = runtime.child.as_mut() {
        kill_child_process(child);
    }
    runtime.child = None;
    runtime.account_id = None;
    Ok(())
}

fn start_oauth_monitor(state: BridgeState) {
    std::thread::spawn(move || loop {
        if let Ok(mut runtime) = state.oauth_runtime.lock() {
            let exited = runtime
                .child
                .as_mut()
                .and_then(|child| child.try_wait().ok().flatten())
                .is_some();
            if exited {
                runtime.child = None;
                runtime.provider = None;
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    });
}

fn start_whatsapp_monitor(state: BridgeState) {
    std::thread::spawn(move || loop {
        if let Ok(mut runtime) = state.whatsapp_runtime.lock() {
            let exited = runtime
                .child
                .as_mut()
                .and_then(|child| child.try_wait().ok().flatten())
                .is_some();
            if exited {
                runtime.child = None;
                runtime.account_id = None;
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    });
}

fn parse_usage_entries_from_jsonl(
    content: &str,
    session_id: &str,
    agent_id: &str,
    limit: Option<usize>,
) -> Vec<Value> {
    let max_entries = limit.unwrap_or(usize::MAX);
    let mut entries = Vec::new();
    let lines = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<&str>>();

    for line in lines.iter().rev() {
        if entries.len() >= max_entries {
            break;
        }
        let Ok(parsed) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let timestamp = parsed.get("timestamp").and_then(Value::as_str);
        let message = parsed.get("message").and_then(Value::as_object);
        let usage = message
            .and_then(|message| message.get("usage"))
            .and_then(Value::as_object);
        let role = message
            .and_then(|message| message.get("role"))
            .and_then(Value::as_str);
        if timestamp.is_none() || role != Some("assistant") || usage.is_none() {
            continue;
        }

        let usage = usage.expect("usage checked above");
        let input_tokens = usage
            .get("input")
            .and_then(Value::as_u64)
            .or_else(|| usage.get("promptTokens").and_then(Value::as_u64))
            .unwrap_or(0);
        let output_tokens = usage
            .get("output")
            .and_then(Value::as_u64)
            .or_else(|| usage.get("completionTokens").and_then(Value::as_u64))
            .unwrap_or(0);
        let cache_read_tokens = usage.get("cacheRead").and_then(Value::as_u64).unwrap_or(0);
        let cache_write_tokens = usage.get("cacheWrite").and_then(Value::as_u64).unwrap_or(0);
        let total_tokens = usage
            .get("total")
            .and_then(Value::as_u64)
            .or_else(|| usage.get("totalTokens").and_then(Value::as_u64))
            .unwrap_or(input_tokens + output_tokens + cache_read_tokens + cache_write_tokens);
        let cost_usd = usage
            .get("cost")
            .and_then(Value::as_object)
            .and_then(|cost| cost.get("total"))
            .and_then(Value::as_f64);

        if total_tokens == 0 && cost_usd.unwrap_or(0.0) <= 0.0 {
            continue;
        }

        entries.push(json!({
            "timestamp": timestamp.unwrap_or_default(),
            "sessionId": session_id,
            "agentId": agent_id,
            "model": message.and_then(|message| message.get("model")).and_then(Value::as_str)
                .or_else(|| message.and_then(|message| message.get("modelRef")).and_then(Value::as_str)),
            "provider": message.and_then(|message| message.get("provider")).and_then(Value::as_str),
            "inputTokens": input_tokens,
            "outputTokens": output_tokens,
            "cacheReadTokens": cache_read_tokens,
            "cacheWriteTokens": cache_write_tokens,
            "totalTokens": total_tokens,
            "costUsd": cost_usd,
        }));
    }

    entries
}

fn recent_token_usage_history(limit: Option<usize>) -> Result<Vec<Value>, String> {
    let max_entries = limit.unwrap_or(usize::MAX);
    let mut files = Vec::<(PathBuf, String, String, u128)>::new();
    let agents_dir = openclaw_config_dir().join("agents");

    if let Ok(agent_entries) = fs::read_dir(&agents_dir) {
        for agent_entry in agent_entries.flatten() {
            let agent_path = agent_entry.path();
            if !agent_path.is_dir() {
                continue;
            }
            let agent_id = agent_entry.file_name().to_string_lossy().to_string();
            let sessions_dir = agent_path.join("sessions");
            let Ok(session_entries) = fs::read_dir(&sessions_dir) else {
                continue;
            };
            for session_entry in session_entries.flatten() {
                let path = session_entry.path();
                let file_name = session_entry.file_name().to_string_lossy().to_string();
                if !file_name.ends_with(".jsonl") || file_name.contains(".deleted.") {
                    continue;
                }
                let Ok(metadata) = session_entry.metadata() else {
                    continue;
                };
                let modified = metadata
                    .modified()
                    .ok()
                    .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
                    .map(|value| value.as_millis())
                    .unwrap_or(0);
                files.push((
                    path,
                    file_name.trim_end_matches(".jsonl").to_string(),
                    agent_id.clone(),
                    modified,
                ));
            }
        }
    }

    files.sort_by(|left, right| right.3.cmp(&left.3));

    let mut results = Vec::new();
    for (path, session_id, agent_id, _) in files {
        if results.len() >= max_entries {
            break;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let remaining = max_entries.saturating_sub(results.len());
        results.extend(parse_usage_entries_from_jsonl(
            &content,
            &session_id,
            &agent_id,
            Some(remaining),
        ));
    }

    results.sort_by(|left, right| {
        let left_ts = left
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let right_ts = right
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or_default();
        right_ts.cmp(left_ts)
    });

    Ok(if limit.is_some() {
        results.into_iter().take(max_entries).collect()
    } else {
        results
    })
}

fn create_application_menu(app: &AppHandle) -> Result<(), String> {
    let file_menu = SubmenuBuilder::new(app, "File")
        .text("menu-new-chat", "New Chat")
        .separator()
        .text("menu-settings", "Settings")
        .separator()
        .quit()
        .build()
        .map_err(|err| err.to_string())?;

    let edit_menu = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .separator()
        .select_all()
        .build()
        .map_err(|err| err.to_string())?;

    let view_menu = SubmenuBuilder::new(app, "View")
        .text("menu-reload", "Reload")
        .text("menu-devtools", "Toggle DevTools")
        .separator()
        .fullscreen()
        .build()
        .map_err(|err| err.to_string())?;

    let navigate_menu = SubmenuBuilder::new(app, "Navigate")
        .text("menu-dashboard", "Dashboard")
        .text("menu-chat", "Chat")
        .text("menu-channels", "Channels")
        .text("menu-skills", "Skills")
        .text("menu-cron", "Cron Tasks")
        .text("menu-settings", "Settings")
        .build()
        .map_err(|err| err.to_string())?;

    let window_menu = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .close_window()
        .build()
        .map_err(|err| err.to_string())?;

    let help_menu = SubmenuBuilder::new(app, "Help")
        .text("menu-docs", "Documentation")
        .text("menu-issues", "Report Issue")
        .separator()
        .text("menu-openclaw-docs", "OpenClaw Documentation")
        .text("menu-check-updates", "Check for Updates...")
        .build()
        .map_err(|err| err.to_string())?;

    let mut builder = MenuBuilder::new(app);
    if cfg!(target_os = "macos") {
        let app_menu = SubmenuBuilder::new(app, app.package_info().name.clone())
            .about(None)
            .separator()
            .text("menu-settings", "Preferences...")
            .separator()
            .services()
            .separator()
            .hide()
            .hide_others()
            .separator()
            .quit()
            .build()
            .map_err(|err| err.to_string())?;
        builder = builder
            .item(&app_menu)
            .item(&file_menu)
            .item(&edit_menu)
            .item(&view_menu)
            .item(&navigate_menu)
            .item(&window_menu)
            .item(&help_menu);
    } else {
        builder = builder
            .item(&file_menu)
            .item(&edit_menu)
            .item(&view_menu)
            .item(&navigate_menu)
            .item(&window_menu)
            .item(&help_menu);
    }

    let menu = builder.build().map_err(|err| err.to_string())?;
    app.set_menu(menu)
        .map(|_| ())
        .map_err(|err| err.to_string())
}

fn create_tray_icon(app: &AppHandle) -> Result<(), String> {
    let tray_menu = MenuBuilder::new(app)
        .text("tray-show", "Show Clawy")
        .separator()
        .text("tray-dashboard", "Open Dashboard")
        .text("tray-chat", "Open Chat")
        .text("tray-settings", "Open Settings")
        .separator()
        .text("tray-check-updates", "Check for Updates...")
        .separator()
        .text("tray-quit", "Quit Clawy")
        .build()
        .map_err(|err| err.to_string())?;

    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&tray_menu)
        .tooltip("Clawy - starting")
        .show_menu_on_left_click(false);
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    builder
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = toggle_main_window(tray.app_handle());
            }
        })
        .build(app)
        .map_err(|err| err.to_string())?;

    update_tray_tooltip(app, "starting");
    Ok(())
}

fn handle_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        "menu-new-chat" | "tray-chat" => {
            let _ = show_main_window(app);
            emit_navigate(app, "/");
        }
        "menu-dashboard" | "tray-dashboard" => {
            let _ = show_main_window(app);
            emit_navigate(app, "/dashboard");
        }
        "menu-channels" => {
            let _ = show_main_window(app);
            emit_navigate(app, "/channels");
        }
        "menu-skills" => {
            let _ = show_main_window(app);
            emit_navigate(app, "/skills");
        }
        "menu-cron" => {
            let _ = show_main_window(app);
            emit_navigate(app, "/cron");
        }
        "menu-settings" | "tray-settings" => {
            let _ = show_main_window(app);
            emit_navigate(app, "/settings");
        }
        "menu-reload" => {
            let _ = with_main_window(app, |window| window.reload().map_err(|err| err.to_string()));
        }
        "menu-devtools" => {
            #[cfg(debug_assertions)]
            {
                let _ = with_main_window(app, |window| {
                    if window.is_devtools_open() {
                        window.close_devtools();
                    } else {
                        window.open_devtools();
                    }
                    Ok(())
                });
            }
        }
        "menu-docs" => {
            let _ = open_url_with_system("https://claw-x.com");
        }
        "menu-issues" => {
            let _ = open_url_with_system("https://github.com/edwardZhang/Clawy/issues");
        }
        "menu-openclaw-docs" => {
            let _ = open_url_with_system("https://docs.openclaw.ai");
        }
        "menu-check-updates" | "tray-check-updates" => {
            let app_handle = app.clone();
            let state = app.state::<BridgeState>().inner().clone();
            std::thread::spawn(move || {
                if let Err(error) = update_check_internal(&app_handle, &state) {
                    let _ = set_update_status(&app_handle, &state, |status| {
                        status.status = "error".into();
                        status.error = Some(error.clone());
                        status.progress = None;
                    });
                }
            });
        }
        "tray-show" => {
            let _ = show_main_window(app);
        }
        "tray-quit" => app.exit(0),
        _ => {}
    }
}

fn validate_discord_credentials(config: &Map<String, Value>) -> Result<Value, String> {
    let token = config
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if token.is_empty() {
        return Ok(json!({ "valid": false, "errors": ["Bot token is required"], "warnings": [] }));
    }

    let client = reqwest_client()?;
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut details = Map::new();

    let me_response = client
        .get("https://discord.com/api/v10/users/@me")
        .header("Authorization", format!("Bot {token}"))
        .send()
        .map_err(|err| err.to_string())?;
    if !me_response.status().is_success() {
        let status = me_response.status();
        if me_response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Ok(
                json!({ "valid": false, "errors": ["Invalid bot token. Please check and try again."], "warnings": [] }),
            );
        }
        let body: Value = me_response.json().unwrap_or_else(|_| json!({}));
        let message = body
            .get("message")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("Discord API error: {status}"));
        return Ok(json!({ "valid": false, "errors": [message], "warnings": [] }));
    }

    let me_data: Value = me_response.json().unwrap_or_else(|_| json!({}));
    if !me_data.get("bot").and_then(Value::as_bool).unwrap_or(false) {
        return Ok(json!({
            "valid": false,
            "errors": ["The provided token belongs to a user account, not a bot. Please use a bot token."],
            "warnings": [],
        }));
    }
    if let Some(username) = me_data.get("username").and_then(Value::as_str) {
        details.insert("botUsername".into(), Value::String(username.to_string()));
    }
    if let Some(bot_id) = me_data.get("id").and_then(Value::as_str) {
        details.insert("botId".into(), Value::String(bot_id.to_string()));
    }

    let guild_id = config
        .get("guildId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if !guild_id.is_empty() {
        let guild_response = client
            .get(format!("https://discord.com/api/v10/guilds/{guild_id}"))
            .header("Authorization", format!("Bot {token}"))
            .send();
        match guild_response {
            Ok(response) if response.status().is_success() => {
                let body: Value = response.json().unwrap_or_else(|_| json!({}));
                if let Some(name) = body.get("name").and_then(Value::as_str) {
                    details.insert("guildName".into(), Value::String(name.to_string()));
                }
            }
            Ok(response)
                if response.status() == reqwest::StatusCode::FORBIDDEN
                    || response.status() == reqwest::StatusCode::NOT_FOUND =>
            {
                errors.push(format!(
                    "Cannot access guild (server) with ID \"{guild_id}\". Make sure the bot has been invited to this server."
                ));
            }
            Ok(response) => {
                errors.push(format!(
                    "Failed to verify guild ID: Discord API returned {}",
                    response.status()
                ));
            }
            Err(err) => {
                warnings.push(format!("Could not verify guild ID: {err}"));
            }
        }
    }

    let channel_id = config
        .get("channelId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if !channel_id.is_empty() {
        let channel_response = client
            .get(format!("https://discord.com/api/v10/channels/{channel_id}"))
            .header("Authorization", format!("Bot {token}"))
            .send();
        match channel_response {
            Ok(response) if response.status().is_success() => {
                let body: Value = response.json().unwrap_or_else(|_| json!({}));
                if let Some(name) = body.get("name").and_then(Value::as_str) {
                    details.insert("channelName".into(), Value::String(name.to_string()));
                }
                if !guild_id.is_empty() {
                    let response_guild_id = body
                        .get("guild_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if !response_guild_id.is_empty() && response_guild_id != guild_id {
                        errors.push(format!(
                            "Channel \"{}\" does not belong to the specified guild. It belongs to a different server.",
                            body.get("name").and_then(Value::as_str).unwrap_or("Unknown")
                        ));
                    }
                }
            }
            Ok(response)
                if response.status() == reqwest::StatusCode::FORBIDDEN
                    || response.status() == reqwest::StatusCode::NOT_FOUND =>
            {
                errors.push(format!(
                    "Cannot access channel with ID \"{channel_id}\". Make sure the bot has permission to view this channel."
                ));
            }
            Ok(response) => {
                errors.push(format!(
                    "Failed to verify channel ID: Discord API returned {}",
                    response.status()
                ));
            }
            Err(err) => {
                warnings.push(format!("Could not verify channel ID: {err}"));
            }
        }
    }

    Ok(json!({
        "valid": errors.is_empty(),
        "errors": errors,
        "warnings": warnings,
        "details": details,
    }))
}

fn validate_telegram_credentials(config: &Map<String, Value>) -> Result<Value, String> {
    let bot_token = config
        .get("botToken")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let allowed_users = config
        .get("allowedUsers")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();

    if bot_token.is_empty() {
        return Ok(json!({ "valid": false, "errors": ["Bot token is required"], "warnings": [] }));
    }
    if allowed_users.is_empty() {
        return Ok(
            json!({ "valid": false, "errors": ["At least one allowed user ID is required"], "warnings": [] }),
        );
    }

    let client = reqwest_client()?;
    let response = client
        .get(format!("https://api.telegram.org/bot{bot_token}/getMe"))
        .send()
        .map_err(|err| err.to_string())?;
    let body: Value = response.json().unwrap_or_else(|_| json!({}));
    if body.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return Ok(json!({
            "valid": true,
            "errors": [],
            "warnings": [],
            "details": {
                "botUsername": body
                    .get("result")
                    .and_then(Value::as_object)
                    .and_then(|result| result.get("username"))
                    .and_then(Value::as_str)
                    .unwrap_or("Unknown")
            }
        }));
    }

    Ok(json!({
        "valid": false,
        "errors": [body.get("description").and_then(Value::as_str).unwrap_or("Invalid bot token")],
        "warnings": [],
    }))
}

fn validate_channel_credentials_value(
    channel_type: &str,
    config: &Map<String, Value>,
) -> Result<Value, String> {
    match channel_type {
        "discord" => validate_discord_credentials(config),
        "telegram" => validate_telegram_credentials(config),
        _ => Ok(json!({
            "valid": true,
            "errors": [],
            "warnings": ["No online validation available for this channel type."],
        })),
    }
}

fn merge_objects(current: &mut Map<String, Value>, patch: &Map<String, Value>) {
    for (key, value) in patch {
        current.insert(key.clone(), value.clone());
    }
}

fn ext_for_mime_type(mime_type: &str) -> &'static str {
    match mime_type {
        "image/png" => ".png",
        "image/jpeg" => ".jpg",
        "image/gif" => ".gif",
        "image/webp" => ".webp",
        "application/pdf" => ".pdf",
        "text/plain" => ".txt",
        "text/markdown" => ".md",
        _ => "",
    }
}

fn file_name_from_path(path: &Path) -> String {
    path.file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into())
}

fn gateway_status_snapshot(state: &BridgeState) -> Result<GatewayStatus, String> {
    let mut status = state
        .gateway_status
        .lock()
        .map_err(|_| "Gateway status lock poisoned".to_string())?
        .clone();

    let runtime = state
        .gateway_runtime
        .lock()
        .map_err(|_| "Gateway runtime lock poisoned".to_string())?;

    if let Some(started_at_ms) = runtime.started_at_ms {
        status.uptime = Some(now_ms().saturating_sub(started_at_ms));
    } else {
        status.uptime = None;
    }

    if runtime.child.is_none() && status.state != "stopped" && status.state != "error" {
        status.state = "stopped".into();
        status.pid = None;
        status.connected_at = None;
        status.version = None;
    }

    Ok(status)
}

fn set_gateway_status(
    app: &AppHandle,
    state: &BridgeState,
    mut update: impl FnMut(&mut GatewayStatus),
) -> Result<GatewayStatus, String> {
    let snapshot = {
        let mut guard = state
            .gateway_status
            .lock()
            .map_err(|_| "Gateway status lock poisoned".to_string())?;
        update(&mut guard);
        guard.clone()
    };

    let _ = app.emit("gateway:status-changed", &snapshot);
    update_tray_tooltip(app, &snapshot.state);
    Ok(snapshot)
}

fn emit_gateway_error(app: &AppHandle, message: &str) {
    let _ = app.emit("gateway:error", message.to_string());
}

fn spawn_log_thread<R>(reader: R, level: &'static str)
where
    R: std::io::Read + Send + 'static,
{
    std::thread::spawn(move || {
        let buffered = BufReader::new(reader);
        for line in buffered.lines().map_while(Result::ok) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            append_log_line(level, trimmed);
        }
    });
}

fn start_gateway_monitor(app: AppHandle, state: BridgeState) {
    std::thread::spawn(move || loop {
        let exit_state = {
            let mut runtime = match state.gateway_runtime.lock() {
                Ok(runtime) => runtime,
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(500));
                    continue;
                }
            };

            let desired_running = runtime.desired_running;
            let mut clear_child = false;
            let mut exit_info: Option<(Option<i32>, bool, Option<String>)> = None;

            if let Some(child) = runtime.child.as_mut() {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        clear_child = true;
                        exit_info = Some((status.code(), !desired_running, None));
                    }
                    Ok(None) => {}
                    Err(err) => {
                        clear_child = true;
                        exit_info = Some((None, !desired_running, Some(err.to_string())));
                    }
                }
            }

            if clear_child {
                runtime.child = None;
                runtime.started_at_ms = None;
            }

            exit_info
        };

        if let Some((code, expected_stop, error_message)) = exit_state {
            let derived_error = error_message.unwrap_or_else(|| match code {
                Some(value) => format!("Gateway exited ({value})"),
                None => "Gateway exited".to_string(),
            });

            let _ = set_gateway_status(&app, &state, |status| {
                status.connected_at = None;
                status.error = if expected_stop {
                    None
                } else {
                    Some(derived_error.clone())
                };
                status.pid = None;
                status.reconnect_attempts = None;
                status.state = if expected_stop {
                    "stopped".into()
                } else {
                    "error".into()
                };
                status.uptime = None;
                status.version = None;
            });

            let _ = app.emit("gateway:exit", code);
            if !expected_stop {
                emit_gateway_error(&app, &derived_error);
            }
        }

        std::thread::sleep(Duration::from_millis(500));
    });
}

#[cfg(windows)]
fn listening_pids_for_port(port: u16) -> Vec<u32> {
    let output = match Command::new("netstat").args(["-ano", "-p", "tcp"]).output() {
        Ok(output) => output,
        Err(_) => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let needle = format!(":{port}");
    let mut pids = Vec::new();

    for line in stdout.lines() {
        let trimmed = line.trim();
        if !trimmed.contains("LISTENING") || !trimmed.contains(&needle) {
            continue;
        }

        if let Some(pid) = trimmed.split_whitespace().last() {
            if let Ok(pid) = pid.parse::<u32>() {
                pids.push(pid);
            }
        }
    }

    pids.sort_unstable();
    pids.dedup();
    pids
}

#[cfg(not(windows))]
fn listening_pids_for_port(port: u16) -> Vec<u32> {
    let output = match Command::new("lsof")
        .args([&format!("-iTCP:{port}"), "-sTCP:LISTEN", "-t", "-n", "-P"])
        .output()
    {
        Ok(output) => output,
        Err(_) => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut pids = stdout
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .collect::<Vec<_>>();
    pids.sort_unstable();
    pids.dedup();
    pids
}

#[cfg(windows)]
fn terminate_pid(pid: u32, force: bool) {
    let mut command = Command::new("taskkill");
    command.args(["/PID", &pid.to_string(), "/T"]);
    if force {
        command.arg("/F");
    }
    let _ = command.output();
}

#[cfg(not(windows))]
fn terminate_pid(pid: u32, force: bool) {
    let signal = if force { "-KILL" } else { "-TERM" };
    let _ = Command::new("kill")
        .args([signal, &pid.to_string()])
        .output();
}

fn force_stop_gateway_listener(port: u16) -> Vec<u32> {
    let current_pid = std::process::id();
    let mut initial = listening_pids_for_port(port)
        .into_iter()
        .filter(|pid| *pid != current_pid)
        .collect::<Vec<_>>();

    if initial.is_empty() {
        return initial;
    }

    for pid in &initial {
        terminate_pid(*pid, false);
    }

    let deadline = now_ms() + 2_000;
    while now_ms() < deadline {
        if listening_pids_for_port(port).is_empty() {
            return initial;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let remaining = listening_pids_for_port(port)
        .into_iter()
        .filter(|pid| *pid != current_pid)
        .collect::<Vec<_>>();

    for pid in &remaining {
        terminate_pid(*pid, true);
    }

    std::thread::sleep(Duration::from_millis(200));

    initial.extend(remaining);
    initial.sort_unstable();
    initial.dedup();
    initial
}

fn gateway_command() -> Result<Command, String> {
    let node_resolution = resolve_node_binary();
    let Some(node_binary) = node_resolution.path.clone() else {
        return Err(format!(
            "No compatible Node.js runtime available: {}",
            node_resolution.failure_message()
        ));
    };

    let openclaw_resolution = resolve_openclaw_runtime();
    let Some(openclaw_entry) = openclaw_resolution.entry_path.clone() else {
        return Err(format!(
            "No compatible OpenClaw runtime available: {}",
            openclaw_resolution.failure_message()
        ));
    };

    let mut command = Command::new(node_binary);
    apply_proxy_env(&mut command, &load_settings());
    command.arg(openclaw_entry);
    Ok(command)
}

fn gateway_start_internal(app: &AppHandle, state: &BridgeState) -> Result<Value, String> {
    let settings = load_settings();
    let port = settings.gateway_port;
    let token = settings.gateway_token.clone();
    sync_gateway_settings_to_openclaw(&settings)?;

    {
        let mut runtime = state
            .gateway_runtime
            .lock()
            .map_err(|_| "Gateway runtime lock poisoned".to_string())?;

        if let Some(child) = runtime.child.as_mut() {
            match child.try_wait() {
                Ok(None) => {
                    let pid = child.id();
                    runtime.desired_running = true;
                    drop(runtime);

                    let status = set_gateway_status(app, state, |status| {
                        status.error = None;
                        status.pid = Some(pid);
                        status.port = port;
                        if status.state == "stopped" || status.state == "error" {
                            status.state = "starting".into();
                        }
                    })?;

                    return Ok(json!({ "success": true, "status": status }));
                }
                Ok(Some(_)) | Err(_) => {
                    runtime.child = None;
                    runtime.started_at_ms = None;
                }
            }
        }
    }

    let cleared_pids = force_stop_gateway_listener(port);
    if !cleared_pids.is_empty() {
        append_log_line(
            "INFO",
            &format!(
                "Cleared stale Gateway listener(s) on port {port}: {}",
                cleared_pids
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }

    set_gateway_status(app, state, |status| {
        status.connected_at = None;
        status.error = None;
        status.pid = None;
        status.port = port;
        status.reconnect_attempts = Some(0);
        status.state = "starting".into();
        status.uptime = None;
        status.version = None;
    })?;

    let runtime_status = runtime_status_payload();
    append_log_line(
        "INFO",
        &format!(
            "Launching Gateway with node={} openclaw={} mode={}",
            runtime_status
                .node
                .source
                .map(NodeBinarySource::as_str)
                .unwrap_or("unresolved"),
            runtime_status
                .openclaw
                .source
                .map(OpenClawRuntimeSource::as_str)
                .unwrap_or("unresolved"),
            runtime_status.mode
        ),
    );

    let mut command = gateway_command()?;
    command
        .arg("gateway")
        .arg("--port")
        .arg(port.to_string())
        .arg("--token")
        .arg(token.clone())
        .arg("--force")
        .arg("--bind")
        .arg("loopback")
        .arg("--allow-unconfigured")
        .arg("--verbose")
        .current_dir(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .env("OPENCLAW_GATEWAY_TOKEN", token)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    let mut child = command
        .spawn()
        .map_err(|err| format!("Failed to launch OpenClaw Gateway: {err}"))?;
    let pid = child.id();

    if let Some(stdout) = child.stdout.take() {
        spawn_log_thread(stdout, "INFO");
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_log_thread(stderr, "WARN");
    }

    {
        let mut runtime = state
            .gateway_runtime
            .lock()
            .map_err(|_| "Gateway runtime lock poisoned".to_string())?;
        runtime.child = Some(child);
        runtime.desired_running = true;
        runtime.started_at_ms = Some(now_ms());
    }

    let status = set_gateway_status(app, state, |status| {
        status.error = None;
        status.pid = Some(pid);
        status.port = port;
        status.reconnect_attempts = Some(0);
        status.state = "starting".into();
        status.uptime = None;
        status.version = None;
    })?;

    append_log_line(
        "INFO",
        &format!("Started OpenClaw Gateway process on port {port} (pid={pid})"),
    );

    Ok(json!({ "success": true, "pid": pid, "status": status }))
}

fn gateway_stop_internal(app: &AppHandle, state: &BridgeState) -> Result<Value, String> {
    let port = load_settings().gateway_port;
    let mut runtime = state
        .gateway_runtime
        .lock()
        .map_err(|_| "Gateway runtime lock poisoned".to_string())?;

    runtime.desired_running = false;

    if let Some(child) = runtime.child.as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }

    runtime.child = None;
    runtime.started_at_ms = None;
    drop(runtime);

    let cleared_pids = force_stop_gateway_listener(port);
    if !cleared_pids.is_empty() {
        append_log_line(
            "INFO",
            &format!(
                "Stopped lingering Gateway listener(s) on port {port}: {}",
                cleared_pids
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }

    let status = set_gateway_status(app, state, |status| {
        status.connected_at = None;
        status.error = None;
        status.pid = None;
        status.reconnect_attempts = Some(0);
        status.state = "stopped".into();
        status.uptime = None;
        status.version = None;
    })?;

    Ok(json!({ "success": true, "status": status }))
}

fn gateway_restart_internal(app: &AppHandle, state: &BridgeState) -> Result<Value, String> {
    let _ = gateway_stop_internal(app, state)?;
    std::thread::sleep(Duration::from_millis(300));
    gateway_start_internal(app, state)
}

fn gateway_health(state: &BridgeState) -> Result<Value, String> {
    let status = gateway_status_snapshot(state)?;
    let address = SocketAddr::from(([127, 0, 0, 1], status.port));
    let ok = TcpStream::connect_timeout(&address, Duration::from_secs(1)).is_ok();

    Ok(json!({
        "success": true,
        "ok": ok,
        "error": if ok { Value::Null } else { Value::String(status.error.unwrap_or_else(|| "Gateway is not reachable".into())) },
        "uptime": status.uptime,
    }))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn decode_base64url<const N: usize>(value: &str) -> Result<[u8; N], String> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|err| err.to_string())?;
    let length = bytes.len();
    bytes
        .try_into()
        .map_err(|_| format!("Expected {N} bytes, got {length}"))
}

fn generate_device_identity() -> StoredDeviceIdentity {
    let signing_key = SigningKey::generate(&mut OsRng);
    let secret_key = signing_key.to_bytes();
    let public_key = signing_key.verifying_key().to_bytes();
    let device_id = hex_encode(Sha256::digest(public_key).as_slice());

    StoredDeviceIdentity {
        version: 1,
        device_id,
        public_key: URL_SAFE_NO_PAD.encode(public_key),
        secret_key: URL_SAFE_NO_PAD.encode(secret_key),
        created_at_ms: now_ms(),
    }
}

fn load_or_create_device_identity() -> Result<StoredDeviceIdentity, String> {
    let path = device_identity_path();

    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(identity) = serde_json::from_str::<StoredDeviceIdentity>(&content) {
                let public_key = decode_base64url::<32>(&identity.public_key);
                let secret_key = decode_base64url::<32>(&identity.secret_key);
                if public_key.is_ok() && secret_key.is_ok() {
                    let derived_id = hex_encode(Sha256::digest(public_key.unwrap()).as_slice());
                    if derived_id == identity.device_id {
                        return Ok(identity);
                    }
                }
            }
        }
    }

    let identity = generate_device_identity();
    write_json(&path, &identity)?;
    Ok(identity)
}

fn build_device_auth_payload(
    device_id: &str,
    client_id: &str,
    client_mode: &str,
    role: &str,
    scopes: &[String],
    signed_at_ms: u64,
    token: Option<&str>,
    nonce: Option<&str>,
) -> String {
    let version = if nonce.is_some() { "v2" } else { "v1" };
    let mut parts = vec![
        version.to_string(),
        device_id.to_string(),
        client_id.to_string(),
        client_mode.to_string(),
        role.to_string(),
        scopes.join(","),
        signed_at_ms.to_string(),
        token.unwrap_or_default().to_string(),
    ];
    if version == "v2" {
        parts.push(nonce.unwrap_or_default().to_string());
    }
    parts.join("|")
}

fn gateway_build_connect_params(app: &AppHandle, payload: &Value) -> Result<Value, String> {
    let settings = load_settings();
    let payload_object = payload.as_object().cloned().unwrap_or_default();
    let client_id = payload_object
        .get("clientId")
        .and_then(Value::as_str)
        .unwrap_or("gateway-client");
    let client_mode = payload_object
        .get("clientMode")
        .and_then(Value::as_str)
        .unwrap_or("ui");
    let nonce = payload_object
        .get("nonce")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let role = payload_object
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("operator");
    let scopes = payload_object
        .get("scopes")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<String>>()
        })
        .filter(|items| !items.is_empty())
        .unwrap_or_else(|| {
            DEFAULT_GATEWAY_SCOPES
                .iter()
                .map(|value| (*value).to_string())
                .collect()
        });

    if nonce.trim().is_empty() {
        return Err("Gateway connect.challenge nonce is required".into());
    }

    let identity = load_or_create_device_identity()?;
    let signing_key = SigningKey::from_bytes(&decode_base64url::<32>(&identity.secret_key)?);
    let signed_at_ms = now_ms();
    let signature_payload = build_device_auth_payload(
        &identity.device_id,
        client_id,
        client_mode,
        role,
        &scopes,
        signed_at_ms,
        Some(&settings.gateway_token),
        Some(nonce),
    );
    let signature = signing_key.sign(signature_payload.as_bytes()).to_bytes();

    Ok(json!({
        "minProtocol": 3,
        "maxProtocol": 3,
        "client": {
            "id": client_id,
            "displayName": "Clawy",
            "version": app.package_info().version.to_string(),
            "platform": platform_name(),
            "mode": client_mode,
        },
        "auth": {
            "token": settings.gateway_token,
        },
        "caps": [],
        "commands": [],
        "permissions": {},
        "role": role,
        "scopes": scopes,
        "device": {
            "id": identity.device_id,
            "publicKey": identity.public_key,
            "signature": URL_SAFE_NO_PAD.encode(signature),
            "signedAt": signed_at_ms,
            "nonce": nonce,
        },
    }))
}

fn gateway_mark_connected(
    app: &AppHandle,
    state: &BridgeState,
    payload: &Value,
) -> Result<Value, String> {
    let version = payload
        .get("version")
        .and_then(Value::as_str)
        .map(ToString::to_string);

    let status = set_gateway_status(app, state, |status| {
        status.connected_at = Some(now_ms());
        status.error = None;
        status.reconnect_attempts = Some(0);
        status.state = "running".into();
        status.version = version.clone();
    })?;

    Ok(json!({ "success": true, "status": status }))
}

fn gateway_mark_disconnected(
    app: &AppHandle,
    state: &BridgeState,
    payload: &Value,
) -> Result<Value, String> {
    let reconnecting = payload
        .get("reconnecting")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let error = payload
        .get("error")
        .and_then(Value::as_str)
        .map(ToString::to_string);

    let status = set_gateway_status(app, state, |status| {
        status.connected_at = None;
        status.error = error.clone();
        status.state = if reconnecting {
            "reconnecting".into()
        } else if error.is_some() {
            "error".into()
        } else {
            "stopped".into()
        };
        status.version = None;
    })?;

    Ok(json!({ "success": true, "status": status }))
}

fn save_image(params: Value) -> Result<Value, String> {
    let file_path = params
        .get("filePath")
        .and_then(Value::as_str)
        .map(PathBuf::from);
    let base64_data = params.get("base64").and_then(Value::as_str);
    let default_file_name = params
        .get("defaultFileName")
        .and_then(Value::as_str)
        .unwrap_or("image.png");
    let downloads_dir = dirs::download_dir().unwrap_or_else(clawy_base_dir);
    ensure_dir(&downloads_dir)?;
    let destination = downloads_dir.join(default_file_name);

    if let Some(existing_path) = file_path {
        fs::copy(&existing_path, &destination).map_err(|err| err.to_string())?;
    } else if let Some(data) = base64_data {
        let bytes = STANDARD.decode(data).map_err(|err| err.to_string())?;
        fs::write(&destination, bytes).map_err(|err| err.to_string())?;
    } else {
        return Ok(json!({ "success": false, "error": "No image data provided" }));
    }

    Ok(json!({ "success": true, "savedPath": destination }))
}

fn prepare_chat_with_media(params: Value) -> Result<Value, String> {
    let session_key = params
        .get("sessionKey")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut message = params
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let deliver = params
        .get("deliver")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let idempotency_key = params
        .get("idempotencyKey")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let media = params
        .get("media")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut image_attachments: Vec<Value> = Vec::new();
    let mut file_references: Vec<String> = Vec::new();

    for item in media {
        let file_path = item
            .get("filePath")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mime_type = item
            .get("mimeType")
            .and_then(Value::as_str)
            .unwrap_or("application/octet-stream");
        let file_name = item
            .get("fileName")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .unwrap_or_else(|| file_name_from_path(Path::new(file_path)));

        if file_path.is_empty() || !Path::new(file_path).exists() {
            continue;
        }

        file_references.push(format!(
            "[media attached: {file_path} ({mime_type}) | {file_path}]"
        ));

        if VISION_MIME_TYPES.contains(&mime_type) {
            let bytes = fs::read(file_path).map_err(|err| err.to_string())?;
            image_attachments.push(json!({
                "content": STANDARD.encode(bytes),
                "mimeType": mime_type,
                "fileName": file_name,
            }));
        }
    }

    if !file_references.is_empty() {
        let refs = file_references.join("\n");
        message = if message.trim().is_empty() {
            refs
        } else {
            format!("{message}\n\n{refs}")
        };
    }

    let mut rpc_params = json!({
        "sessionKey": session_key,
        "message": message,
        "deliver": deliver,
        "idempotencyKey": idempotency_key,
    });

    if !image_attachments.is_empty() {
        if let Some(object) = rpc_params.as_object_mut() {
            object.insert("attachments".into(), Value::Array(image_attachments));
        }
    }

    Ok(rpc_params)
}

fn delete_session(session_key: &str) -> Result<Value, String> {
    if !session_key.starts_with("agent:") {
        return Ok(
            json!({ "success": false, "error": format!("Invalid sessionKey: {session_key}") }),
        );
    }

    let parts: Vec<&str> = session_key.split(':').collect();
    if parts.len() < 3 {
        return Ok(
            json!({ "success": false, "error": format!("sessionKey has too few parts: {session_key}") }),
        );
    }

    let agent_id = parts[1];
    let sessions_dir = openclaw_config_dir()
        .join("agents")
        .join(agent_id)
        .join("sessions");
    let sessions_json_path = sessions_dir.join("sessions.json");

    let sessions_raw = fs::read_to_string(&sessions_json_path)
        .map_err(|err| format!("Could not read sessions.json: {err}"))?;
    let mut sessions_json: Value = serde_json::from_str(&sessions_raw)
        .map_err(|err| format!("Invalid sessions.json: {err}"))?;

    let mut uuid_file_name: Option<String> = None;
    let mut resolved_src_path: Option<String> = None;

    if let Some(entries) = sessions_json.get("sessions").and_then(Value::as_array) {
        for entry in entries {
            let entry_matches = entry
                .get("key")
                .and_then(Value::as_str)
                .map(|value| value == session_key)
                .unwrap_or(false)
                || entry
                    .get("sessionKey")
                    .and_then(Value::as_str)
                    .map(|value| value == session_key)
                    .unwrap_or(false);

            if !entry_matches {
                continue;
            }

            if let Some(value) = entry.get("file").and_then(Value::as_str) {
                uuid_file_name = Some(value.to_string());
                break;
            }
            if let Some(value) = entry.get("fileName").and_then(Value::as_str) {
                uuid_file_name = Some(value.to_string());
                break;
            }
            if let Some(value) = entry.get("path").and_then(Value::as_str) {
                uuid_file_name = Some(value.to_string());
                break;
            }
            if let Some(value) = entry.get("id").and_then(Value::as_str) {
                uuid_file_name = Some(format!("{value}.jsonl"));
                break;
            }
        }
    }

    if uuid_file_name.is_none() {
        if let Some(entry) = sessions_json.get(session_key) {
            if let Some(value) = entry.as_str() {
                uuid_file_name = Some(value.to_string());
            } else if let Some(object) = entry.as_object() {
                if let Some(path) = object
                    .get("sessionFile")
                    .and_then(Value::as_str)
                    .or_else(|| object.get("file").and_then(Value::as_str))
                    .or_else(|| object.get("fileName").and_then(Value::as_str))
                    .or_else(|| object.get("path").and_then(Value::as_str))
                {
                    if Path::new(path).is_absolute() {
                        resolved_src_path = Some(path.to_string());
                    } else {
                        uuid_file_name = Some(path.to_string());
                    }
                } else if let Some(value) = object
                    .get("id")
                    .and_then(Value::as_str)
                    .or_else(|| object.get("sessionId").and_then(Value::as_str))
                {
                    uuid_file_name = Some(format!("{value}.jsonl"));
                }
            }
        }
    }

    if uuid_file_name.is_none() && resolved_src_path.is_none() {
        return Ok(json!({
            "success": false,
            "error": format!("Cannot resolve file for session: {session_key}"),
        }));
    }

    if resolved_src_path.is_none() {
        let mut file_name = uuid_file_name.unwrap_or_default();
        if !file_name.ends_with(".jsonl") {
            file_name.push_str(".jsonl");
        }
        resolved_src_path = Some(sessions_dir.join(file_name).to_string_lossy().to_string());
    }

    let src_path = resolved_src_path.unwrap_or_default();
    let dst_path = src_path.replace(".jsonl", ".deleted.jsonl");

    if Path::new(&src_path).exists() {
        let _ = fs::rename(&src_path, &dst_path);
    }

    if let Some(entries) = sessions_json
        .get_mut("sessions")
        .and_then(Value::as_array_mut)
    {
        entries.retain(|entry| {
            entry
                .get("key")
                .and_then(Value::as_str)
                .map(|value| value != session_key)
                .unwrap_or(true)
                && entry
                    .get("sessionKey")
                    .and_then(Value::as_str)
                    .map(|value| value != session_key)
                    .unwrap_or(true)
        });
    } else if let Some(object) = sessions_json.as_object_mut() {
        object.remove(session_key);
    }

    fs::write(
        &sessions_json_path,
        serde_json::to_string_pretty(&sessions_json).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;

    Ok(json!({ "success": true }))
}

#[tauri::command]
fn invoke_ipc(
    app: tauri::AppHandle,
    state: tauri::State<'_, BridgeState>,
    channel: String,
    args: Vec<Value>,
) -> Result<Value, String> {
    match channel.as_str() {
        "app:version" => Ok(json!(app.package_info().version.to_string())),
        "app:name" => Ok(json!(app.package_info().name.clone())),
        "app:platform" => Ok(json!(platform_name())),
        "app:getPath" => Ok(json!(app_get_path(
            args.get(0).and_then(Value::as_str).unwrap_or("userData")
        ))),
        "shell:openPath" => {
            let target = args.get(0).and_then(Value::as_str).unwrap_or_default();
            if target.trim().is_empty() {
                return Err("Path is required".to_string());
            }

            open_path_with_system(Path::new(target))?;
            Ok(json!(""))
        }
        "app:quit" => {
            app.exit(0);
            Ok(Value::Null)
        }
        "app:relaunch" => {
            app.restart();
        }

        "settings:getAll" => {
            Ok(serde_json::to_value(load_settings()).map_err(|err| err.to_string())?)
        }
        "settings:get" => {
            let key = args.get(0).and_then(Value::as_str).unwrap_or_default();
            let settings = serde_json::to_value(load_settings()).map_err(|err| err.to_string())?;
            Ok(settings.get(key).cloned().unwrap_or(Value::Null))
        }
        "settings:set" => {
            let key = args.get(0).and_then(Value::as_str).unwrap_or_default();
            let value = args.get(1).cloned().unwrap_or(Value::Null);
            let previous_settings = load_settings();
            let mut settings_value =
                serde_json::to_value(previous_settings.clone()).map_err(|err| err.to_string())?;
            if let Some(object) = settings_value.as_object_mut() {
                object.insert(key.to_string(), value);
                let settings: Settings =
                    serde_json::from_value(settings_value).map_err(|err| err.to_string())?;
                save_settings(&settings)?;
                if proxy_changed(&previous_settings, &settings) {
                    sync_proxy_settings_to_openclaw(&settings)?;
                    maybe_restart_gateway(&app, &state);
                }
            }
            Ok(json!({ "success": true }))
        }
        "settings:setMany" => {
            let patch = args
                .get(0)
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let previous_settings = load_settings();
            let mut settings_value =
                serde_json::to_value(previous_settings.clone()).map_err(|err| err.to_string())?;
            if let Some(object) = settings_value.as_object_mut() {
                merge_objects(object, &patch);
                let settings: Settings =
                    serde_json::from_value(settings_value).map_err(|err| err.to_string())?;
                save_settings(&settings)?;
                if proxy_changed(&previous_settings, &settings) {
                    sync_proxy_settings_to_openclaw(&settings)?;
                    maybe_restart_gateway(&app, &state);
                }
            }
            Ok(json!({ "success": true }))
        }
        "settings:reset" => {
            let settings = Settings::default();
            save_settings(&settings)?;
            Ok(json!({ "success": true, "settings": settings }))
        }

        "gateway:status" => Ok(serde_json::to_value(gateway_status_snapshot(&state)?)
            .map_err(|err| err.to_string())?),
        "gateway:isConnected" => {
            let status = gateway_status_snapshot(&state)?;
            Ok(json!(status.state == "running"))
        }
        "gateway:start" => gateway_start_internal(&app, &state),
        "gateway:stop" => gateway_stop_internal(&app, &state),
        "gateway:restart" => gateway_restart_internal(&app, &state),
        "gateway:rpc" => Ok(json!({
            "success": false,
            "error": "Gateway RPC is handled by the Tauri desktop bridge."
        })),
        "gateway:getControlUiUrl" => {
            let settings = load_settings();
            let url = format!(
                "http://127.0.0.1:{}/?token={}",
                settings.gateway_port, settings.gateway_token
            );
            Ok(json!({
                "success": true,
                "url": url,
                "port": settings.gateway_port,
                "token": settings.gateway_token
            }))
        }
        "gateway:health" => gateway_health(&state),
        "gateway:autoApprovePairing" => auto_approve_local_device_pairing(),
        "gateway:buildConnectParams" => {
            gateway_build_connect_params(&app, args.get(0).unwrap_or(&Value::Null))
        }
        "gateway:onConnected" => {
            gateway_mark_connected(&app, &state, args.get(0).unwrap_or(&Value::Null))
        }
        "gateway:onDisconnected" => {
            gateway_mark_disconnected(&app, &state, args.get(0).unwrap_or(&Value::Null))
        }

        "openclaw:status" => Ok(openclaw_status()),
        "openclaw:isReady" => Ok(json!(resolve_openclaw_runtime().dir.is_some())),
        "openclaw:getDir" => Ok(json!(get_openclaw_dir())),
        "openclaw:getConfigDir" => Ok(json!(openclaw_config_dir())),
        "openclaw:getSkillsDir" => {
            ensure_dir(&openclaw_skills_dir())?;
            Ok(json!(openclaw_skills_dir()))
        }
        "openclaw:getUpdateStatus" => Ok(openclaw_update_status()?),
        "openclaw:installUpdate" => {
            let payload = args
                .get(0)
                .cloned()
                .filter(|value| !value.is_null())
                .map(|value| {
                    serde_json::from_value::<ManagedOpenClawInstallPayload>(value)
                        .map_err(|err| format!("Invalid OpenClaw update payload: {err}"))
                })
                .transpose()?;
            Ok(install_openclaw_update(&app, &state, payload)?)
        }
        "openclaw:getCliCommand" => {
            let node_resolution = resolve_node_binary();
            let openclaw_resolution = resolve_openclaw_runtime();
            if let (Some(node_path), Some(entry_path)) = (
                node_resolution.path.as_ref(),
                openclaw_resolution.entry_path.as_ref(),
            ) {
                Ok(json!({
                    "success": true,
                    "command": format!("\"{}\" \"{}\"", node_path.to_string_lossy(), entry_path.to_string_lossy()),
                    "nodeSource": node_resolution.source.map(NodeBinarySource::as_str),
                    "openclawSource": openclaw_resolution.source.map(OpenClawRuntimeSource::as_str),
                }))
            } else {
                let node_error = if node_resolution.path.is_none() {
                    Some(node_resolution.failure_message())
                } else {
                    None
                };
                let openclaw_error = if openclaw_resolution.entry_path.is_none() {
                    Some(openclaw_resolution.failure_message())
                } else {
                    None
                };
                Ok(json!({
                    "success": false,
                    "error": node_error
                        .into_iter()
                        .chain(openclaw_error.into_iter())
                        .collect::<Vec<_>>()
                        .join("; ")
                }))
            }
        }
        "runtime:status" => Ok(json!(runtime_status_payload())),

        "runtime:installManagedNode" => {
            let payload = serde_json::from_value::<ManagedNodeInstallPayload>(
                args.get(0).cloned().unwrap_or(Value::Null),
            )
            .map_err(|err| format!("Invalid managed Node install payload: {err}"))?;
            Ok(json!({
                "success": true,
                "result": install_managed_node_release(&payload)?
            }))
        }
        "runtime:installRecommendedNode" => {
            match install_recommended_managed_node_release_with_progress(&app) {
                Ok(result) => Ok(json!({
                    "success": true,
                    "result": result
                })),
                Err(error) => {
                    emit_runtime_install_progress(
                        &app,
                        ManagedRuntimeKind::Node,
                        "failed",
                        "failed",
                        100.0,
                        Some(RECOMMENDED_MANAGED_NODE_VERSION),
                        None,
                        None,
                        Some(error.clone()),
                    );
                    Err(error)
                }
            }
        }
        "runtime:switchManagedNode" => {
            let version = args.get(0).and_then(Value::as_str).unwrap_or_default();
            let pointer = switch_managed_node_version(version)?;
            Ok(json!({
                "success": true,
                "pointer": pointer,
                "currentPath": managed_runtime_current_pointer_path(ManagedRuntimeKind::Node)
            }))
        }
        "runtime:installManagedOpenClaw" => {
            let payload = serde_json::from_value::<ManagedOpenClawInstallPayload>(
                args.get(0).cloned().unwrap_or(Value::Null),
            )
            .map_err(|err| format!("Invalid managed OpenClaw install payload: {err}"))?;
            Ok(json!({
                "success": true,
                "result": install_managed_openclaw_release(&payload)?
            }))
        }
        "runtime:installRecommendedOpenClaw" => {
            let version = recommended_openclaw_version().ok();
            match install_recommended_managed_openclaw_release_with_progress(&app) {
                Ok(result) => Ok(json!({
                    "success": true,
                    "result": result
                })),
                Err(error) => {
                    emit_runtime_install_progress(
                        &app,
                        ManagedRuntimeKind::OpenClaw,
                        "failed",
                        "failed",
                        100.0,
                        version.as_deref(),
                        None,
                        None,
                        Some(error.clone()),
                    );
                    Err(error)
                }
            }
        }
        "runtime:switchManagedOpenClaw" => {
            let version = args.get(0).and_then(Value::as_str).unwrap_or_default();
            let pointer = switch_managed_openclaw_version(version)?;
            Ok(json!({
                "success": true,
                "pointer": pointer,
                "currentPath": managed_runtime_current_pointer_path(ManagedRuntimeKind::OpenClaw)
            }))
        }

        "log:getDir" => {
            ensure_dir(&logs_dir())?;
            Ok(json!(logs_dir()))
        }
        "log:getFilePath" => Ok(json!(current_log_file_path())),
        "log:readFile" => {
            let tail = args.get(0).and_then(Value::as_u64).unwrap_or(200) as usize;
            Ok(json!(read_log_tail(tail)))
        }
        "log:getRecent" => {
            let tail = args.get(0).and_then(Value::as_u64).unwrap_or(100) as usize;
            let lines: Vec<String> = read_log_tail(tail)
                .lines()
                .map(|line| line.to_string())
                .collect();
            Ok(json!(lines))
        }
        "log:listFiles" => Ok(json!(list_log_files()?)),

        "update:status" => {
            Ok(serde_json::to_value(update_status_snapshot(&state)?)
                .map_err(|err| err.to_string())?)
        }
        "update:version" => Ok(json!(app.package_info().version.to_string())),
        "update:check" => match update_check_internal(&app, &state) {
            Ok(status) => Ok(json!({ "success": true, "status": status })),
            Err(error) => {
                let status = set_update_status(&app, &state, |status| {
                    status.status = "error".into();
                    status.error = Some(error.clone());
                    status.progress = None;
                })?;
                Ok(json!({ "success": false, "error": error, "status": status }))
            }
        },
        "update:download" => match download_update_internal(&app, &state) {
            Ok(()) => Ok(json!({ "success": true })),
            Err(error) => {
                if let Ok(mut runtime) = state.updater_runtime.lock() {
                    runtime.is_downloading = false;
                }
                let _ = set_update_status(&app, &state, |status| {
                    status.status = "error".into();
                    status.error = Some(error.clone());
                    status.progress = None;
                });
                Ok(json!({ "success": false, "error": error }))
            }
        },
        "update:install" => {
            cancel_auto_install_countdown(&app, &state);
            let downloaded = state
                .updater_runtime
                .lock()
                .map_err(|_| "Updater runtime lock poisoned".to_string())?
                .downloaded_file
                .clone();
            if let Some(path) = downloaded {
                open_path_with_system(&path)?;
                Ok(json!({ "success": true }))
            } else {
                Ok(json!({ "success": false, "error": "No downloaded update is available." }))
            }
        }
        "update:setChannel" => {
            let channel = args.get(0).and_then(Value::as_str).unwrap_or("stable");
            let mut settings = load_settings();
            settings.update_channel = channel.to_string();
            save_settings(&settings)?;
            cancel_auto_install_countdown(&app, &state);
            let _ = set_update_status(&app, &state, |status| {
                *status = UpdateStatusPayload::default();
            });
            if let Ok(mut runtime) = state.updater_runtime.lock() {
                runtime.download_target = None;
                runtime.downloaded_file = None;
                runtime.is_downloading = false;
            }
            Ok(json!({ "success": true }))
        }
        "update:setAutoDownload" => {
            let enable = args.get(0).and_then(Value::as_bool).unwrap_or(false);
            let mut settings = load_settings();
            settings.auto_download_update = enable;
            save_settings(&settings)?;
            if !enable {
                cancel_auto_install_countdown(&app, &state);
            }
            Ok(json!({ "success": true }))
        }
        "update:cancelAutoInstall" => {
            cancel_auto_install_countdown(&app, &state);
            Ok(json!({ "success": true }))
        }

        "usage:recentTokenHistory" => {
            let limit = args
                .get(0)
                .and_then(Value::as_u64)
                .map(|value| value.max(1) as usize);
            Ok(json!(recent_token_usage_history(limit)?))
        }

        "provider:list" => {
            let store = load_provider_store();
            Ok(serde_json::to_value(list_providers(&store)).map_err(|err| err.to_string())?)
        }
        "provider:get" => {
            let provider_id = args.get(0).and_then(Value::as_str).unwrap_or_default();
            let store = load_provider_store();
            Ok(store
                .providers
                .get(provider_id)
                .cloned()
                .map(|value| serde_json::to_value(value).map_err(|err| err.to_string()))
                .transpose()?
                .unwrap_or(Value::Null))
        }
        "provider:save" => {
            let config: ProviderConfig =
                serde_json::from_value(args.get(0).cloned().unwrap_or(Value::Null))
                    .map_err(|err| err.to_string())?;
            let api_key = args.get(1).and_then(Value::as_str).map(str::to_string);
            let mut store = load_provider_store();
            store.providers.insert(config.id.clone(), config.clone());
            if let Some(key) = api_key.as_ref() {
                if key.trim().is_empty() {
                    store.api_keys.remove(&config.id);
                } else {
                    store.api_keys.insert(config.id.clone(), key.clone());
                }
            }
            save_provider_store(&store)?;
            let _ = sync_provider_state_to_openclaw(&store, &config, api_key.as_deref());
            maybe_restart_gateway(&app, &state);
            Ok(json!({ "success": true }))
        }
        "provider:updateWithKey" => {
            let provider_id = args.get(0).and_then(Value::as_str).unwrap_or_default();
            let updates = args
                .get(1)
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let api_key = args.get(2).and_then(Value::as_str).map(str::to_string);
            let mut store = load_provider_store();
            let current = store
                .providers
                .get(provider_id)
                .cloned()
                .ok_or_else(|| "Provider not found".to_string())?;
            let mut current_value = serde_json::to_value(current).map_err(|err| err.to_string())?;
            if let Some(object) = current_value.as_object_mut() {
                merge_objects(object, &updates);
            }
            let updated: ProviderConfig =
                serde_json::from_value(current_value).map_err(|err| err.to_string())?;
            store
                .providers
                .insert(provider_id.to_string(), updated.clone());
            if let Some(key) = api_key.as_ref() {
                if key.trim().is_empty() {
                    store.api_keys.remove(provider_id);
                } else {
                    store.api_keys.insert(provider_id.to_string(), key.clone());
                }
            }
            save_provider_store(&store)?;
            let _ = sync_provider_state_to_openclaw(&store, &updated, api_key.as_deref());
            maybe_restart_gateway(&app, &state);
            Ok(json!({ "success": true }))
        }
        "provider:delete" => {
            let provider_id = args.get(0).and_then(Value::as_str).unwrap_or_default();
            let mut store = load_provider_store();
            let removed = store.providers.remove(provider_id);
            store.api_keys.remove(provider_id);
            if store.default_provider.as_deref() == Some(provider_id) {
                store.default_provider = None;
            }
            save_provider_store(&store)?;
            if let Some(config) = removed {
                let provider_key = get_openclaw_provider_key(&config.provider_type, provider_id);
                let _ = remove_provider_from_openclaw(&provider_key);
            }
            maybe_restart_gateway(&app, &state);
            Ok(json!({ "success": true }))
        }
        "provider:setApiKey" => {
            let provider_id = args.get(0).and_then(Value::as_str).unwrap_or_default();
            let api_key = args.get(1).and_then(Value::as_str).unwrap_or_default();
            let mut store = load_provider_store();
            store
                .api_keys
                .insert(provider_id.to_string(), api_key.to_string());
            save_provider_store(&store)?;
            if let Some(config) = store.providers.get(provider_id) {
                let _ = sync_provider_state_to_openclaw(&store, config, Some(api_key));
            }
            maybe_restart_gateway(&app, &state);
            Ok(json!({ "success": true }))
        }
        "provider:deleteApiKey" => {
            let provider_id = args.get(0).and_then(Value::as_str).unwrap_or_default();
            let mut store = load_provider_store();
            store.api_keys.remove(provider_id);
            save_provider_store(&store)?;
            if let Some(config) = store.providers.get(provider_id) {
                let provider_key = get_openclaw_provider_key(&config.provider_type, provider_id);
                let _ = remove_provider_from_openclaw(&provider_key);
                let _ = sync_provider_state_to_openclaw(&store, config, None);
            }
            maybe_restart_gateway(&app, &state);
            Ok(json!({ "success": true }))
        }
        "provider:hasApiKey" => {
            let provider_id = args.get(0).and_then(Value::as_str).unwrap_or_default();
            let store = load_provider_store();
            Ok(json!(store.api_keys.contains_key(provider_id)))
        }
        "provider:getApiKey" => {
            let provider_id = args.get(0).and_then(Value::as_str).unwrap_or_default();
            let store = load_provider_store();
            Ok(store
                .api_keys
                .get(provider_id)
                .cloned()
                .map(Value::String)
                .unwrap_or(Value::Null))
        }
        "provider:setDefault" => {
            let provider_id = args.get(0).and_then(Value::as_str).unwrap_or_default();
            let mut store = load_provider_store();
            store.default_provider = Some(provider_id.to_string());
            save_provider_store(&store)?;
            if let Some(config) = store.providers.get(provider_id) {
                let api_key = store.api_keys.get(provider_id).map(String::as_str);
                let _ = sync_provider_state_to_openclaw(&store, config, api_key);
            }
            maybe_restart_gateway(&app, &state);
            Ok(json!({ "success": true }))
        }
        "provider:getDefault" => {
            let store = load_provider_store();
            Ok(store
                .default_provider
                .map(Value::String)
                .unwrap_or(Value::Null))
        }
        "provider:validateKey" => {
            let api_key = args.get(1).and_then(Value::as_str).unwrap_or_default();
            if api_key.trim().is_empty() {
                Ok(json!({ "valid": false, "error": "API key is required" }))
            } else {
                Ok(json!({ "valid": true }))
            }
        }
        "provider:requestOAuth" => {
            let provider = args.get(0).and_then(Value::as_str).unwrap_or_default();
            start_oauth_flow(&app, &state, provider)
        }
        "provider:cancelOAuth" => {
            cancel_oauth_flow(&state)?;
            Ok(json!({ "success": true }))
        }

        "skill:getAllConfigs" => get_all_skill_configs_value(),
        "skill:getConfig" => {
            let skill_key = args.get(0).and_then(Value::as_str).unwrap_or_default();
            get_skill_config_value(skill_key)
        }
        "skill:updateConfig" => update_skill_config_value(args.get(0).unwrap_or(&Value::Null)),

        "clawhub:list" => {
            let output = run_clawhub_command(&[String::from("list")])?;
            Ok(json!({
                "success": true,
                "results": parse_clawhub_list_results(&output),
            }))
        }
        "clawhub:search" => {
            let payload = args
                .get(0)
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let query = payload
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            let limit = payload
                .get("limit")
                .and_then(Value::as_u64)
                .map(|value| value.to_string());

            let mut command_args = if query.is_empty() {
                vec![String::from("explore")]
            } else {
                vec![String::from("search"), query]
            };
            if let Some(limit) = limit {
                command_args.push(String::from("--limit"));
                command_args.push(limit);
            }

            let output = run_clawhub_command(&command_args)?;
            let results = if command_args.first().map(String::as_str) == Some("explore") {
                parse_clawhub_explore_results(&output)
            } else {
                parse_clawhub_search_results(&output)
            };
            Ok(json!({
                "success": true,
                "results": results,
            }))
        }
        "clawhub:install" => {
            let payload = args
                .get(0)
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let slug = payload
                .get("slug")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            if slug.is_empty() {
                return Ok(json!({ "success": false, "error": "Skill slug is required" }));
            }

            let mut command_args = vec![String::from("install"), slug];
            if let Some(version) = payload
                .get("version")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                command_args.push(String::from("--version"));
                command_args.push(version.to_string());
            }
            if payload
                .get("force")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                command_args.push(String::from("--force"));
            }

            match run_clawhub_command(&command_args) {
                Ok(_) => Ok(json!({ "success": true })),
                Err(error) => Ok(json!({ "success": false, "error": error })),
            }
        }
        "clawhub:uninstall" => {
            let slug = args
                .get(0)
                .and_then(Value::as_object)
                .and_then(|payload| payload.get("slug"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            if slug.is_empty() {
                return Ok(json!({ "success": false, "error": "Skill slug is required" }));
            }
            match uninstall_clawhub_skill(&slug) {
                Ok(_) => Ok(json!({ "success": true })),
                Err(error) => Ok(json!({ "success": false, "error": error })),
            }
        }
        "clawhub:openSkillReadme" => {
            let slug = args.get(0).and_then(Value::as_str).unwrap_or_default();
            match find_skill_readme_path(slug).and_then(|path| open_path_with_system(&path)) {
                Ok(_) => Ok(json!({ "success": true })),
                Err(error) => Ok(json!({ "success": false, "error": error })),
            }
        }

        "cron:list" | "cron:create" | "cron:update" | "cron:delete" | "cron:toggle"
        | "cron:trigger" => Ok(json!({
            "success": false,
            "error": "Cron RPC is handled by the Tauri desktop bridge."
        })),

        "channel:saveConfig" => {
            let channel_type = args.get(0).and_then(Value::as_str).unwrap_or_default();
            let config = args
                .get(1)
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            save_channel_config_value(channel_type, &config)
        }
        "channel:listConfigured" => Ok(json!({
            "success": true,
            "channels": list_configured_channels_value()?,
        })),
        "channel:getConfig" => {
            let channel_type = args.get(0).and_then(Value::as_str).unwrap_or_default();
            Ok(json!({
                "success": true,
                "config": read_channel_config_value(channel_type)?,
            }))
        }
        "channel:getFormValues" => {
            let channel_type = args.get(0).and_then(Value::as_str).unwrap_or_default();
            Ok(json!({
                "success": true,
                "values": get_channel_form_values_value(channel_type)?,
            }))
        }
        "channel:deleteConfig" => {
            let channel_type = args.get(0).and_then(Value::as_str).unwrap_or_default();
            delete_channel_config_value(channel_type)?;
            Ok(json!({ "success": true }))
        }
        "channel:setEnabled" => {
            let channel_type = args.get(0).and_then(Value::as_str).unwrap_or_default();
            let enabled = args.get(1).and_then(Value::as_bool).unwrap_or(true);
            set_channel_enabled_value(channel_type, enabled)?;
            Ok(json!({ "success": true }))
        }
        "channel:validate" => {
            let channel_type = args.get(0).and_then(Value::as_str).unwrap_or_default();
            let result = validate_channel_config_value(channel_type)?;
            Ok(
                json!({ "success": true, "valid": result.get("valid").and_then(Value::as_bool).unwrap_or(false), "errors": result.get("errors").cloned().unwrap_or_else(|| json!([])), "warnings": result.get("warnings").cloned().unwrap_or_else(|| json!([])) }),
            )
        }
        "channel:validateCredentials" => {
            let channel_type = args.get(0).and_then(Value::as_str).unwrap_or_default();
            let config = args
                .get(1)
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let result = validate_channel_credentials_value(channel_type, &config)?;
            Ok(json!({
                "success": true,
                "valid": result.get("valid").and_then(Value::as_bool).unwrap_or(false),
                "errors": result.get("errors").cloned().unwrap_or_else(|| json!([])),
                "warnings": result.get("warnings").cloned().unwrap_or_else(|| json!([])),
                "details": result.get("details").cloned().unwrap_or(Value::Null),
            }))
        }
        "channel:requestWhatsAppQr" => {
            let account_id = args.get(0).and_then(Value::as_str).unwrap_or("default");
            request_whatsapp_qr(&app, &state, account_id)
        }
        "channel:cancelWhatsAppQr" => {
            cancel_whatsapp_qr(&state)?;
            Ok(json!({ "success": true }))
        }

        "uv:check" => {
            let (binary, source) = resolve_uv_binary();
            Ok(json!(
                source != "missing" && (binary == PathBuf::from("uv") || binary.exists())
            ))
        }
        "uv:install-all" => uv_install_all(),

        "file:stage" => {
            let paths = args
                .get(0)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            ensure_dir(&outbound_media_dir())?;
            let mut staged = Vec::new();
            for path in paths {
                let source = PathBuf::from(path.as_str().unwrap_or_default());
                if !source.exists() {
                    continue;
                }
                let extension = source
                    .extension()
                    .map(|value| format!(".{}", value.to_string_lossy()))
                    .unwrap_or_default();
                let id = Uuid::new_v4().to_string();
                let destination = outbound_media_dir().join(format!("{id}{extension}"));
                fs::copy(&source, &destination).map_err(|err| err.to_string())?;
                let metadata = fs::metadata(&destination).map_err(|err| err.to_string())?;
                staged.push(json!({
                    "id": id,
                    "fileName": file_name_from_path(&source),
                    "mimeType": "application/octet-stream",
                    "fileSize": metadata.len(),
                    "stagedPath": destination,
                    "preview": Value::Null
                }));
            }
            Ok(json!(staged))
        }
        "file:stageBuffer" => {
            let payload = args.get(0).cloned().unwrap_or(Value::Null);
            let base64_data = payload
                .get("base64")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let file_name = payload
                .get("fileName")
                .and_then(Value::as_str)
                .unwrap_or("file");
            let mime_type = payload
                .get("mimeType")
                .and_then(Value::as_str)
                .unwrap_or("application/octet-stream");
            ensure_dir(&outbound_media_dir())?;
            let extension = Path::new(file_name)
                .extension()
                .map(|value| format!(".{}", value.to_string_lossy()))
                .unwrap_or_else(|| ext_for_mime_type(mime_type).to_string());
            let id = Uuid::new_v4().to_string();
            let destination = outbound_media_dir().join(format!("{id}{extension}"));
            let bytes = STANDARD
                .decode(base64_data)
                .map_err(|err| err.to_string())?;
            fs::write(&destination, bytes.as_slice()).map_err(|err| err.to_string())?;
            Ok(json!({
                "id": id,
                "fileName": file_name,
                "mimeType": mime_type,
                "fileSize": bytes.len(),
                "stagedPath": destination,
                "preview": Value::Null
            }))
        }
        "media:getThumbnails" => {
            let paths = args
                .get(0)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut result = Map::new();
            for path in paths {
                let file_path = path
                    .get("filePath")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let file_size = fs::metadata(file_path).map(|meta| meta.len()).unwrap_or(0);
                result.insert(
                    file_path.to_string(),
                    json!({
                        "preview": Value::Null,
                        "fileSize": file_size
                    }),
                );
            }
            Ok(Value::Object(result))
        }
        "media:saveImage" => save_image(args.get(0).cloned().unwrap_or(Value::Null)),

        "chat:prepareWithMedia" => {
            prepare_chat_with_media(args.get(0).cloned().unwrap_or(Value::Null))
        }
        "chat:sendWithMedia" => Ok(json!({
            "success": false,
            "error": "chat:sendWithMedia is handled by the Tauri desktop bridge."
        })),
        "session:delete" => delete_session(args.get(0).and_then(Value::as_str).unwrap_or_default()),

        other => {
            append_log_line("WARN", &format!("Unsupported Tauri IPC channel: {other}"));
            Err(format!("Unsupported Tauri IPC channel: {other}"))
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    append_log_line("INFO", "Starting Clawy Tauri bridge");

    let bridge_state = BridgeState::default();
    let setup_state = bridge_state.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .on_menu_event(handle_menu_event)
        .manage(bridge_state)
        .invoke_handler(tauri::generate_handler![invoke_ipc])
        .setup(move |app| {
            start_gateway_monitor(app.handle().clone(), setup_state.clone());
            start_oauth_monitor(setup_state.clone());
            start_whatsapp_monitor(setup_state.clone());

            if let Err(error) = create_application_menu(app.handle()) {
                append_log_line(
                    "WARN",
                    &format!("Failed to create application menu: {error}"),
                );
            }
            if let Err(error) = create_tray_icon(app.handle()) {
                append_log_line("WARN", &format!("Failed to create tray icon: {error}"));
            }

            if load_settings().gateway_auto_start {
                let app_handle = app.handle().clone();
                let state = setup_state.clone();
                std::thread::spawn(move || {
                    if let Err(error) = gateway_start_internal(&app_handle, &state) {
                        append_log_line("WARN", &format!("Gateway auto-start failed: {error}"));
                    }
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Clawy Tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "clawy-managed-runtime-{name}-{}",
                Uuid::new_v4().simple()
            ));
            fs::create_dir_all(&path).expect("create test dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn write_test_script(path: &Path, contents: &str) {
        fs::write(path, contents).expect("write test script");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = fs::metadata(path).expect("read metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions).expect("set execute bit");
        }
    }

    fn fake_node_script_path(base_dir: &Path, name: &str) -> PathBuf {
        if cfg!(windows) {
            base_dir.join(format!("{name}.cmd"))
        } else {
            base_dir.join(name)
        }
    }

    fn create_fake_node_binary(
        base_dir: &Path,
        name: &str,
        version: &str,
        smoke_ok: bool,
    ) -> PathBuf {
        let path = fake_node_script_path(base_dir, name);
        let script = if cfg!(windows) {
            let smoke_exit = if smoke_ok { "0" } else { "1" };
            format!(
                "@echo off\r\nif \"%~1\"==\"--version\" (\r\n  echo v{version}\r\n  exit /b 0\r\n)\r\nif \"%~1\"==\"-e\" (\r\n  if \"%~2\"==\"{NODE_SMOKE_TEST_SCRIPT}\" (\r\n    if \"{smoke_exit}\"==\"0\" (\r\n      <nul set /p ={NODE_SMOKE_TEST_OUTPUT}\r\n    )\r\n    exit /b {smoke_exit}\r\n  )\r\n)\r\necho unexpected args %* 1>&2\r\nexit /b 1\r\n"
            )
        } else {
            let smoke_body = if smoke_ok {
                format!("  printf '{NODE_SMOKE_TEST_OUTPUT}'\n  exit 0")
            } else {
                "  echo 'smoke test failed' >&2\n  exit 1".into()
            };

            format!(
                "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  printf 'v{version}\\n'\n  exit 0\nfi\nif [ \"$1\" = \"-e\" ] && [ \"$2\" = \"{NODE_SMOKE_TEST_SCRIPT}\" ]; then\n{smoke_body}\nfi\necho \"unexpected args: $*\" >&2\nexit 1\n"
            )
        };

        write_test_script(&path, &script);
        path
    }

    fn create_fake_node_archive(base_dir: &Path, version: &str, archive_name: &str) -> PathBuf {
        let archive_path = base_dir.join(archive_name);
        let root_dir = format!("node-v{version}-test/");
        let archived_binary = create_fake_node_binary(base_dir, "archived-node", version, true);
        let archived_bytes = fs::read(&archived_binary).expect("read archived node bytes");

        let file = File::create(&archive_path).expect("create fake node archive");
        let mut archive = zip::ZipWriter::new(file);
        let options = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .unix_permissions(0o755);

        archive
            .add_directory(root_dir.clone(), options)
            .expect("add root directory");
        if !cfg!(windows) {
            archive
                .add_directory(format!("{root_dir}bin/"), options)
                .expect("add bin directory");
        }
        archive
            .start_file(
                format!("{root_dir}{}", managed_node_binary_relative_path()),
                options,
            )
            .expect("start archived node file");
        archive
            .write_all(&archived_bytes)
            .expect("write archived node bytes");
        archive.finish().expect("finish archive");

        archive_path
    }

    fn create_fake_openclaw_archive_with_options(
        base_dir: &Path,
        version: &str,
        archive_name: &str,
        include_entry: bool,
        include_node_modules: bool,
        package_name: &str,
    ) -> PathBuf {
        let archive_path = base_dir.join(archive_name);
        let package_dir =
            base_dir.join(format!("fake-openclaw-package-{}", Uuid::new_v4().simple()));

        fs::create_dir_all(package_dir.join("dist")).expect("create fake openclaw dist dir");
        if include_node_modules {
            fs::create_dir_all(package_dir.join("node_modules"))
                .expect("create fake openclaw node_modules dir");
            fs::write(package_dir.join("node_modules").join(".keep"), b"")
                .expect("write fake openclaw keep file");
        }

        fs::write(
            package_dir.join("package.json"),
            serde_json::to_string_pretty(&json!({
                "name": package_name,
                "version": version,
            }))
            .expect("serialize fake openclaw package"),
        )
        .expect("write fake openclaw package json");
        if include_entry {
            fs::write(
                package_dir.join("openclaw.mjs"),
                "#!/usr/bin/env node\nimport './dist/entry.mjs';\n",
            )
            .expect("write fake openclaw entry");
        }
        fs::write(
            package_dir.join("dist").join("entry.mjs"),
            "export const ok = true;\n",
        )
        .expect("write fake openclaw dist entry");

        let file = File::create(&archive_path).expect("create fake openclaw archive");
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        builder
            .append_dir_all("package", &package_dir)
            .expect("append fake openclaw package");
        let encoder = builder.into_inner().expect("finish tar archive");
        encoder.finish().expect("finish gzip encoder");

        fs::remove_dir_all(&package_dir).expect("remove fake openclaw package dir");
        archive_path
    }

    fn create_fake_openclaw_archive(base_dir: &Path, version: &str, archive_name: &str) -> PathBuf {
        create_fake_openclaw_archive_with_options(
            base_dir,
            version,
            archive_name,
            true,
            true,
            OPENCLAW_PACKAGE_NAME,
        )
    }

    fn create_fake_openclaw_runtime_dir(
        base_dir: &Path,
        name: &str,
        version: &str,
        include_entry: bool,
    ) -> PathBuf {
        let runtime_dir = base_dir.join(name);
        fs::create_dir_all(runtime_dir.join("dist")).expect("create fake openclaw dist dir");
        fs::write(
            runtime_dir.join("package.json"),
            serde_json::to_string_pretty(&json!({
                "name": OPENCLAW_PACKAGE_NAME,
                "version": version,
            }))
            .expect("serialize fake openclaw package"),
        )
        .expect("write fake openclaw package json");
        if include_entry {
            fs::write(
                runtime_dir.join("openclaw.mjs"),
                "#!/usr/bin/env node\nimport './dist/entry.mjs';\n",
            )
            .expect("write fake openclaw entry");
        }
        fs::write(
            runtime_dir.join("dist").join("entry.mjs"),
            "export const ok = true;\n",
        )
        .expect("write fake openclaw dist entry");
        runtime_dir
    }

    fn serve_http_bytes_once(file_name: &str, bytes: Vec<u8>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let port = listener.local_addr().expect("server addr").port();

        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request);
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                bytes.len()
            );
            stream
                .write_all(headers.as_bytes())
                .expect("write response headers");
            stream.write_all(&bytes).expect("write response body");
        });

        format!("http://127.0.0.1:{port}/{file_name}")
    }

    #[test]
    fn managed_runtime_helpers_build_versioned_layout_without_touching_openclaw_data_dir() {
        let home_dir = PathBuf::from("/tmp/clawy-home");
        let clawy_dir = clawy_base_dir_from_home(&home_dir);

        assert_eq!(clawy_dir, home_dir.join(".clawy-tauri"));
        assert_eq!(
            openclaw_config_dir_from_home(&home_dir),
            home_dir.join(".openclaw")
        );
        assert_eq!(
            managed_runtime_root_dir_from_base(&clawy_dir),
            clawy_dir.join("runtime")
        );
        assert_eq!(
            managed_runtime_downloads_dir_from_base(&clawy_dir),
            clawy_dir.join("runtime").join("downloads")
        );
        assert_eq!(
            managed_runtime_staging_dir_from_base(&clawy_dir, ManagedRuntimeKind::Node),
            clawy_dir.join("runtime").join("node").join("staging")
        );
        assert_eq!(
            managed_runtime_version_dir_from_base(&clawy_dir, ManagedRuntimeKind::Node, "20.18.0"),
            clawy_dir
                .join("runtime")
                .join("node")
                .join("versions")
                .join("20.18.0")
        );
        assert_eq!(
            managed_runtime_current_pointer_path_from_base(&clawy_dir, ManagedRuntimeKind::Node),
            clawy_dir.join("runtime").join("node").join("current")
        );
        assert_eq!(
            managed_runtime_manifest_path_from_base(
                &clawy_dir,
                ManagedRuntimeKind::OpenClaw,
                "2026.3.2",
            ),
            clawy_dir
                .join("runtime")
                .join("openclaw")
                .join("versions")
                .join("2026.3.2")
                .join("manifest.json")
        );
    }

    #[test]
    fn managed_runtime_state_load_or_create_uses_sane_defaults_and_explicit_pointers() {
        let test_dir = TestDir::new("state");

        let mut state = load_or_create_managed_runtime_state_in_base(test_dir.path())
            .expect("create runtime state");

        assert_eq!(state.schema_version, MANAGED_RUNTIME_SCHEMA_VERSION);
        assert!(state.node.current.is_none());
        assert!(state.node.versions.is_empty());
        assert!(managed_runtime_state_path_from_base(test_dir.path()).exists());

        state.set_current_version(ManagedRuntimeKind::Node, "20.18.0");
        state.track_version(ManagedRuntimeKind::OpenClaw, "2026.3.2");
        save_managed_runtime_state_to_base(test_dir.path(), &state).expect("save runtime state");

        let reloaded = load_managed_runtime_state_from_base(test_dir.path());
        assert_eq!(
            reloaded.node.current,
            Some(ManagedRuntimeVersionPointer::new(
                ManagedRuntimeKind::Node,
                "20.18.0",
            ))
        );
        assert_eq!(
            reloaded.openclaw.versions,
            vec![ManagedRuntimeVersionPointer::new(
                ManagedRuntimeKind::OpenClaw,
                "2026.3.2",
            )]
        );
    }

    #[test]
    fn managed_runtime_manifest_load_or_create_persists_node_and_openclaw_versions() {
        let test_dir = TestDir::new("manifest");

        let node_manifest = load_or_create_managed_runtime_manifest_in_base(
            test_dir.path(),
            ManagedRuntimeKind::Node,
            "20.18.0",
        )
        .expect("create node manifest");
        let openclaw_manifest = load_or_create_managed_runtime_manifest_in_base(
            test_dir.path(),
            ManagedRuntimeKind::OpenClaw,
            "2026.3.2",
        )
        .expect("create openclaw manifest");

        assert_eq!(
            node_manifest.runtime_dir,
            PathBuf::from("node").join("versions").join("20.18.0")
        );
        assert_eq!(
            openclaw_manifest.runtime_dir,
            PathBuf::from("openclaw").join("versions").join("2026.3.2")
        );
        assert!(managed_runtime_manifest_path_from_base(
            test_dir.path(),
            ManagedRuntimeKind::Node,
            "20.18.0",
        )
        .exists());

        let reloaded = load_managed_runtime_manifest_from_base(
            test_dir.path(),
            ManagedRuntimeKind::OpenClaw,
            "2026.3.2",
        );
        assert_eq!(reloaded, openclaw_manifest);
    }

    #[test]
    fn managed_node_download_pipeline_stores_archive_under_runtime_downloads() {
        let test_dir = TestDir::new("managed-node-download");
        let archive_name = "node-v24.8.0-test.zip";
        let source_archive = create_fake_node_archive(test_dir.path(), "24.8.0", archive_name);
        let archive_bytes = fs::read(&source_archive).expect("read source archive");
        let archive_sha256 = sha256_digest_file(&source_archive).expect("sha256 digest");
        let archive_url = serve_http_bytes_once(archive_name, archive_bytes);
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .build()
            .expect("build client");
        let payload = ManagedNodeInstallPayload {
            version: "24.8.0".into(),
            archive_url,
            sha256: archive_sha256,
        };

        let downloaded =
            download_managed_node_archive_with_client_in_base(test_dir.path(), &client, &payload)
                .expect("download managed node archive");

        assert_eq!(
            downloaded,
            managed_runtime_downloads_dir_from_base(test_dir.path()).join(archive_name)
        );
        assert!(downloaded.exists());
        assert_eq!(
            sha256_digest_file(&downloaded).expect("downloaded archive sha256"),
            payload.sha256
        );
    }

    #[test]
    fn managed_node_download_pipeline_enforces_archive_verification() {
        let test_dir = TestDir::new("managed-node-download-verify");
        let archive_name = "node-v24.8.0-test.zip";
        let source_archive = create_fake_node_archive(test_dir.path(), "24.8.0", archive_name);
        let archive_bytes = fs::read(&source_archive).expect("read source archive");
        let archive_url = serve_http_bytes_once(archive_name, archive_bytes);
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .build()
            .expect("build client");
        let payload = ManagedNodeInstallPayload {
            version: "24.8.0".into(),
            archive_url,
            sha256: "0000000000000000000000000000000000000000000000000000000000000000".into(),
        };

        let error =
            download_managed_node_archive_with_client_in_base(test_dir.path(), &client, &payload)
                .expect_err("checksum mismatch should fail");

        assert!(error.contains("checksum mismatch"));
        assert!(!managed_runtime_downloads_dir_from_base(test_dir.path())
            .join(archive_name)
            .exists());
    }

    #[cfg(not(windows))]
    #[test]
    fn managed_node_install_pipeline_stages_promotes_and_activates_version() {
        let test_dir = TestDir::new("managed-node-install");
        let archive = create_fake_node_archive(test_dir.path(), "24.8.0", "node-v24.8.0-test.zip");

        let result = install_managed_node_archive_in_base(test_dir.path(), "24.8.0", &archive)
            .expect("install managed node archive");

        assert_eq!(
            result.runtime_dir,
            managed_runtime_version_dir_from_base(
                test_dir.path(),
                ManagedRuntimeKind::Node,
                "24.8.0"
            )
        );
        assert!(result.runtime_dir.exists());
        assert!(result.manifest_path.exists());
        assert_eq!(
            fs::read_to_string(&result.current_path)
                .expect("read current pointer")
                .trim(),
            "24.8.0"
        );
        assert!(managed_node_binary_path_for_version_from_base(test_dir.path(), "24.8.0").exists());

        let state = load_managed_runtime_state_from_base(test_dir.path());
        assert_eq!(
            state.node.current,
            Some(ManagedRuntimeVersionPointer::new(
                ManagedRuntimeKind::Node,
                "24.8.0",
            ))
        );
        assert_eq!(
            fs::read_dir(managed_runtime_staging_dir_from_base(
                test_dir.path(),
                ManagedRuntimeKind::Node,
            ))
            .expect("read staging dir")
            .count(),
            0
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn managed_node_switching_updates_current_pointer_and_resolver() {
        let test_dir = TestDir::new("managed-node-switch");
        let first_archive =
            create_fake_node_archive(test_dir.path(), "24.8.0", "node-v24.8.0-test.zip");
        let second_archive =
            create_fake_node_archive(test_dir.path(), "24.8.1", "node-v24.8.1-test.zip");

        install_managed_node_archive_in_base(test_dir.path(), "24.8.0", &first_archive)
            .expect("install first managed node");
        install_managed_node_archive_in_base(test_dir.path(), "24.8.1", &second_archive)
            .expect("install second managed node");
        switch_managed_node_version_in_base(test_dir.path(), "24.8.0")
            .expect("switch managed node version");

        let system_node = create_fake_node_binary(test_dir.path(), "system-node", "24.7.9", true);
        let bundled_node = create_fake_node_binary(test_dir.path(), "bundled-node", "24.8.2", true);
        let resolution = resolve_node_binary_with_candidates_and_managed(
            Some(system_node),
            managed_node_binary_path_from_base(test_dir.path()),
            bundled_node,
        );

        assert_eq!(resolution.source, Some(NodeBinarySource::Managed));
        assert_eq!(resolution.version.as_deref(), Some("24.8.0"));
        assert_eq!(resolution.diagnostics.len(), 2);
        assert_eq!(
            fs::read_to_string(managed_runtime_current_pointer_path_from_base(
                test_dir.path(),
                ManagedRuntimeKind::Node,
            ))
            .expect("read current pointer")
            .trim(),
            "24.8.0"
        );
    }

    #[test]
    fn managed_openclaw_download_pipeline_resolves_version_and_verifies_archive() {
        let test_dir = TestDir::new("managed-openclaw-download");
        let version = "2026.3.2";
        let archive_name = "openclaw-2026.3.2.tgz";
        let source_archive = create_fake_openclaw_archive(test_dir.path(), version, archive_name);
        let archive_bytes = fs::read(&source_archive).expect("read source openclaw archive");
        let integrity = format!("sha512-{}", STANDARD.encode(Sha512::digest(&archive_bytes)));
        let metadata_path = format!("/{OPENCLAW_PACKAGE_NAME}/{version}");
        let archive_path = format!("/{OPENCLAW_PACKAGE_NAME}/-/{archive_name}");
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("bind openclaw registry server");
        let port = listener.local_addr().expect("registry addr").port();
        let registry_url = format!("http://127.0.0.1:{port}");
        let routes = std::sync::Arc::new(std::sync::Mutex::new(
            vec![
                (
                    metadata_path,
                    (
                        "application/json".to_string(),
                        serde_json::to_vec(&json!({
                            "name": OPENCLAW_PACKAGE_NAME,
                            "version": version,
                            "dist": {
                                "tarball": format!("{registry_url}{archive_path}"),
                                "integrity": integrity.clone(),
                            },
                        }))
                        .expect("serialize registry metadata"),
                    ),
                ),
                (
                    archive_path,
                    ("application/octet-stream".to_string(), archive_bytes),
                ),
            ]
            .into_iter()
            .collect::<std::collections::HashMap<_, _>>(),
        ));
        let server_routes = routes.clone();
        std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept registry request");
                let mut request = [0_u8; 4096];
                let read = stream.read(&mut request).expect("read registry request");
                let request_line = String::from_utf8_lossy(&request[..read]);
                let path = request_line
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/")
                    .to_string();
                let (content_type, body) = server_routes
                    .lock()
                    .expect("lock registry routes")
                    .remove(&path)
                    .expect("expected route");
                let headers = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream
                    .write_all(headers.as_bytes())
                    .expect("write registry response headers");
                stream
                    .write_all(&body)
                    .expect("write registry response body");
            }
        });
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .build()
            .expect("build client");

        let downloaded = download_managed_openclaw_archive_with_client_in_base_and_registry(
            test_dir.path(),
            &client,
            version,
            &registry_url,
        )
        .expect("download managed openclaw archive");

        assert_eq!(
            downloaded,
            managed_runtime_downloads_dir_from_base(test_dir.path()).join(archive_name)
        );
        assert!(downloaded.exists());
        assert!(verify_file_integrity(&downloaded, &integrity).is_ok());
    }

    #[test]
    fn managed_openclaw_install_pipeline_stages_promotes_and_activates_version() {
        let test_dir = TestDir::new("managed-openclaw-install");
        let version = "2026.3.2";
        let archive =
            create_fake_openclaw_archive(test_dir.path(), version, "openclaw-2026.3.2.tgz");

        let result = install_managed_openclaw_archive_in_base(test_dir.path(), version, &archive)
            .expect("install managed openclaw archive");

        assert_eq!(
            result.runtime_dir,
            managed_runtime_version_dir_from_base(
                test_dir.path(),
                ManagedRuntimeKind::OpenClaw,
                version,
            )
        );
        assert!(result.runtime_dir.exists());
        assert!(result.manifest_path.exists());
        assert_eq!(
            fs::read_to_string(&result.current_path)
                .expect("read current pointer")
                .trim(),
            version
        );
        assert!(
            managed_openclaw_entry_path_for_version_from_base(test_dir.path(), version).exists()
        );

        let state = load_managed_runtime_state_from_base(test_dir.path());
        assert_eq!(
            state.openclaw.current,
            Some(ManagedRuntimeVersionPointer::new(
                ManagedRuntimeKind::OpenClaw,
                version,
            ))
        );
        assert_eq!(
            managed_openclaw_dir_from_base(test_dir.path()),
            Some(managed_openclaw_dir_for_version_from_base(
                test_dir.path(),
                version
            ))
        );
        assert_eq!(
            fs::read_dir(managed_runtime_staging_dir_from_base(
                test_dir.path(),
                ManagedRuntimeKind::OpenClaw,
            ))
            .expect("read openclaw staging dir")
            .count(),
            0
        );
    }

    #[test]
    fn managed_openclaw_install_failure_does_not_replace_working_runtime() {
        let test_dir = TestDir::new("managed-openclaw-install-failure");
        let working_version = "2026.3.2";
        let broken_version = "2026.3.3";
        let working_archive =
            create_fake_openclaw_archive(test_dir.path(), working_version, "openclaw-2026.3.2.tgz");
        let broken_archive = create_fake_openclaw_archive_with_options(
            test_dir.path(),
            broken_version,
            "openclaw-2026.3.3.tgz",
            false,
            true,
            OPENCLAW_PACKAGE_NAME,
        );

        install_managed_openclaw_archive_in_base(
            test_dir.path(),
            working_version,
            &working_archive,
        )
        .expect("install working managed openclaw");
        let error = install_managed_openclaw_archive_in_base(
            test_dir.path(),
            broken_version,
            &broken_archive,
        )
        .expect_err("broken openclaw archive should fail");

        assert!(error.contains("openclaw.mjs"));
        assert_eq!(
            fs::read_to_string(managed_runtime_current_pointer_path_from_base(
                test_dir.path(),
                ManagedRuntimeKind::OpenClaw,
            ))
            .expect("read current pointer")
            .trim(),
            working_version
        );
        assert_eq!(
            managed_openclaw_dir_from_base(test_dir.path()),
            Some(managed_openclaw_dir_for_version_from_base(
                test_dir.path(),
                working_version,
            ))
        );
        assert!(!managed_runtime_version_dir_from_base(
            test_dir.path(),
            ManagedRuntimeKind::OpenClaw,
            broken_version,
        )
        .exists());
    }

    #[test]
    fn managed_openclaw_switching_updates_current_pointer_and_resolver() {
        let test_dir = TestDir::new("managed-openclaw-switch");
        let first_version = "2026.3.2";
        let second_version = "2026.3.3";
        let first_archive =
            create_fake_openclaw_archive(test_dir.path(), first_version, "openclaw-2026.3.2.tgz");
        let second_archive =
            create_fake_openclaw_archive(test_dir.path(), second_version, "openclaw-2026.3.3.tgz");

        install_managed_openclaw_archive_in_base(test_dir.path(), first_version, &first_archive)
            .expect("install first managed openclaw");
        install_managed_openclaw_archive_in_base(test_dir.path(), second_version, &second_archive)
            .expect("install second managed openclaw");
        switch_managed_openclaw_version_in_base(test_dir.path(), first_version)
            .expect("switch managed openclaw version");

        assert_eq!(
            fs::read_to_string(managed_runtime_current_pointer_path_from_base(
                test_dir.path(),
                ManagedRuntimeKind::OpenClaw,
            ))
            .expect("read current pointer")
            .trim(),
            first_version
        );
        assert_eq!(
            managed_openclaw_dir_from_base(test_dir.path()),
            Some(managed_openclaw_dir_for_version_from_base(
                test_dir.path(),
                first_version,
            ))
        );
    }

    #[test]
    fn node_binary_resolution_prefers_supported_system_node() {
        let test_dir = TestDir::new("node-resolution-system");
        let system_node = create_fake_node_binary(test_dir.path(), "system-node", "24.8.0", true);
        let bundled_node = create_fake_node_binary(test_dir.path(), "bundled-node", "24.9.0", true);

        let resolution =
            resolve_node_binary_with_candidates(Some(system_node.clone()), bundled_node);

        assert_eq!(resolution.path, Some(system_node.clone()));
        assert_eq!(resolution.source, Some(NodeBinarySource::Path));
        assert_eq!(resolution.version.as_deref(), Some("24.8.0"));
        assert_eq!(resolution.diagnostics.len(), 1);
        assert_eq!(
            resolution.diagnostics[0].status,
            NodeBinaryDiagnosticStatus::Accepted
        );
        assert_eq!(
            resolution.diagnostics[0].path.as_deref(),
            Some(system_node.as_path())
        );
    }

    #[test]
    fn node_binary_resolution_rejects_unsupported_system_node_and_falls_back() {
        let test_dir = TestDir::new("node-resolution-fallback-version");
        let system_node = create_fake_node_binary(test_dir.path(), "system-node", "24.7.9", true);
        let bundled_node = create_fake_node_binary(test_dir.path(), "bundled-node", "24.8.0", true);

        let resolution =
            resolve_node_binary_with_candidates(Some(system_node.clone()), bundled_node.clone());

        assert_eq!(resolution.path, Some(bundled_node.clone()));
        assert_eq!(resolution.source, Some(NodeBinarySource::Bundled));
        assert_eq!(resolution.diagnostics.len(), 2);
        assert_eq!(
            resolution.diagnostics[0].reason,
            Some(NodeBinaryDiagnosticReason::UnsupportedVersion)
        );
        assert_eq!(resolution.diagnostics[0].version.as_deref(), Some("24.7.9"));
        assert_eq!(
            resolution.diagnostics[1].status,
            NodeBinaryDiagnosticStatus::Accepted
        );
        assert_eq!(
            resolution.diagnostics[1].path.as_deref(),
            Some(bundled_node.as_path())
        );
    }

    #[test]
    fn node_binary_resolution_rejects_smoke_test_failures() {
        let test_dir = TestDir::new("node-resolution-smoke");
        let system_node = create_fake_node_binary(test_dir.path(), "system-node", "24.8.0", false);
        let bundled_node = create_fake_node_binary(test_dir.path(), "bundled-node", "24.8.1", true);

        let resolution =
            resolve_node_binary_with_candidates(Some(system_node), bundled_node.clone());

        assert_eq!(resolution.path, Some(bundled_node));
        assert_eq!(
            resolution.diagnostics[0].reason,
            Some(NodeBinaryDiagnosticReason::SmokeTestFailed)
        );
        assert_eq!(
            resolution.diagnostics[1].status,
            NodeBinaryDiagnosticStatus::Accepted
        );
    }

    #[test]
    fn node_binary_resolution_reports_missing_candidates() {
        let test_dir = TestDir::new("node-resolution-missing");
        let missing_bundled = fake_node_script_path(test_dir.path(), "missing-node");

        let resolution = resolve_node_binary_with_candidates(None, missing_bundled.clone());

        assert!(resolution.path.is_none());
        assert_eq!(resolution.diagnostics.len(), 2);
        assert_eq!(
            resolution.diagnostics[0].status,
            NodeBinaryDiagnosticStatus::Missing
        );
        assert_eq!(
            resolution.diagnostics[0].reason,
            Some(NodeBinaryDiagnosticReason::NotFoundInPath)
        );
        assert_eq!(
            resolution.diagnostics[1].status,
            NodeBinaryDiagnosticStatus::Rejected
        );
        assert_eq!(
            resolution.diagnostics[1].reason,
            Some(NodeBinaryDiagnosticReason::PathDoesNotExist)
        );
        assert_eq!(
            resolution.diagnostics[1].path.as_deref(),
            Some(missing_bundled.as_path())
        );
        assert!(resolution.failure_message().contains("NotFoundInPath"));
    }

    #[test]
    fn node_binary_resolution_prefers_bundled_when_full_mode_enabled() {
        let test_dir = TestDir::new("node-resolution-full-mode");
        let system_node = create_fake_node_binary(test_dir.path(), "system-node", "24.8.0", true);
        let bundled_node = create_fake_node_binary(test_dir.path(), "bundled-node", "24.8.1", true);

        let resolution = resolve_node_binary_with_candidates_and_preference(
            Some(system_node),
            bundled_node.clone(),
            true,
        );

        assert_eq!(resolution.path, Some(bundled_node));
        assert_eq!(resolution.source, Some(NodeBinarySource::Bundled));
        assert_eq!(resolution.version.as_deref(), Some("24.8.1"));
        assert_eq!(resolution.diagnostics.len(), 1);
        assert_eq!(
            resolution.diagnostics[0].status,
            NodeBinaryDiagnosticStatus::Accepted
        );
    }

    #[test]
    fn openclaw_runtime_resolution_prefers_managed_before_workspace_and_bundled() {
        let test_dir = TestDir::new("openclaw-resolution-managed");
        let managed_dir =
            create_fake_openclaw_runtime_dir(test_dir.path(), "managed-openclaw", "2026.3.2", true);
        let workspace_dir = create_fake_openclaw_runtime_dir(
            test_dir.path(),
            "workspace-openclaw",
            "2026.3.1",
            true,
        );
        let bundled_dir =
            create_fake_openclaw_runtime_dir(test_dir.path(), "bundled-openclaw", "2026.3.0", true);

        let resolution = resolve_openclaw_runtime_with_candidates(
            Some(managed_dir.clone()),
            workspace_dir,
            bundled_dir,
            false,
        );

        assert_eq!(resolution.dir, Some(managed_dir));
        assert_eq!(resolution.source, Some(OpenClawRuntimeSource::Managed));
        assert_eq!(resolution.version.as_deref(), Some("2026.3.2"));
        assert_eq!(resolution.diagnostics.len(), 1);
        assert!(matches!(
            resolution.diagnostics[0].status,
            OpenClawRuntimeDiagnosticStatus::Accepted
        ));
    }

    #[test]
    fn openclaw_runtime_resolution_prefers_bundled_when_full_mode_enabled() {
        let test_dir = TestDir::new("openclaw-resolution-full-mode");
        let workspace_dir = create_fake_openclaw_runtime_dir(
            test_dir.path(),
            "workspace-openclaw",
            "2026.3.1",
            true,
        );
        let bundled_dir =
            create_fake_openclaw_runtime_dir(test_dir.path(), "bundled-openclaw", "2026.3.2", true);

        let resolution = resolve_openclaw_runtime_with_candidates(
            None,
            workspace_dir,
            bundled_dir.clone(),
            true,
        );

        assert_eq!(resolution.dir, Some(bundled_dir));
        assert_eq!(resolution.source, Some(OpenClawRuntimeSource::Bundled));
        assert_eq!(resolution.version.as_deref(), Some("2026.3.2"));
        assert_eq!(resolution.diagnostics.len(), 1);
        assert!(matches!(
            resolution.diagnostics[0].status,
            OpenClawRuntimeDiagnosticStatus::Accepted
        ));
    }
}
