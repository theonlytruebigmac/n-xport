//! Small helpers shared by the handlers.

use crate::models::*;
use tauri::{AppHandle, Emitter, Runtime};

pub(super) fn emit_log<R: Runtime>(app: &AppHandle<R>, level: &str, message: &str) {
    let _ = app.emit(
        "backend-log",
        LogMessage {
            level: level.to_string(),
            message: message.to_string(),
        },
    );
}

/// Helper: extract created resource id from an N-central JSON response,
/// trying common envelope shapes.
pub(super) fn extract_id(resp: &serde_json::Value, keys: &[&str]) -> i64 {
    let data = if resp.get("data").is_some() {
        &resp["data"]
    } else {
        resp
    };
    for k in keys {
        if let Some(id) = data.get(k).and_then(|v| v.as_i64()) {
            return id;
        }
        if let Some(id) = resp.get(k).and_then(|v| v.as_i64()) {
            return id;
        }
    }
    0
}
