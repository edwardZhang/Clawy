use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path as FsPath, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::auth::RequestContext;
use super::node;
use super::response::{success, ApiError};
use super::server::BridgeAppState;

const DEFAULT_HISTORY_LIMIT: usize = 100;
const MAX_HISTORY_LIMIT: usize = 500;

#[derive(Debug, Deserialize, Default)]
pub(crate) struct SessionHistoryQuery {
    limit: Option<usize>,
    before: Option<String>,
    after: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
struct SessionSummary {
    session_id: String,
    display_name: String,
    state: String,
    last_activity_at: Option<String>,
    thinking_level: Option<String>,
    model: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
struct BridgeUsage {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    total_tokens: u64,
    cost_usd: f64,
}

#[derive(Debug, Serialize, Clone)]
struct BridgeContentBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_text: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
struct BridgeMessage {
    message_id: String,
    run_id: Option<String>,
    role: String,
    created_at: String,
    stop_reason: Option<String>,
    content: Vec<BridgeContentBlock>,
    usage: Option<BridgeUsage>,
}

#[derive(Debug, Serialize)]
struct SessionListData {
    sessions: Vec<SessionSummary>,
}

#[derive(Debug, Serialize)]
struct SessionDetailData {
    session: SessionSummary,
}

#[derive(Debug, Serialize)]
struct SessionHistoryData {
    session_id: String,
    messages: Vec<BridgeMessage>,
    paging: SessionHistoryPaging,
}

#[derive(Debug, Serialize)]
struct SessionHistoryPaging {
    limit: usize,
    before: Option<String>,
    after: Option<String>,
    has_more_before: bool,
    has_more_after: bool,
}

#[derive(Debug, Clone)]
struct SessionRecord {
    session_id: String,
    display_name: String,
    updated_at_ms: Option<i128>,
    updated_at: Option<String>,
    model_hint: Option<String>,
    aborted_last_run: bool,
    transcript_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct ParsedHistoryQuery {
    limit: usize,
    before: Option<String>,
    before_ms: Option<i128>,
    after: Option<String>,
    after_ms: Option<i128>,
}

#[derive(Debug, Clone)]
struct BridgeMessageRecord {
    created_at_ms: i128,
    message: BridgeMessage,
}

#[derive(Debug, Clone, Default)]
struct TranscriptMeta {
    messages: Vec<BridgeMessageRecord>,
    last_model: Option<String>,
    last_thinking_level: Option<String>,
    last_activity_at: Option<String>,
}

pub(crate) async fn session_list_handler(
    State(state): State<BridgeAppState>,
    Extension(context): Extension<RequestContext>,
) -> Response {
    if let Err(response) = node::ensure_runtime_ready(&state, &context) {
        return response;
    }

    match load_session_records(&state.config.openclaw_config_dir) {
        Ok(records) => {
            let mut sessions = records
                .into_iter()
                .map(|record| {
                    let transcript = load_transcript_meta(record.transcript_path.as_deref()).ok();
                    build_session_summary(&record, transcript.as_ref())
                })
                .collect::<Vec<_>>();

            sessions.sort_by(|left, right| {
                let left_ts = left.last_activity_at.as_deref();
                let right_ts = right.last_activity_at.as_deref();
                match (left_ts, right_ts) {
                    (Some(left_ts), Some(right_ts)) => right_ts.cmp(left_ts),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => left.session_id.cmp(&right.session_id),
                }
            });

            success(
                StatusCode::OK,
                &context.request_id,
                SessionListData { sessions },
            )
        }
        Err(error) => ApiError::custom(
            StatusCode::SERVICE_UNAVAILABLE,
            "OPENCLAW_UNREACHABLE",
            "OpenClaw session store is not reachable",
            Some(error),
            "openclaw",
            true,
            &context,
        )
        .into_response(),
    }
}

pub(crate) async fn session_detail_handler(
    Path(session_id): Path<String>,
    State(state): State<BridgeAppState>,
    Extension(context): Extension<RequestContext>,
) -> Response {
    if let Err(response) = node::ensure_runtime_ready(&state, &context) {
        return response;
    }

    let session_id = match validate_session_id(&session_id, &context) {
        Ok(session_id) => session_id,
        Err(error) => return error.into_response(),
    };

    let record = match find_session_record(&state.config.openclaw_config_dir, &session_id, &context)
    {
        Ok(record) => record,
        Err(response) => return response,
    };

    let transcript = load_transcript_meta(record.transcript_path.as_deref()).ok();
    success(
        StatusCode::OK,
        &context.request_id,
        SessionDetailData {
            session: build_session_summary(&record, transcript.as_ref()),
        },
    )
}

pub(crate) async fn session_history_handler(
    Path(session_id): Path<String>,
    Query(query): Query<SessionHistoryQuery>,
    State(state): State<BridgeAppState>,
    Extension(context): Extension<RequestContext>,
) -> Response {
    if let Err(response) = node::ensure_runtime_ready(&state, &context) {
        return response;
    }

    let session_id = match validate_session_id(&session_id, &context) {
        Ok(session_id) => session_id,
        Err(error) => return error.into_response(),
    };
    let query = match parse_history_query(query, &context) {
        Ok(query) => query,
        Err(error) => return error.into_response(),
    };

    let record = match find_session_record(&state.config.openclaw_config_dir, &session_id, &context)
    {
        Ok(record) => record,
        Err(response) => return response,
    };

    let transcript_path = match record.transcript_path.as_deref() {
        Some(path) => path,
        None => {
            return ApiError::custom(
                StatusCode::SERVICE_UNAVAILABLE,
                "OPENCLAW_UNREACHABLE",
                "OpenClaw transcript is not available for this session",
                Some(format!(
                    "Session `{}` does not expose a readable transcript file.",
                    session_id
                )),
                "openclaw",
                true,
                &context,
            )
            .into_response();
        }
    };

    let transcript = match load_transcript_meta(Some(transcript_path)) {
        Ok(transcript) => transcript,
        Err(error) => {
            return ApiError::custom(
                StatusCode::SERVICE_UNAVAILABLE,
                "OPENCLAW_UNREACHABLE",
                "OpenClaw transcript could not be read",
                Some(error),
                "openclaw",
                true,
                &context,
            )
            .into_response();
        }
    };

    let filtered = transcript
        .messages
        .iter()
        .filter(|record| {
            query
                .before_ms
                .map(|before_ms| record.created_at_ms < before_ms)
                .unwrap_or(true)
                && query
                    .after_ms
                    .map(|after_ms| record.created_at_ms > after_ms)
                    .unwrap_or(true)
        })
        .cloned()
        .collect::<Vec<_>>();

    let selected = if filtered.len() <= query.limit {
        filtered.clone()
    } else if query.after_ms.is_some() && query.before_ms.is_none() {
        filtered[..query.limit].to_vec()
    } else {
        filtered[filtered.len().saturating_sub(query.limit)..].to_vec()
    };

    let (has_more_before, has_more_after) = match (selected.first(), selected.last()) {
        (Some(first), Some(last)) => (
            transcript
                .messages
                .iter()
                .any(|record| record.created_at_ms < first.created_at_ms),
            transcript
                .messages
                .iter()
                .any(|record| record.created_at_ms > last.created_at_ms),
        ),
        _ => (false, false),
    };

    success(
        StatusCode::OK,
        &context.request_id,
        SessionHistoryData {
            session_id,
            messages: selected.into_iter().map(|record| record.message).collect(),
            paging: SessionHistoryPaging {
                limit: query.limit,
                before: query.before,
                after: query.after,
                has_more_before,
                has_more_after,
            },
        },
    )
}

fn validate_session_id(session_id: &str, context: &RequestContext) -> Result<String, ApiError> {
    if is_canonical_session_id(session_id) {
        Ok(session_id.to_string())
    } else {
        Err(session_not_found_error(session_id, context))
    }
}

fn parse_history_query(
    query: SessionHistoryQuery,
    context: &RequestContext,
) -> Result<ParsedHistoryQuery, ApiError> {
    let limit = query.limit.unwrap_or(DEFAULT_HISTORY_LIMIT);
    if !(1..=MAX_HISTORY_LIMIT).contains(&limit) {
        return Err(ApiError::custom(
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
            "Request is not valid for this route",
            Some(format!(
                "`limit` must be between 1 and {MAX_HISTORY_LIMIT}, got `{limit}`."
            )),
            "bridge",
            false,
            context,
        ));
    }

    let before_ms = parse_rfc3339_query("before", query.before.as_deref(), context)?;
    let after_ms = parse_rfc3339_query("after", query.after.as_deref(), context)?;

    if let (Some(after_ms), Some(before_ms)) = (after_ms, before_ms) {
        if after_ms >= before_ms {
            return Err(ApiError::custom(
                StatusCode::BAD_REQUEST,
                "INVALID_REQUEST",
                "Request is not valid for this route",
                Some("`after` must be earlier than `before`.".into()),
                "bridge",
                false,
                context,
            ));
        }
    }

    Ok(ParsedHistoryQuery {
        limit,
        before: query.before,
        before_ms,
        after: query.after,
        after_ms,
    })
}

fn parse_rfc3339_query(
    field_name: &str,
    value: Option<&str>,
    context: &RequestContext,
) -> Result<Option<i128>, ApiError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    if !value.ends_with('Z') {
        return Err(ApiError::custom(
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
            "Request is not valid for this route",
            Some(format!("`{field_name}` must be an RFC3339 UTC timestamp.")),
            "bridge",
            false,
            context,
        ));
    }

