use super::*;
use crate::test_support::{authenticated_client, json_response, not_found, start};
use std::process::Command;
use tempfile::TempDir;

fn target(id: i64, kind: &str, name: &str, customer: Option<&str>) -> InstallerTarget {
    InstallerTarget {
        org_unit_id: id,
        org_unit_type: kind.into(),
        name: name.into(),
        customer_name: customer.map(Into::into),
        customer_id: None,
    }
}

fn server() -> ServerAddress {
    server_address("https://nc.example.com").unwrap()
}

const TOKEN: &str = "2f64ee5e-7c37-dce7-ffd2-d32912609623";

#[test]
fn the_server_must_be_plain_https_on_443() {
    assert_eq!(
        server_address("https://nc.example.com/").unwrap(),
        ServerAddress {
            url: "https://nc.example.com".into(),
            host: "nc.example.com".into()
        }
    );
    assert!(server_address("https://nc.example.com:443").is_ok());
    for bad in [
        "http://nc.example.com",
        "https://nc.example.com:8443",
        "https://user:pw@nc.example.com",
        "not a url",
    ] {
        assert!(server_address(bad).is_err(), "{bad} should be refused");
    }
}

#[test]
fn names_cannot_break_out_of_the_powershell_string() {
    assert_eq!(powershell_literal("O'Brien & Co"), "O''Brien & Co");
    // PowerShell also treats curly single quotes as quote characters.
    assert_eq!(
        powershell_literal("A\u{2018}B\u{2019}C"),
        "A\u{2018}\u{2018}B\u{2019}\u{2019}C"
    );
    assert_eq!(powershell_literal("line1\r\nline2\t'x"), "line1  line2 ''x");
    assert_eq!(powershell_literal("$(Get-Date) `n"), "$(Get-Date) `n");
}

#[test]
fn a_script_carries_the_target_server_id_and_token() {
    let script = render_script(&server(), &target(55, "site", "HQ", Some("Acme")), TOKEN).unwrap();
    assert!(script.contains("$NcentralServer = 'https://nc.example.com'"));
    assert!(script.contains("$CustomerId = '55'"));
    assert!(script.contains("$CustomerName = 'Acme - HQ'"));
    assert!(script.contains(&format!("$RegistrationToken = '{TOKEN}'")));
    assert!(script.contains("(site)"));
    assert!(script.contains("https://github.com/MorganR99"));
    assert!(!script.contains("__"), "every placeholder is filled");
}

#[test]
fn tokens_with_unexpected_characters_are_refused() {
    for bad in ["", "abc'; Remove-Item *; '", "tok en", "tok\"en", "a$(b)"] {
        assert!(
            render_script(&server(), &target(1, "customer", "C", None), bad).is_err(),
            "{bad:?} should be refused"
        );
    }
}

#[test]
fn file_names_are_safe_on_windows() {
    assert_eq!(safe_file_stem("Acme: East/West?"), "Acme_ East_West_");
    assert_eq!(safe_file_stem("trailing dots..."), "trailing dots");
    assert_eq!(safe_file_stem("   "), "Customer");
    assert_eq!(safe_file_stem("CON"), "_CON");
    assert_eq!(safe_file_stem("nul.txt"), "_nul.txt");
    assert_eq!(safe_file_stem("COM3"), "_COM3");
    assert_eq!(safe_file_stem("COM10"), "COM10");
    assert_eq!(safe_file_stem(&"x".repeat(200)).chars().count(), 80);
    assert_eq!(
        script_file_name(&target(7, "site", "HQ", Some("Acme"))),
        "Acme - HQ_7_Install-NCentralAgent.ps1"
    );
    // Same name, different org units: the ID keeps the files apart.
    assert_ne!(
        script_file_name(&target(1, "customer", "Same", None)),
        script_file_name(&target(2, "customer", "Same", None))
    );
}

#[test]
fn token_expiry_is_read_from_the_api_format() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-06-01T00:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert!(token_expired(
        "2024-05-01T23:59:00-04:00[America/New_York]",
        now
    ));
    assert!(!token_expired(
        "2030-05-01T23:59:00-04:00[America/New_York]",
        now
    ));
    assert!(!token_expired("not a date", now));
}

fn token_server() -> impl std::future::Future<Output = crate::test_support::MockServer> {
    start(|method, path, _| {
        assert_eq!(method, "GET");
        match path {
            "/api/org-units/20/registration-token" => not_found(),
            "/api/org-units/30/registration-token" => {
                json_response(serde_json::json!({"data": {}}))
            }
            "/api/org-units/40/registration-token" => json_response(serde_json::json!({"data": {
                "registrationToken": "has bad chars!"}})),
            "/api/org-units/50/registration-token" => json_response(serde_json::json!({"data": {
                "registrationToken": TOKEN,
                "registrationTokenExpiryDate": "2001-01-01T00:00:00Z[UTC]"}})),
            p if p.ends_with("/registration-token") => {
                let id = p
                    .trim_start_matches("/api/org-units/")
                    .trim_end_matches("/registration-token");
                json_response(serde_json::json!({"data": {
                    "registrationToken": format!("token-{id}"),
                    "registrationTokenExpiryDate": "2099-01-01T00:00:00Z[UTC]"}}))
            }
            _ => not_found(),
        }
    })
}

