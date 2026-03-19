use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;

const RECENT_EVENT_CACHE_LIMIT: usize = 512;

static LAST_RUNTIME_STATUS: OnceLock<Mutex<Option<BridgeEventEnvelope>>> = OnceLock::new();
static RECENT_EVENTS: OnceLock<Mutex<VecDeque<BridgeEventEnvelope>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct BridgeEventEnvelope {
    pub(crate) event_id: String,
    pub(crate) node_id: String,
    pub(crate) session_id: Option<String>,
    pub(crate) run_id: Option<String>,
    #[serde(rename = "type")]
    pub(crate) event_type: String,
    pub(crate) ts: String,
    pub(crate) payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct BridgeError {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) detail: Option<String>,
    pub(crate) source: String,
    pub(crate) retryable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct RuntimeStatusPayload {
    pub(crate) status: String,
    pub(crate) gateway_running: bool,
    pub(crate) openclaw_reachable: bool,
    pub(crate) detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct RuntimeErrorPayload {
    pub(crate) error: BridgeError,
    pub(crate) fatal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct SessionSummary {
    pub(crate) session_id: String,
    pub(crate) display_name: String,
    pub(crate) state: String,
    pub(crate) last_activity_at: String,
    pub(crate) thinking_level: Option<String>,
    pub(crate) model: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EventReplayCursorStatus {
    Disabled,
    Found,
    NotFound,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EventReplayBatch {
    pub(crate) cursor_status: EventReplayCursorStatus,
    pub(crate) events: Vec<BridgeEventEnvelope>,
}

fn runtime_status_cache() -> &'static Mutex<Option<BridgeEventEnvelope>> {
    LAST_RUNTIME_STATUS.get_or_init(|| Mutex::new(None))
}

fn recent_event_cache() -> &'static Mutex<VecDeque<BridgeEventEnvelope>> {
    RECENT_EVENTS.get_or_init(|| Mutex::new(VecDeque::with_capacity(RECENT_EVENT_CACHE_LIMIT)))
}

pub(crate) fn new_event(
    node_id: &str,
    session_id: Option<String>,
    run_id: Option<String>,
    event_type: &str,
    payload: Value,
) -> BridgeEventEnvelope {
    BridgeEventEnvelope {
        event_id: format!("evt_{}", Uuid::new_v4().simple()),
        node_id: node_id.to_string(),
        session_id,
        run_id,
        event_type: event_type.to_string(),
        ts: crate::now_iso_string(),
        payload,
    }
}

pub(crate) fn bridge_error(
    code: impl Into<String>,
    message: impl Into<String>,
    detail: Option<String>,
    source: impl Into<String>,
    retryable: bool,
) -> BridgeError {
    BridgeError {
        code: code.into(),
        message: message.into(),
        detail,
        source: source.into(),
        retryable,
    }
}

pub(crate) fn runtime_status_payload(
    status: impl Into<String>,
    gateway_running: bool,
    openclaw_reachable: bool,
    detail: Option<String>,
) -> Value {
    serde_json::to_value(RuntimeStatusPayload {
        status: status.into(),
        gateway_running,
        openclaw_reachable,
        detail,
    })
    .unwrap_or_else(|_| {
        json!({
            "status": "unavailable",
            "gateway_running": gateway_running,
            "openclaw_reachable": openclaw_reachable,
            "detail": "failed to serialize runtime status payload",
        })
    })
}

pub(crate) fn runtime_error_payload(error: BridgeError, fatal: bool) -> Value {
    serde_json::to_value(RuntimeErrorPayload { error, fatal }).unwrap_or_else(|_| {
        json!({
            "error": {
                "code": "INTERNAL_ERROR",
                "message": "Failed to serialize runtime error payload",
                "detail": null,
                "source": "bridge",
                "retryable": false,
            },
            "fatal": fatal,
        })
    })
}

pub(crate) fn session_summary_payload(summary: SessionSummary) -> Value {
    serde_json::to_value(json!({ "session": summary }))
        .unwrap_or_else(|_| json!({ "session": Value::Null }))
}

pub(crate) fn publish_event(event: BridgeEventEnvelope) {
    remember_recent_event(&event);
    if event.event_type == "runtime.status" {
        remember_runtime_status(&event);
    }

    if let Some(sender) = super::bootstrap::bridge_events_sender() {
        let _ = sender.send(event);
    }
}

pub(crate) fn cached_runtime_status() -> Option<BridgeEventEnvelope> {
    runtime_status_cache()
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
}

pub(crate) fn replay_events_after(last_event_id: Option<&str>) -> EventReplayBatch {
    let Some(last_event_id) = last_event_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return EventReplayBatch {
            cursor_status: EventReplayCursorStatus::Disabled,
            events: Vec::new(),
        };
    };

    let Ok(guard) = recent_event_cache().lock() else {
        return EventReplayBatch {
            cursor_status: EventReplayCursorStatus::NotFound,
            events: Vec::new(),
        };
    };
    let Some(position) = guard
        .iter()
        .position(|event| event.event_id == last_event_id)
    else {
        return EventReplayBatch {
            cursor_status: EventReplayCursorStatus::NotFound,
            events: Vec::new(),
        };
    };

    EventReplayBatch {
        cursor_status: EventReplayCursorStatus::Found,
        events: guard.iter().skip(position + 1).cloned().collect(),
    }
}

pub(crate) fn remember_recent_event(event: &BridgeEventEnvelope) {
    if let Ok(mut guard) = recent_event_cache().lock() {
        guard.push_back(event.clone());
        while guard.len() > RECENT_EVENT_CACHE_LIMIT {
            guard.pop_front();
        }
    }
}

fn remember_runtime_status(event: &BridgeEventEnvelope) {
    if let Ok(mut guard) = runtime_status_cache().lock() {
        *guard = Some(event.clone());
    }
}

#[cfg(test)]
pub(crate) fn reset_runtime_status_cache() {
    if let Ok(mut guard) = runtime_status_cache().lock() {
        *guard = None;
    }
    if let Ok(mut guard) = recent_event_cache().lock() {
        guard.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replays_events_after_known_cursor() {
        reset_runtime_status_cache();

        let first = new_event(
            "node_test",
            Some("agent:main:main".into()),
            None,
            "message.delta",
            json!({ "delta": "1" }),
        );
        let second = new_event(
            "node_test",
            Some("agent:main:main".into()),
            None,
            "message.delta",
            json!({ "delta": "2" }),
        );
        remember_recent_event(&first);
        remember_recent_event(&second);

        let replay = replay_events_after(Some(&first.event_id));
        assert_eq!(replay.cursor_status, EventReplayCursorStatus::Found);
        assert_eq!(replay.events.len(), 1);
        assert_eq!(replay.events[0].event_id, second.event_id);
    }

    #[test]
    fn reports_missing_cursor_when_not_in_cache() {
        reset_runtime_status_cache();

        let replay = replay_events_after(Some("evt_missing"));
        assert_eq!(replay.cursor_status, EventReplayCursorStatus::NotFound);
        assert!(replay.events.is_empty());
    }
}