    OffsetDateTime::parse(value, &Rfc3339)
        .map(|value| value.unix_timestamp_nanos() / 1_000_000)
        .map(Some)
        .map_err(|error| {
            ApiError::custom(
                StatusCode::BAD_REQUEST,
                "INVALID_REQUEST",
                "Request is not valid for this route",
                Some(format!(
                    "Invalid `{field_name}` timestamp `{value}`: {error}"
                )),
                "bridge",
                false,
                context,
            )
        })
}

fn find_session_record(
    openclaw_config_dir: &FsPath,
    session_id: &str,
    context: &RequestContext,
) -> Result<SessionRecord, Response> {
    let records = load_session_records(openclaw_config_dir).map_err(|error| {
        ApiError::custom(
            StatusCode::SERVICE_UNAVAILABLE,
            "OPENCLAW_UNREACHABLE",
            "OpenClaw session store is not reachable",
            Some(error),
            "openclaw",
            true,
            context,
        )
        .into_response()
    })?;

    records
        .into_iter()
        .find(|record| record.session_id == session_id)
        .ok_or_else(|| session_not_found_error(session_id, context).into_response())
}

fn load_session_records(openclaw_config_dir: &FsPath) -> Result<Vec<SessionRecord>, String> {
    let agents_dir = openclaw_config_dir.join("agents");
    if !openclaw_config_dir.exists() {
        return Err(format!(
            "OpenClaw config dir `{}` does not exist.",
            openclaw_config_dir.display()
        ));
    }

    if !agents_dir.exists() {
        return Ok(Vec::new());
    }

    let mut deduped = HashMap::<String, SessionRecord>::new();
    let agent_entries = fs::read_dir(&agents_dir).map_err(|error| {
        format!(
            "Could not read OpenClaw agents dir `{}`: {error}",
            agents_dir.display()
        )
    })?;

    for agent_entry in agent_entries {
        let agent_entry = match agent_entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let agent_path = agent_entry.path();
        if !agent_path.is_dir() {
            continue;
        }

        let sessions_dir = agent_path.join("sessions");
        if !sessions_dir.exists() {
            continue;
        }

        let sessions_json_path = sessions_dir.join("sessions.json");
        if !sessions_json_path.exists() {
            continue;
        }

        let raw = fs::read_to_string(&sessions_json_path).map_err(|error| {
            format!(
                "Could not read sessions index `{}`: {error}",
                sessions_json_path.display()
            )
        })?;
        let payload: Value = serde_json::from_str(&raw).map_err(|error| {
            format!(
                "Invalid JSON in sessions index `{}`: {error}",
                sessions_json_path.display()
            )
        })?;

        let mut records = extract_records_from_payload(&sessions_dir, &payload);
        for record in records.drain(..) {
            match deduped.get(&record.session_id) {
                Some(existing)
                    if existing.updated_at_ms.unwrap_or(0) >= record.updated_at_ms.unwrap_or(0) => {
                }
                _ => {
                    deduped.insert(record.session_id.clone(), record);
                }
            }
        }
    }

    let mut records = deduped.into_values().collect::<Vec<_>>();
    records.sort_by(|left, right| left.session_id.cmp(&right.session_id));
    Ok(records)
}

fn extract_records_from_payload(sessions_dir: &FsPath, payload: &Value) -> Vec<SessionRecord> {
    let mut records = Vec::new();

    if let Some(entries) = payload.get("sessions").and_then(Value::as_array) {
        for entry in entries {
            let session_id = entry
                .get("sessionKey")
                .and_then(Value::as_str)
                .or_else(|| entry.get("key").and_then(Value::as_str));
            if let Some(record) = session_id
                .filter(|session_id| is_canonical_session_id(session_id))
                .and_then(|session_id| build_session_record(session_id, sessions_dir, entry))
            {
                records.push(record);
            }
        }
    }

    if let Some(object) = payload.as_object() {
        for (session_id, entry) in object {
            if !is_canonical_session_id(session_id) {
                continue;
            }
            if let Some(record) = build_session_record(session_id, sessions_dir, entry) {
                records.push(record);
            }
        }
    }

    records
}

fn build_session_record(
    session_id: &str,
    sessions_dir: &FsPath,
    value: &Value,
) -> Option<SessionRecord> {
    let updated_at_ms = value
        .get("updatedAt")
        .and_then(json_number_as_i128)
        .or_else(|| value.get("updatedAtMs").and_then(json_number_as_i128));
    let transcript_path = resolve_transcript_path(sessions_dir, value);

    Some(SessionRecord {
        session_id: session_id.to_string(),
        display_name: display_name_from_session_id(session_id),
        updated_at: updated_at_ms.and_then(ms_to_rfc3339),
        updated_at_ms,
        model_hint: value
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_string),
        aborted_last_run: value
            .get("abortedLastRun")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        transcript_path,
    })
}

fn resolve_transcript_path(sessions_dir: &FsPath, value: &Value) -> Option<PathBuf> {
    let candidate = value
        .get("sessionFile")
        .and_then(Value::as_str)
        .or_else(|| value.get("file").and_then(Value::as_str))
        .or_else(|| value.get("fileName").and_then(Value::as_str))
        .or_else(|| value.get("path").and_then(Value::as_str));

    if let Some(candidate) = candidate {
        let path = if FsPath::new(candidate).is_absolute() {
            PathBuf::from(candidate)
        } else {
            sessions_dir.join(candidate)
        };
        return Some(ensure_jsonl_suffix(path));
    }

    value
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| value.get("sessionId").and_then(Value::as_str))
        .map(|session_uuid| sessions_dir.join(format!("{session_uuid}.jsonl")))
}

