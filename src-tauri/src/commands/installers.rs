//! Generate Windows agent installer scripts for customers and sites.
//!
//! Each script embeds the registration token of its customer or site, so the output is
//! sensitive. The install logic comes from the N-central Customer Installer Generator sample
//! script by Morgan Rourke (https://github.com/MorganR99).

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Emitter, State, Window};

use crate::commands::connection::AppState;
use crate::commands::export::registration_tokens::{
    fetch_tokens, token_targets, TokenPhase, TokenTarget,
};
use crate::commands::export::ExportResult;
use crate::models::{ProgressUpdate, RegistrationToken};

const TEMPLATE: &str = include_str!("../../data/install_agent.ps1.template");

/// A customer or site that can get an installer script.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallerTarget {
    pub org_unit_id: i64,
    /// `customer` or `site`.
    pub org_unit_type: String,
    pub name: String,
    /// The customer a site belongs to, when known.
    pub customer_name: Option<String>,
    /// The ID of that customer, so a list can group sites under it.
    #[serde(default)]
    pub customer_id: Option<i64>,
}

impl InstallerTarget {
    /// How the target is named in the script, its file name, and messages.
    fn display_name(&self) -> String {
        match (&*self.org_unit_type, &self.customer_name) {
            ("site", Some(customer)) => format!("{} - {}", customer, self.name),
            _ => self.name.clone(),
        }
    }

    fn as_token_target(&self) -> TokenTarget {
        let is_site = self.org_unit_type == "site";
        TokenTarget {
            org_unit_id: self.org_unit_id,
            org_unit_type: if is_site { "site" } else { "customer" },
            customer_id: None,
            customer_name: if is_site {
                self.customer_name.clone()
            } else {
                Some(self.name.clone())
            },
            site_id: is_site.then_some(self.org_unit_id),
            site_name: is_site.then(|| self.name.clone()),
        }
    }
}

/// The server a script installs from: HTTPS on port 443, which is what the agent installer
/// arguments assume.
#[derive(Debug, PartialEq, Eq)]
struct ServerAddress {
    url: String,
    host: String,
}

fn server_address(base_url: &str) -> Result<ServerAddress, String> {
    let url = reqwest::Url::parse(base_url).map_err(|e| {
        format!(
            "The server address '{}' is not a valid URL: {}",
            base_url, e
        )
    })?;
    let host = url.host_str().unwrap_or_default().to_string();
    let host_is_plain = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    if url.scheme() != "https" || !host_is_plain || !url.username().is_empty() {
        return Err(format!(
            "Installer scripts need an HTTPS server address with a plain host name; got '{}'.",
            base_url
        ));
    }
    if url.port().is_some_and(|port| port != 443) {
        return Err(format!(
            "Installer scripts use port 443, but '{}' uses another port.",
            base_url
        ));
    }
    Ok(ServerAddress {
        url: format!("https://{}", host),
        host,
    })
}

/// Make a value safe inside a PowerShell single-quoted string. PowerShell treats the curly
/// single quotes as quote characters too, so each of them is doubled like a plain `'`, and
/// line breaks and other control characters become spaces.
fn powershell_literal(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => format!("{c}{c}"),
            c if c.is_control() => " ".to_string(),
            c => c.to_string(),
        })
        .collect()
}

/// Registration tokens are GUID-like. Anything else would not install, and could not be
/// safely placed on the installer command line.
fn is_plain_token(token: &str) -> bool {
    !token.is_empty()
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn render_script(
    server: &ServerAddress,
    target: &InstallerTarget,
    token: &str,
) -> Result<String, String> {
    if !is_plain_token(token) {
        return Err("The registration token has unexpected characters".into());
    }
    let org_unit_type = if target.org_unit_type == "site" {
        "site"
    } else {
        "customer"
    };
    Ok(TEMPLATE
        .replace("__ORG_UNIT_TYPE__", org_unit_type)
        .replace("__SERVER__", &powershell_literal(&server.url))
        .replace("__CUSTOMER_ID__", &target.org_unit_id.to_string())
        .replace(
            "__CUSTOMER_NAME__",
            &powershell_literal(&target.display_name()),
        )
        .replace("__REGISTRATION_TOKEN__", token))
}

/// A file name Windows accepts: no reserved characters or device names, no trailing dots or
/// spaces, and not unreasonably long.
fn safe_file_stem(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let mut stem: String = cleaned
        .trim()
        .trim_end_matches(['.', ' '])
        .chars()
        .take(80)
        .collect();
    stem = stem.trim_end_matches(['.', ' ']).to_string();
    if stem.is_empty() {
        return "Customer".into();
    }
    let device_name = stem.split('.').next().unwrap_or_default().to_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&device_name.as_str())
        || ["COM", "LPT"].iter().any(|prefix| {
            device_name
                .strip_prefix(prefix)
                .is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"))
        });
    if reserved {
        format!("_{}", stem)
    } else {
        stem
    }
}

