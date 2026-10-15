//! The environment every migration step runs in, and the progress, logging, and cancellation helpers.

use crate::api::client::NcClient;
use crate::api::NcSoapClient;
use crate::models::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Runtime};

/// What every migration step needs: both servers, the service-org pair, the optional
/// SOAP client for the destination, progress reporting, and cancellation.
pub(crate) struct MigrationEnv<'a, R: Runtime> {
    pub(crate) source: &'a NcClient,
    pub(crate) dest: &'a NcClient,
    pub(crate) source_so_id: i64,
    pub(crate) dest_so_id: i64,
    pub(crate) soap: Option<&'a NcSoapClient>,
    pub(crate) app_handle: &'a AppHandle<R>,
    pub(crate) cancel_token: &'a Arc<AtomicBool>,
}

// Everything in the struct is a reference or a plain value, so it is copyable for any
// runtime; a derive would wrongly require `R: Copy`.
impl<R: Runtime> Clone for MigrationEnv<'_, R> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<R: Runtime> Copy for MigrationEnv<'_, R> {}

/// Build an environment for tests: service orgs 1 → 2, with the given clients.
#[cfg(test)]
pub(super) fn test_env<'a>(
    source: &'a NcClient,
    dest: &'a NcClient,
    soap: Option<&'a NcSoapClient>,
    app_handle: &'a AppHandle<tauri::test::MockRuntime>,
    cancel_token: &'a Arc<AtomicBool>,
) -> MigrationEnv<'a, tauri::test::MockRuntime> {
    MigrationEnv {
        source,
        dest,
        source_so_id: 1,
        dest_so_id: 2,
        soap,
        app_handle,
        cancel_token,
    }
}

/// Helper to emit progress updates
pub(super) fn report_progress<R: Runtime>(
    app_handle: &AppHandle<R>,
    phase: &str,
    message: &str,
    percent: f32,
) {
    let _ = app_handle.emit(
        "export-progress",
        ProgressUpdate {
            phase: phase.to_string(),
            message: message.to_string(),
            percent,
            current: 0,
            total: 100,
        },
    );
}

/// Helper to emit log messages to the frontend Activity Log
pub(super) fn emit_log<R: Runtime>(app_handle: &AppHandle<R>, level: &str, message: &str) {
    let _ = app_handle.emit(
        "backend-log",
        LogMessage {
            level: level.to_string(),
            message: message.to_string(),
        },
    );
}

pub(super) fn items_until_cancelled<'a, T: 'a>(
    items: Vec<T>,
    cancel_token: &'a AtomicBool,
) -> impl Iterator<Item = T> + 'a {
    items
        .into_iter()
        .take_while(move |_| !cancel_token.load(Ordering::Relaxed))
}

/// Unwrap an API fetch result, logging a warning on error instead of returning
/// an empty Vec silently. Silent defaults previously hid network/auth failures
/// that caused migration to skip whole categories without surfacing anything.
pub(super) fn fetch_or_warn<R: Runtime, T, E: std::fmt::Display>(
    result: Result<Vec<T>, E>,
    app_handle: &AppHandle<R>,
    context: &str,
    failures: &mut Vec<String>,
) -> Vec<T> {
    match result {
        Ok(v) => v,
        Err(e) => {
            let msg = format!("Fetch failed: {}: {}", context, e);
            tracing::warn!("{}", msg);
            emit_log(app_handle, "warning", &msg);
            failures.push(msg);
            Vec::new()
        }
    }
}