fn ensure_jsonl_suffix(path: PathBuf) -> PathBuf {
    if path
        .file_name()
        .map(|name| name.to_string_lossy().ends_with(".jsonl"))
        .unwrap_or(false)
    {
        path
    } else {
        PathBuf::from(format!("{}.jsonl", path.to_string_lossy()))
    }
}

fn build_session_summary(
    record: &SessionRecord,
    transcript: Option<&TranscriptMeta>,
) -> SessionSummary {
    let state = if record.aborted_last_run {
        "aborted".to_string()
    } else {
        infer_session_state(transcript).unwrap_or_else(|| {
            if record.updated_at_ms.is_some() {
                "completed".into()
            } else {
                "idle".into()
            }
        })
    };

    SessionSummary {
        session_id: record.session_id.clone(),
        display_name: record.display_name.clone(),
        state,
        last_activity_at: transcript
            .and_then(|transcript| transcript.last_activity_at.clone())
            .or_else(|| record.updated_at.clone()),
        thinking_level: transcript.and_then(|transcript| transcript.last_thinking_level.clone()),
        model: transcript
            .and_then(|transcript| transcript.last_model.clone())
            .or_else(|| record.model_hint.clone()),
    }
}

fn infer_session_state(transcript: Option<&TranscriptMeta>) -> Option<String> {
    let transcript = transcript?;
    let last_message = transcript.messages.last()?;

    match last_message.message.stop_reason.as_deref() {
        Some("tool_call") => Some("running".into()),
        Some("error") => Some("error".into()),
        Some("aborted") => Some("aborted".into()),
        Some("completed") => Some("completed".into()),
        _ => match last_message.message.role.as_str() {
            "assistant" | "tool" => Some("completed".into()),
            _ => Some("idle".into()),
        },
    }
}