fn script_file_name(target: &InstallerTarget) -> String {
    format!(
        "{}_{}_Install-NCentralAgent.ps1",
        safe_file_stem(&target.display_name()),
        target.org_unit_id
    )
}

fn package_folder_name(host: &str, stamp: &str) -> String {
    format!("N-central Installer Scripts - {} - {}", host, stamp)
}

/// Write a file only the current user can read where the OS supports it.
fn write_private_file(path: &Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(contents.as_bytes())
}

/// Whether a token's expiry date, such as `2024-05-01T23:59:00-04:00[America/New_York]`, is
/// in the past. Unreadable dates count as not expired.
fn token_expired(expiry: &str, now: chrono::DateTime<chrono::Utc>) -> bool {
    let instant = expiry.split('[').next().unwrap_or_default().trim();
    chrono::DateTime::parse_from_rfc3339(instant).is_ok_and(|date| date < now)
}

#[derive(Debug, Default)]
pub(crate) struct GenerationOutcome {
    pub folder: Option<PathBuf>,
    pub files: Vec<String>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub cancelled: bool,
}

/// Fetch the token of each target and write one script per target into a new folder under
/// `output_dir`. A problem with one target becomes a warning and the rest are still written.
pub(crate) async fn generate_scripts(
    client: &crate::api::client::NcClient,
    server_url: &str,
    output_dir: &Path,
    targets: &[InstallerTarget],
    stamp: &str,
    emit_progress: &(dyn Fn(&str, &str, f32) + Send + Sync),
    cancel_token: &AtomicBool,
) -> GenerationOutcome {
    const PHASE: &str = "Installer Scripts";
    let mut outcome = GenerationOutcome::default();

    let server = match server_address(server_url) {
        Ok(server) => server,
        Err(message) => {
            outcome.errors.push(message);
            return outcome;
        }
    };

    let mut seen = HashSet::new();
    let unique: Vec<&InstallerTarget> = targets
        .iter()
        .filter(|t| seen.insert((t.org_unit_type.clone(), t.org_unit_id)))
        .collect();

    let folder = output_dir.join(package_folder_name(&server.host, stamp));
    if let Err(e) = std::fs::create_dir_all(&folder) {
        outcome.errors.push(format!(
            "Could not create the output folder {}: {}",
            folder.display(),
            e
        ));
        return outcome;
    }
    outcome.folder = Some(folder.clone());

    let phase = TokenPhase {
        name: PHASE,
        base_percent: 5.0,
        percent_range: 80.0,
    };
    let fetched = fetch_tokens(
        client,
        unique.iter().map(|t| t.as_token_target()).collect(),
        phase,
        emit_progress,
        cancel_token,
    )
    .await;
    outcome.cancelled = cancel_token.load(Ordering::Relaxed);

    emit_progress(PHASE, "Writing scripts...", 90.0);
    let now = chrono::Utc::now();
    for (target, (_, token)) in unique.iter().zip(fetched) {
        let name = target.display_name();
        let RegistrationToken {
            registration_token,
            registration_token_expiry_date,
        } = match token {
            Ok(token) => token,
            Err(e) => {
                outcome.warnings.push(format!(
                    "Could not get the registration token for {} [ID {}]: {}",
                    name, target.org_unit_id, e
                ));
                continue;
            }
        };
        let Some(token) = registration_token.filter(|t| !t.trim().is_empty()) else {
            outcome.warnings.push(format!(
                "{} [ID {}] has no registration token.",
                name, target.org_unit_id
            ));
            continue;
        };
        let script = match render_script(&server, target, token.trim()) {
            Ok(script) => script,
            Err(e) => {
                outcome.warnings.push(format!(
                    "Skipped {} [ID {}]: {}.",
                    name, target.org_unit_id, e
                ));
                continue;
            }
        };
        let path = folder.join(script_file_name(target));
        match write_private_file(&path, &script) {
            Ok(()) => {
                outcome.files.push(path.display().to_string());
                if registration_token_expiry_date
                    .as_deref()
                    .is_some_and(|expiry| token_expired(expiry, now))
                {
                    outcome.warnings.push(format!(
                        "The registration token for {} [ID {}] has expired, so its script will not install an agent until a new token is generated in N-central.",
                        name, target.org_unit_id
                    ));
                }
            }
            Err(e) => outcome.warnings.push(format!(
                "Could not write the script for {} [ID {}]: {}",
                name, target.org_unit_id, e
            )),
        }
    }
    outcome
}