#[tokio::test]
async fn writes_a_script_for_each_target_that_has_a_usable_token() {
    let server = token_server().await;
    let client = authenticated_client(&server).await;
    let dir = TempDir::new().unwrap();
    let targets = vec![
        target(10, "customer", "Acme", None),
        target(11, "site", "Acme HQ", Some("Acme")),
        target(20, "customer", "No access", None),
        target(30, "customer", "No token", None),
        target(40, "customer", "Odd token", None),
        target(50, "customer", "Expired", None),
        // Listed twice: still one script.
        target(10, "customer", "Acme", None),
    ];

    let outcome = generate_scripts(
        &client,
        "https://nc.example.com",
        dir.path(),
        &targets,
        "2026-01-02 030405",
        &|_, _, _| {},
        &AtomicBool::new(false),
    )
    .await;

    assert!(outcome.errors.is_empty(), "{:?}", outcome.errors);
    let folder = outcome.folder.clone().expect("package folder");
    assert_eq!(
        folder.file_name().unwrap().to_str().unwrap(),
        "N-central Installer Scripts - nc.example.com - 2026-01-02 030405"
    );
    let mut names: Vec<_> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "Acme - Acme HQ_11_Install-NCentralAgent.ps1",
            "Acme_10_Install-NCentralAgent.ps1",
            "Expired_50_Install-NCentralAgent.ps1",
        ]
    );
    assert_eq!(outcome.files.len(), 3);

    let joined = outcome.warnings.join("\n");
    assert!(joined.contains("No access [ID 20]"), "{joined}");
    assert!(
        joined.contains("No token [ID 30] has no registration token"),
        "{joined}"
    );
    assert!(joined.contains("Odd token [ID 40]"), "{joined}");
    assert!(joined.contains("Expired [ID 50] has expired"), "{joined}");
    assert_eq!(outcome.warnings.len(), 4);
    // Tokens are secrets and never appear in warnings.
    assert!(!joined.contains(TOKEN));
    assert_eq!(
        server.count("GET", "/api/org-units/10/registration-token"),
        1
    );
}

#[cfg(unix)]
#[tokio::test]
async fn scripts_are_readable_only_by_the_current_user() {
    use std::os::unix::fs::PermissionsExt;
    let server = token_server().await;
    let client = authenticated_client(&server).await;
    let dir = TempDir::new().unwrap();
    let outcome = generate_scripts(
        &client,
        "https://nc.example.com",
        dir.path(),
        &[target(10, "customer", "Acme", None)],
        "stamp",
        &|_, _, _| {},
        &AtomicBool::new(false),
    )
    .await;
    let mode = std::fs::metadata(&outcome.files[0])
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[tokio::test]
async fn an_unusable_server_address_writes_nothing() {
    let server = token_server().await;
    let client = authenticated_client(&server).await;
    let dir = TempDir::new().unwrap();
    let outcome = generate_scripts(
        &client,
        "https://nc.example.com:8443",
        dir.path(),
        &[target(10, "customer", "Acme", None)],
        "stamp",
        &|_, _, _| {},
        &AtomicBool::new(false),
    )
    .await;
    assert_eq!(outcome.errors.len(), 1);
    assert!(outcome.files.is_empty());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    assert_eq!(server.requests().len(), 0);
}

/// Parse a generated script with PowerShell itself, including names built to break out of
/// the string they are placed in. Skipped where `pwsh` is not installed.
#[tokio::test]
async fn generated_scripts_parse_in_powershell_even_with_hostile_names() {
    if Command::new("pwsh").arg("-Version").output().is_err() {
        eprintln!("pwsh not installed; skipping");
        return;
    }
    let server = token_server().await;
    let client = authenticated_client(&server).await;
    let dir = TempDir::new().unwrap();
    let hostile = [
        "Plain",
        "O'Brien",
        "Curly \u{2018}quote\u{2019} \u{201B}x\u{201A}",
        "'; Remove-Item -Recurse C:\\ #",
        "#> Write-Host pwned <#",
        "$(Start-Process calc) `\" \"\"",
        "Line\nBreak",
    ];
    let targets: Vec<_> = hostile
        .iter()
        .enumerate()
        .map(|(i, name)| target(100 + i as i64, "customer", name, None))
        .collect();

    let outcome = generate_scripts(
        &client,
        "https://nc.example.com",
        dir.path(),
        &targets,
        "stamp",
        &|_, _, _| {},
        &AtomicBool::new(false),
    )
    .await;
    assert_eq!(outcome.files.len(), hostile.len(), "{:?}", outcome.warnings);

    for file in &outcome.files {
        let check = format!(
            "$errors = $null; $tokens = $null; \
             $ast = [System.Management.Automation.Language.Parser]::ParseFile('{}', [ref]$tokens, [ref]$errors); \
             if ($errors.Count -gt 0) {{ $errors | ForEach-Object {{ $_.Message }}; exit 1 }}; \
             $assign = $ast.FindAll({{ param($n) $n -is [System.Management.Automation.Language.AssignmentStatementAst] -and $n.Left.Extent.Text -eq '$CustomerName' }}, $true); \
             if ($assign.Count -ne 1) {{ 'CustomerName assigned ' + $assign.Count + ' times'; exit 2 }}; \
             if ($assign[0].Right.Extent.Text -notmatch '^\\$?[''\\u2018\\u2019\\u201A\\u201B]') {{ exit 3 }}",
            powershell_literal(file)
        );
        let output = Command::new("pwsh")
            .args(["-NoProfile", "-NonInteractive", "-Command", &check])
            .output()
            .expect("run pwsh");
        assert!(
            output.status.success(),
            "{} did not parse cleanly:\n{}{}",
            file,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