fn load_transcript_meta(transcript_path: Option<&FsPath>) -> Result<TranscriptMeta, String> {
    let Some(transcript_path) = transcript_path else {
        return Ok(TranscriptMeta::default());
    };

    let raw = fs::read_to_string(transcript_path).map_err(|error| {
        format!(
            "Could not read transcript `{}`: {error}",
            transcript_path.display()
        )
    })?;

    let mut messages = Vec::new();
    let mut last_model = None;
    let mut last_thinking_level = None;

    for (index, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let event: Value = serde_json::from_str(line).map_err(|error| {
            format!(
                "Invalid JSON on line {} in `{}`: {error}",
                index + 1,
                transcript_path.display()
            )
        })?;

        match event.get("type").and_then(Value::as_str) {
            Some("model_change") => {
                last_model = event
                    .get("modelId")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| last_model.take());
            }
            Some("thinking_level_change") => {
                last_thinking_level = event
                    .get("thinkingLevel")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| last_thinking_level.take());
            }
            Some("message") => {
                if let Some(record) = parse_message_record(index, &event) {
                    if record.message.role == "assistant" {
                        if let Some(model) = event
                            .get("message")
                            .and_then(|message| message.get("model"))
                            .and_then(Value::as_str)
                        {
                            last_model = Some(model.to_string());
                        }
                    }
                    messages.push(record);
                }
            }
            _ => {}
        }
    }

    let last_activity_at = messages
        .last()
        .map(|record| record.message.created_at.clone());
    Ok(TranscriptMeta {
        messages,
        last_model,
        last_thinking_level,
        last_activity_at,
    })
}

fn parse_message_record(index: usize, event: &Value) -> Option<BridgeMessageRecord> {
    let message = event.get("message")?;
    let raw_role = message.get("role").and_then(Value::as_str)?;
    let role = normalize_role(raw_role)?;
    let (created_at, created_at_ms) = extract_message_timestamp(event, message)?;
    let stop_reason = message
        .get("stopReason")
        .and_then(Value::as_str)
        .and_then(normalize_stop_reason);
    let content = normalize_content(raw_role, message, event);
    let usage = normalize_usage(message.get("usage"));
    let message_id = event
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| message.get("id").and_then(Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| format!("line_{}", index + 1));

    Some(BridgeMessageRecord {
        created_at_ms,
        message: BridgeMessage {
            message_id,
            run_id: extract_run_id(event, message),
            role: role.to_string(),
            created_at,
            stop_reason,
            content,
            usage,
        },
    })
}