/// List the customers and sites of a service organization that can get a script.
#[tauri::command]
pub async fn list_installer_targets(
    service_org_id: i64,
    state: State<'_, AppState>,
) -> std::result::Result<Vec<InstallerTarget>, String> {
    let client = state.client.lock().await;
    let client = client.as_ref().ok_or("Not connected")?;

    let customers = client
        .get_customers_by_so(service_org_id)
        .await
        .map_err(|e| format!("Failed to fetch customers: {}", e))?;
    let customer_ids: HashSet<i64> = customers.iter().map(|c| c.customer_id).collect();
    let mut sites = client
        .get_sites_by_so(service_org_id)
        .await
        .map_err(|e| format!("Failed to fetch sites: {}", e))?;
    sites.retain(|site| {
        crate::commands::export::site_is_in_scope(site, service_org_id, &customer_ids)
    });

    Ok(token_targets(&customers, &sites)
        .into_iter()
        .map(|target| match target.org_unit_type {
            "site" => InstallerTarget {
                org_unit_id: target.org_unit_id,
                org_unit_type: "site".into(),
                name: target.site_name.unwrap_or_default(),
                customer_name: target.customer_name,
                customer_id: target.customer_id,
            },
            _ => InstallerTarget {
                org_unit_id: target.org_unit_id,
                org_unit_type: "customer".into(),
                name: target.customer_name.unwrap_or_default(),
                customer_name: None,
                customer_id: None,
            },
        })
        .collect())
}

/// Generate one installer script per selected customer or site.
#[tauri::command]
pub async fn generate_installer_scripts(
    window: Window,
    output_dir: String,
    targets: Vec<InstallerTarget>,
    state: State<'_, AppState>,
) -> std::result::Result<ExportResult, String> {
    let client = state.client.lock().await;
    let client = client.as_ref().ok_or("Not connected")?;

    state.cancel_token.store(false, Ordering::Relaxed);
    let cancel_token = state.cancel_token.clone();
    let emit_progress = |phase: &str, message: &str, percent: f32| {
        let _ = window.emit(
            "export-progress",
            ProgressUpdate {
                phase: phase.to_string(),
                message: message.to_string(),
                percent,
                current: 0,
                total: 0,
            },
        );
    };
    emit_progress("Installer Scripts", "Starting...", 0.0);

    let stamp = chrono::Local::now().format("%Y-%m-%d %H%M%S").to_string();
    let outcome = generate_scripts(
        client,
        client.base_url(),
        Path::new(&output_dir),
        &targets,
        &stamp,
        &emit_progress,
        &cancel_token,
    )
    .await;

    let created = outcome.files.len();
    let partial =
        !outcome.cancelled && (!outcome.warnings.is_empty() || !outcome.errors.is_empty());
    let folder = outcome
        .folder
        .as_ref()
        .map(|f| f.display().to_string())
        .unwrap_or_default();
    let message = if outcome.cancelled {
        format!("Cancelled after creating {} script(s)", created)
    } else if created == 0 && !outcome.errors.is_empty() {
        outcome.errors[0].clone()
    } else if partial {
        format!(
            "Created {} of {} script(s) with {} warning(s) in {}",
            created,
            targets.len(),
            outcome.warnings.len() + outcome.errors.len(),
            folder
        )
    } else {
        format!("Created {} script(s) in {}", created, folder)
    };
    emit_progress("Complete", &message, 100.0);

    Ok(ExportResult {
        success: !outcome.cancelled && (outcome.errors.is_empty() || created > 0),
        cancelled: outcome.cancelled,
        partial,
        message,
        files_created: outcome.files,
        total_records: created,
        warnings: outcome.warnings,
        errors: outcome.errors,
    })
}

#[cfg(test)]
mod tests;