fn normalize_role(raw_role: &str) -> Option<&'static str> {
    match raw_role {
        "user" => Some("user"),
        "assistant" => Some("assistant"),
        "system" => Some("system"),
        "tool" | "toolResult" | "toolresult" | "tool_result" => Some("tool"),
        _ => None,
    }
}

fn normalize_stop_reason(raw_stop_reason: &str) -> Option<String> {
    match raw_stop_reason {
        "stop" | "completed" => Some("completed".into()),
        "toolUse" | "tool_call" | "toolCall" => Some("tool_call".into()),
        "aborted" | "abort" | "cancelled" | "canceled" => Some("aborted".into()),
        "error" | "failed" => Some("error".into()),
        _ => None,
    }
}

fn normalize_content(raw_role: &str, message: &Value, event: &Value) -> Vec<BridgeContentBlock> {
    if matches!(
        raw_role,
        "toolResult" | "toolresult" | "tool_result" | "tool"
    ) {
        return vec![BridgeContentBlock {
            kind: "tool_result".into(),
            text: None,
            tool_call_id: message
                .get("toolCallId")
                .and_then(Value::as_str)
                .map(str::to_string),
            name: message
                .get("toolName")
                .and_then(Value::as_str)
                .map(str::to_string),
            arguments: None,
            status: extract_tool_result_status(message),
            output_text: extract_text(message.get("content")),
        }];
    }

    let content = message.get("content");
    if let Some(text) = content.and_then(Value::as_str) {
        return vec![BridgeContentBlock {
            kind: "text".into(),
            text: Some(text.to_string()),
            tool_call_id: None,
            name: None,
            arguments: None,
            status: None,
            output_text: None,
        }];
    }

    let mut blocks = Vec::new();
    if let Some(items) = content.and_then(Value::as_array) {
        for item in items {
            let Some(kind) = item.get("type").and_then(Value::as_str) else {
                continue;
            };

            match kind {
                "text" => blocks.push(BridgeContentBlock {
                    kind: "text".into(),
                    text: item.get("text").and_then(Value::as_str).map(str::to_string),
                    tool_call_id: None,
                    name: None,
                    arguments: None,
                    status: None,
                    output_text: None,
                }),
                "thinking" => blocks.push(BridgeContentBlock {
                    kind: "thinking".into(),
                    text: item
                        .get("text")
                        .and_then(Value::as_str)
                        .or_else(|| item.get("thinking").and_then(Value::as_str))
                        .map(str::to_string),
                    tool_call_id: None,
                    name: None,
                    arguments: None,
                    status: None,
                    output_text: None,
                }),
                "toolUse" | "tool_use" | "toolCall" | "tool_call" => {
                    blocks.push(BridgeContentBlock {
                        kind: "tool_call".into(),
                        text: None,
                        tool_call_id: item
                            .get("id")
                            .and_then(Value::as_str)
                            .or_else(|| item.get("toolCallId").and_then(Value::as_str))
                            .map(str::to_string),
                        name: item
                            .get("name")
                            .and_then(Value::as_str)
                            .or_else(|| item.get("toolName").and_then(Value::as_str))
                            .map(str::to_string),
                        arguments: item
                            .get("arguments")
                            .cloned()
                            .or_else(|| item.get("input").cloned())
                            .or_else(|| Some(Value::Object(Map::new()))),
                        status: None,
                        output_text: None,
                    })
                }
                "toolResult" | "tool_result" => blocks.push(BridgeContentBlock {
                    kind: "tool_result".into(),
                    text: None,
                    tool_call_id: item
                        .get("toolCallId")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    name: item
                        .get("name")
                        .and_then(Value::as_str)
                        .or_else(|| item.get("toolName").and_then(Value::as_str))
                        .map(str::to_string),
                    arguments: None,
                    status: item
                        .get("status")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    output_text: item
                        .get("outputText")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| extract_text(item.get("content"))),
                }),
                _ => {
                    if let Some(text) = item.get("text").and_then(Value::as_str) {
                        blocks.push(BridgeContentBlock {
                            kind: "text".into(),
                            text: Some(text.to_string()),
                            tool_call_id: None,
                            name: None,
                            arguments: None,
                            status: None,
                            output_text: None,
                        });
                    }
                }
            }
        }
    }

    if blocks.is_empty() {
        if let Some(text) = extract_text(Some(message)) {
            blocks.push(BridgeContentBlock {
                kind: "text".into(),
                text: Some(text),
                tool_call_id: None,
                name: None,
                arguments: None,
                status: None,
                output_text: None,
            });
        } else if let Some(text) = extract_text(Some(event)) {
            blocks.push(BridgeContentBlock {
                kind: "text".into(),
                text: Some(text),
                tool_call_id: None,
                name: None,
                arguments: None,
                status: None,
                output_text: None,
            });
        }
    }

    blocks
}

fn extract_run_id(event: &Value, message: &Value) -> Option<String> {
    event
        .get("runId")
        .and_then(Value::as_str)
        .or_else(|| message.get("runId").and_then(Value::as_str))
        .or_else(|| message.get("run_id").and_then(Value::as_str))
        .map(str::to_string)
}

fn extract_message_timestamp(event: &Value, message: &Value) -> Option<(String, i128)> {
    if let Some(timestamp_ms) = message.get("timestamp").and_then(json_number_as_i128) {
        let created_at = ms_to_rfc3339(timestamp_ms)?;
        return Some((created_at, timestamp_ms));
    }

    let created_at = event.get("timestamp").and_then(Value::as_str)?.to_string();
    let created_at_ms = OffsetDateTime::parse(&created_at, &Rfc3339)
        .ok()?
        .unix_timestamp_nanos()
        / 1_000_000;

    Some((created_at, created_at_ms))
}

fn normalize_usage(usage: Option<&Value>) -> Option<BridgeUsage> {
    let usage = usage?;
    let input_tokens = usage
        .get("input")
        .and_then(Value::as_u64)
        .or_else(|| usage.get("inputTokens").and_then(Value::as_u64))
        .unwrap_or(0);
    let output_tokens = usage
        .get("output")
        .and_then(Value::as_u64)
        .or_else(|| usage.get("outputTokens").and_then(Value::as_u64))
        .unwrap_or(0);
    let cache_read_tokens = usage
        .get("cacheRead")
        .and_then(Value::as_u64)
        .or_else(|| usage.get("cacheReadTokens").and_then(Value::as_u64))
        .unwrap_or(0);
    let cache_write_tokens = usage
        .get("cacheWrite")
        .and_then(Value::as_u64)
        .or_else(|| usage.get("cacheWriteTokens").and_then(Value::as_u64))
        .unwrap_or(0);
    let total_tokens = usage
        .get("totalTokens")
        .and_then(Value::as_u64)
        .unwrap_or(input_tokens + output_tokens + cache_read_tokens + cache_write_tokens);
    let cost_usd = usage
        .get("cost")
        .and_then(|cost| cost.get("total"))
        .and_then(Value::as_f64)
        .or_else(|| usage.get("costUsd").and_then(Value::as_f64))
        .unwrap_or(0.0);

    Some(BridgeUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        total_tokens,
        cost_usd,
    })
}

fn extract_text(value: Option<&Value>) -> Option<String> {
    let value = value?;
    match value {
        Value::String(text) => Some(text.to_string()),
        Value::Array(items) => {
            let parts = items
                .iter()
                .filter_map(|item| match item {
                    Value::String(text) => Some(text.to_string()),
                    Value::Object(_) => {
                        item.get("text").and_then(Value::as_str).map(str::to_string)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("\n"))
            }
        }
        Value::Object(object) => object
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

fn extract_tool_result_status(message: &Value) -> Option<String> {
    message
        .get("details")
        .and_then(|details| details.get("status"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            message
                .get("isError")
                .and_then(Value::as_bool)
                .map(|is_error| if is_error { "error" } else { "success" }.to_string())
        })
}

fn json_number_as_i128(value: &Value) -> Option<i128> {
    value
        .as_i64()
        .map(i128::from)
        .or_else(|| value.as_u64().map(|value| value as i128))
}

fn ms_to_rfc3339(timestamp_ms: i128) -> Option<String> {
    OffsetDateTime::from_unix_timestamp_nanos(timestamp_ms * 1_000_000)
        .ok()
        .and_then(|value| value.format(&Rfc3339).ok())
}

fn display_name_from_session_id(session_id: &str) -> String {
    session_id
        .splitn(3, ':')
        .nth(2)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| session_id.to_string())
}

fn is_canonical_session_id(session_id: &str) -> bool {
    let mut parts = session_id.splitn(3, ':');
    matches!(parts.next(), Some("agent"))
        && parts.next().map(|part| !part.is_empty()).unwrap_or(false)
        && parts.next().map(|part| !part.is_empty()).unwrap_or(false)
}

fn session_not_found_error(session_id: &str, context: &RequestContext) -> ApiError {
    ApiError::custom(
        StatusCode::NOT_FOUND,
        "SESSION_NOT_FOUND",
        "session_id not found",
        Some(format!("No canonical session matched `{session_id}`.")),
        "bridge",
        false,
        context,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::server::{spawn_server, BridgeRuntimeConfig, BridgeRuntimeHandle};
    use reqwest::{Client, StatusCode};
    use serde_json::json;
    use std::net::SocketAddr;
    use std::process::{Child, Command, Stdio};
    use std::time::Duration;
    use uuid::Uuid;

    fn test_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("clawy-bridge-{name}-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).expect("test dir should exist");
        path
    }

    fn test_client() -> Client {
        Client::builder()
            .no_proxy()
            .build()
            .expect("test client should build")
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

    fn spawn_dummy_gateway_process() -> Child {
        #[cfg(windows)]
        {
            Command::new("cmd")
                .args(["/C", "ping 127.0.0.1 -n 30 > nul"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("dummy process should spawn")
        }

        #[cfg(not(windows))]
        {
            Command::new("sleep")
                .arg("30")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("dummy process should spawn")
        }
    }

    async fn spawn_test_server_with_state(
        state: crate::BridgeState,
        config: BridgeRuntimeConfig,
    ) -> BridgeRuntimeHandle {
        let handle = spawn_server(None, state, config).expect("bridge server should start");
        tokio::time::sleep(Duration::from_millis(50)).await;
        handle
    }

    fn mark_gateway_running(state: &crate::BridgeState) {
        let child = spawn_dummy_gateway_process();
        {
            let mut runtime = state
                .gateway_runtime
                .lock()
                .expect("gateway runtime lock should work");
            runtime.child = Some(child);
            runtime.desired_running = true;
            runtime.started_at_ms = Some(crate::now_ms());
        }
        {
            let mut status = state
                .gateway_status
                .lock()
                .expect("gateway status lock should work");
            status.state = "running".into();
            status.error = None;
        }
    }

    fn stop_dummy_gateway(state: &crate::BridgeState) {
        let mut runtime = state
            .gateway_runtime
            .lock()
            .expect("gateway runtime lock should work");
        if let Some(child) = runtime.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        runtime.child = None;
        runtime.started_at_ms = None;
        runtime.desired_running = false;
    }

    fn write_sample_session_store(openclaw_config_dir: &FsPath) {
        let sessions_dir = openclaw_config_dir
            .join("agents")
            .join("main")
            .join("sessions");
        fs::create_dir_all(&sessions_dir).expect("sessions dir should exist");

        crate::write_json(
            &sessions_dir.join("sessions.json"),
            &json!({
                "agent:main:main": {
                    "sessionId": "11111111-1111-1111-1111-111111111111",
                    "updatedAt": 1773972120000i64,
                    "abortedLastRun": false,
                    "sessionFile": sessions_dir
                        .join("11111111-1111-1111-1111-111111111111.jsonl")
                        .to_string_lossy()
                        .to_string(),
                    "model": "gpt-5.4"
                }
            }),
        )
        .expect("sessions.json should be written");

        fs::write(
            sessions_dir.join("11111111-1111-1111-1111-111111111111.jsonl"),
            [
                r#"{"type":"session","id":"11111111-1111-1111-1111-111111111111","timestamp":"2026-03-20T02:18:00Z"}"#,
                r#"{"type":"model_change","timestamp":"2026-03-20T02:18:00Z","modelId":"gpt-5.4"}"#,
                r#"{"type":"thinking_level_change","timestamp":"2026-03-20T02:18:01Z","thinkingLevel":"medium"}"#,
                r#"{"type":"message","id":"msg_001","timestamp":"2026-03-20T02:20:00Z","message":{"role":"user","content":[{"type":"text","text":"hello"}],"timestamp":1773973200000}}"#,
                r#"{"type":"message","id":"msg_002","timestamp":"2026-03-20T02:21:00Z","message":{"role":"assistant","content":[{"type":"toolCall","id":"call_001","name":"read","arguments":{"path":"/repo/package.json"}}],"stopReason":"toolUse","timestamp":1773973260000,"usage":{"input":12,"output":4,"cacheRead":0,"cacheWrite":0,"totalTokens":16,"cost":{"total":0.0}}}}"#,
                r#"{"type":"message","id":"msg_003","timestamp":"2026-03-20T02:21:30Z","message":{"role":"toolResult","toolCallId":"call_001","toolName":"read","content":[{"type":"text","text":"read ok"}],"details":{"status":"success"},"isError":false,"timestamp":1773973290000}}"#,
                r#"{"type":"message","id":"msg_004","timestamp":"2026-03-20T02:22:00Z","message":{"role":"assistant","content":[{"type":"text","text":"done"}],"stopReason":"stop","timestamp":1773973320000,"usage":{"input":20,"output":8,"cacheRead":0,"cacheWrite":0,"totalTokens":28,"cost":{"total":0.1}}}}"#,
            ]
            .join("\n"),
        )
        .expect("transcript should be written");
    }

    #[tokio::test]
    async fn sessions_endpoints_return_canonical_schema() {
        let state = crate::BridgeState::default();
        mark_gateway_running(&state);

        let clawy_base_dir = test_dir("sessions-base");
        let openclaw_config_dir = test_dir("sessions-openclaw");
        write_sample_session_store(&openclaw_config_dir);

        let handle = spawn_test_server_with_state(
            state.clone(),
            test_config(clawy_base_dir, openclaw_config_dir.clone()),
        )
        .await;

        let list_response = test_client()
            .get(format!("http://{}/api/sessions", handle.local_addr))
            .header("Authorization", "Bearer bridge-test-token")
            .send()
            .await
            .expect("list request should succeed");
        assert_eq!(list_response.status(), StatusCode::OK);
        let list_body: Value = list_response.json().await.expect("list body should parse");
        assert_eq!(list_body["ok"], true);
        assert_eq!(
            list_body["data"]["sessions"][0]["session_id"],
            "agent:main:main"
        );
        assert_eq!(list_body["data"]["sessions"][0]["display_name"], "main");
        assert_eq!(list_body["data"]["sessions"][0]["thinking_level"], "medium");
        assert_eq!(list_body["data"]["sessions"][0]["model"], "gpt-5.4");
        assert_eq!(list_body["data"]["sessions"][0]["state"], "completed");

        let detail_response = test_client()
            .get(format!(
                "http://{}/api/sessions/agent%3Amain%3Amain",
                handle.local_addr
            ))
            .header("Authorization", "Bearer bridge-test-token")
            .send()
            .await
            .expect("detail request should succeed");
        assert_eq!(detail_response.status(), StatusCode::OK);
        let detail_body: Value = detail_response
            .json()
            .await
            .expect("detail body should parse");
        assert_eq!(
            detail_body["data"]["session"]["session_id"],
            "agent:main:main"
        );
        assert_eq!(detail_body["data"]["session"]["display_name"], "main");

        stop_dummy_gateway(&state);
    }

    #[tokio::test]
    async fn history_supports_limit_and_after() {
        let state = crate::BridgeState::default();
        mark_gateway_running(&state);

        let clawy_base_dir = test_dir("history-base");
        let openclaw_config_dir = test_dir("history-openclaw");
        write_sample_session_store(&openclaw_config_dir);

        let handle = spawn_test_server_with_state(
            state.clone(),
            test_config(clawy_base_dir, openclaw_config_dir),
        )
        .await;

        let response = test_client()
            .get(format!(
                "http://{}/api/sessions/agent%3Amain%3Amain/history?limit=2&after=2026-03-20T02:20:30Z",
                handle.local_addr
            ))
            .header("Authorization", "Bearer bridge-test-token")
            .send()
            .await
            .expect("history request should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = response.json().await.expect("history body should parse");
        assert_eq!(body["ok"], true);
        assert_eq!(body["data"]["paging"]["limit"], 2);
        assert_eq!(body["data"]["messages"].as_array().map(Vec::len), Some(2));
        assert_eq!(body["data"]["messages"][0]["message_id"], "msg_002");
        assert_eq!(body["data"]["messages"][0]["stop_reason"], "tool_call");
        assert_eq!(body["data"]["messages"][1]["message_id"], "msg_003");
        assert_eq!(body["data"]["messages"][1]["role"], "tool");
        assert_eq!(
            body["data"]["messages"][1]["content"][0]["type"],
            "tool_result"
        );

        stop_dummy_gateway(&state);
    }

    #[tokio::test]
    async fn history_rejects_invalid_bounds() {
        let state = crate::BridgeState::default();
        mark_gateway_running(&state);

        let clawy_base_dir = test_dir("history-invalid-base");
        let openclaw_config_dir = test_dir("history-invalid-openclaw");
        write_sample_session_store(&openclaw_config_dir);

        let handle = spawn_test_server_with_state(
            state.clone(),
            test_config(clawy_base_dir, openclaw_config_dir),
        )
        .await;

        let response = test_client()
            .get(format!(
                "http://{}/api/sessions/agent%3Amain%3Amain/history?after=2026-03-20T02:22:00Z&before=2026-03-20T02:22:00Z",
                handle.local_addr
            ))
            .header("Authorization", "Bearer bridge-test-token")
            .send()
            .await
            .expect("history request should succeed");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: Value = response.json().await.expect("history body should parse");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"]["code"], "INVALID_REQUEST");

        stop_dummy_gateway(&state);
    }

    #[tokio::test]
    async fn sessions_return_gateway_not_running_when_runtime_is_down() {
        let state = crate::BridgeState::default();
        let clawy_base_dir = test_dir("gateway-down-base");
        let openclaw_config_dir = test_dir("gateway-down-openclaw");
        write_sample_session_store(&openclaw_config_dir);

        let handle =
            spawn_test_server_with_state(state, test_config(clawy_base_dir, openclaw_config_dir))
                .await;

        let response = test_client()
            .get(format!("http://{}/api/sessions", handle.local_addr))
            .header("Authorization", "Bearer bridge-test-token")
            .send()
            .await
            .expect("list request should succeed");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body: Value = response.json().await.expect("list body should parse");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"]["code"], "GATEWAY_NOT_RUNNING");
    }
}
