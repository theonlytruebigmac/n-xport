//! CLI command runner

use crate::api::NcClient;
use crate::commands::export::{get_scoped_devices, site_is_in_scope, DeviceCsvRow};
use crate::config::{Profile, Settings};
use crate::credentials::CredentialStore;
use crate::export::{export_to_csv, export_to_json};
use crate::models::{AccessGroupCsvRow, OrgPropertyCsvRow, UserRoleCsvRow};
use std::collections::HashSet;
use std::io::{self, BufRead, Write};

use super::{Cli, Commands, ExportArgs, ProfileCommands, TestArgs};

/// Run the CLI application
pub async fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Some(Commands::Export(args)) => run_export(cli.server, cli.profile, args).await,
        Some(Commands::Profile(args)) => run_profile(args.command).await,
        Some(Commands::Test(args)) => run_test(cli.server, cli.profile, args).await,
        None => {
            // No command - show help or interactive mode
            println!("N-Central Data Export Tool");
            println!("Use --help for usage information");
            Ok(())
        }
    }
}

/// Run the export command
async fn run_export(
    server: Option<String>,
    profile_name: Option<String>,
    args: ExportArgs,
) -> anyhow::Result<()> {
    // Resolve server and JWT
    let (base_url, jwt) = resolve_connection(server, profile_name, args.jwt.as_deref()).await?;

    println!("Connecting to {}...", base_url);

    let client = NcClient::new(&base_url);
    client.authenticate(&jwt).await?;

    println!("✓ Connected successfully");

    // Create output directory
    std::fs::create_dir_all(&args.output)?;

    let export_csv = args.format.iter().any(|f| f == "csv");
    let export_json = args.format.iter().any(|f| f == "json");

    let mut total_records = 0;
    let so_id = args.service_org;

    // Export Service Orgs
    if args.should_export("service_orgs") {
        print!("Exporting service organizations... ");
        io::stdout().flush()?;
        match client.get_service_orgs().await {
            Ok(data) => {
                let count = data.len();
                if export_csv {
                    export_to_csv(&data, args.output.join("service_orgs.csv"))?;
                }
                if export_json {
                    export_to_json(&data, args.output.join("service_orgs.json"))?;
                }
                println!("✓ {} records", count);
                total_records += count;
            }
            Err(e) => println!("✗ Error: {}", e),
        }
    }

    // Export Customers
    if args.should_export("customers") {
        print!("Exporting customers... ");
        io::stdout().flush()?;
        match client.get_customers_by_so(so_id).await {
            Ok(data) => {
                let count = data.len();
                if export_csv {
                    export_to_csv(&data, args.output.join("customers.csv"))?;
                }
                if export_json {
                    export_to_json(&data, args.output.join("customers.json"))?;
                }
                println!("✓ {} records", count);
                total_records += count;
            }
            Err(e) => println!("✗ Error: {}", e),
        }
    }

    // Export Sites
    if args.should_export("sites") {
        print!("Exporting sites... ");
        io::stdout().flush()?;
        // `/api/sites` lists every site on the server, so keep only this service org's.
        let scoped_sites = match client.get_customers_by_so(so_id).await {
            Ok(customers) => {
                let customer_ids: HashSet<i64> = customers.iter().map(|c| c.customer_id).collect();
                client.get_sites_by_so(so_id).await.map(|mut sites| {
                    sites.retain(|site| site_is_in_scope(site, so_id, &customer_ids));
                    sites
                })
            }
            Err(e) => Err(e),
        };
        match scoped_sites {
            Ok(data) => {
                let count = data.len();
                if export_csv {
                    export_to_csv(&data, args.output.join("sites.csv"))?;
                }
                if export_json {
                    export_to_json(&data, args.output.join("sites.json"))?;
                }
                println!("✓ {} records", count);
                total_records += count;
            }
            Err(e) => println!("✗ Error: {}", e),
        }
    }

    // Export Devices
    if args.should_export("devices") {
        print!("Exporting devices... ");
        io::stdout().flush()?;
        // Devices are listed server-wide; keep those under this service org's org units,
        // and write the flattened CSV shape the app uses (raw devices have variable columns).
        let mut valid_ou_ids: HashSet<i64> = HashSet::from([so_id]);
        let mut scope_error = None;
        match client.get_customers_by_so(so_id).await {
            Ok(customers) => {
                let customer_ids: HashSet<i64> = customers.iter().map(|c| c.customer_id).collect();
                valid_ou_ids.extend(&customer_ids);
                match client.get_sites_by_so(so_id).await {
                    Ok(sites) => valid_ou_ids.extend(
                        sites
                            .iter()
                            .filter(|site| site_is_in_scope(site, so_id, &customer_ids))
                            .map(|site| site.site_id),
                    ),
                    Err(e) => scope_error = Some(e.to_string()),
                }
            }
            Err(e) => scope_error = Some(e.to_string()),
        }
        let devices = match scope_error {
            Some(e) => Err(e),
            None => get_scoped_devices(&client, &valid_ou_ids, &mut None).await,
        };
        match devices {
            Ok(data) => {
                let count = data.len();
                if export_csv {
                    let rows: Vec<DeviceCsvRow> = data.iter().map(Into::into).collect();
                    export_to_csv(&rows, args.output.join("devices.csv"))?;
                }
                if export_json {
                    export_to_json(&data, args.output.join("devices.json"))?;
                }
                println!("✓ {} records", count);
                total_records += count;
            }
            Err(e) => println!("✗ Error: {}", e),
        }
    }

    // Export Access Groups
    if args.should_export("access_groups") {
        print!("Exporting access groups... ");
        io::stdout().flush()?;
        match client.get_access_groups(so_id).await {
            Ok(data) => {
                let count = data.len();
                if export_csv {
                    let rows: Vec<AccessGroupCsvRow> = data.iter().map(Into::into).collect();
                    export_to_csv(&rows, args.output.join("access_groups.csv"))?;
                }
                if export_json {
                    export_to_json(&data, args.output.join("access_groups.json"))?;
                }
                println!("✓ {} records", count);
                total_records += count;
            }
            Err(e) => println!("✗ Error: {}", e),
        }
    }

    // Export User Roles
    if args.should_export("user_roles") {
        print!("Exporting user roles... ");
        io::stdout().flush()?;
        match client.get_user_roles(so_id).await {
            Ok(data) => {
                let count = data.len();
                if export_csv {
                    let rows: Vec<UserRoleCsvRow> = data.iter().map(Into::into).collect();
                    export_to_csv(&rows, args.output.join("user_roles.csv"))?;
                }
                if export_json {
                    export_to_json(&data, args.output.join("user_roles.json"))?;
                }
                println!("✓ {} records", count);
                total_records += count;
            }
            Err(e) => println!("✗ Error: {}", e),
        }
    }

    // Export Org Properties
    if args.should_export("org_properties") {
        print!("Exporting organization properties... ");
        io::stdout().flush()?;
        match client.get_org_properties(so_id).await {
            Ok(data) => {
                let count = data.len();
                if export_csv {
                    // Raw properties have a variable number of columns; use the flattened shape.
                    let rows: Vec<OrgPropertyCsvRow> = data.iter().map(Into::into).collect();
                    export_to_csv(&rows, args.output.join("org_properties.csv"))?;
                }
                if export_json {
                    export_to_json(&data, args.output.join("org_properties.json"))?;
                }
                println!("✓ {} records", count);
                total_records += count;
            }
            Err(e) => println!("✗ Error: {}", e),
        }
    }

    // `--device-properties` and `--all` select device properties, which only the app exports.
    // Say so rather than silently producing nothing.
    if args.should_export("device_properties") {
        println!("Exporting device properties... ✗ not supported by the command line; use the app");
    }

    println!(
        "\n✓ Export complete: {} total records to {}",
        total_records,
        args.output.display()
    );

    Ok(())
}

/// Read a passphrase: from NC_CREDENTIALS_PASSPHRASE if set (for scripts; the environment is
/// readable by other processes of the same user, so prefer the prompt), otherwise a prompt
/// that does not echo.
fn read_passphrase(prompt: &str) -> anyhow::Result<String> {
    if let Ok(passphrase) = std::env::var("NC_CREDENTIALS_PASSPHRASE") {
        if !passphrase.is_empty() {
            return Ok(passphrase);
        }
    }
    rpassword::prompt_password(prompt)
        .map_err(|e| anyhow::anyhow!("Could not read the passphrase ({}). Set NC_CREDENTIALS_PASSPHRASE for non-interactive use.", e))
}

/// If credentials are kept in the encrypted file and it is locked, ask for the passphrase.
fn unlock_for_cli() -> anyhow::Result<()> {
    let status = CredentialStore::storage_status();
    if !status.disk_enabled || status.unlocked {
        return Ok(());
    }
    if status.vault_exists {
        let passphrase = read_passphrase("Passphrase for the encrypted credentials file: ")?;
        CredentialStore::unlock(&passphrase)?;
    } else {
        // Encrypted storage is on but no file exists yet: a new one needs a passphrase.
        let passphrase = read_passphrase(
            "New passphrase for the encrypted credentials file (at least 12 characters): ",
        )?;
        CredentialStore::enable_disk_storage(&passphrase)?;
    }
    Ok(())
}

/// Run profile management commands
async fn run_profile(cmd: ProfileCommands) -> anyhow::Result<()> {
    match cmd {
        ProfileCommands::List => {
            let settings = Settings::load()?;
            if settings.profiles.is_empty() {
                println!("No profiles saved.");
            } else {
                println!("Saved profiles:");
                for profile in &settings.profiles {
                    let active = settings.active_profile.as_ref() == Some(&profile.name);
                    let marker = if active { "*" } else { " " };
                    let creds = if CredentialStore::has_jwt(&profile.name) {
                        "✓"
                    } else {
                        " "
                    };
                    println!(
                        "  {} {} {} ({})",
                        marker, creds, profile.name, profile.source.fqdn
                    );
                }
                println!("\n* = active profile");
                println!("✓ = credentials stored");
            }
        }

        ProfileCommands::Add {
            name,
            server,
            service_org,
        } => {
            let mut settings = Settings::load()?;
            let mut profile = Profile::new_export(&name, &server);
            profile.source.service_org_id = service_org;
            settings.add_profile(profile);

            if settings.active_profile.is_none() {
                settings.active_profile = Some(name.clone());
            }

            settings.save()?;
            println!("✓ Profile '{}' saved", name);
        }

        ProfileCommands::Delete { name } => {
            let mut settings = Settings::load()?;
            settings.delete_profile(&name);
            let credentials = CredentialStore::delete_profile_credentials(&name);
            settings.save()?;
            println!("✓ Profile '{}' deleted", name);
            if let Err(e) = credentials {
                eprintln!("⚠ Some stored credentials could not be removed: {}", e);
            }
        }

        ProfileCommands::Use { name } => {
            let mut settings = Settings::load()?;
            settings.set_active_profile(&name)?;
            settings.save()?;
            println!("✓ Active profile set to '{}'", name);
        }

        ProfileCommands::EnableEncryptedStorage => {
            let status = CredentialStore::storage_status();
            let passphrase = if status.vault_exists {
                read_passphrase("Passphrase for the existing encrypted credentials file: ")?
            } else {
                println!(
                    "Credentials will be kept in a file encrypted with a passphrase you choose.\n\
                     There is no recovery: if you forget it, the stored credentials are lost."
                );
                let first = read_passphrase("New passphrase (at least 12 characters): ")?;
                let again = read_passphrase("Repeat the passphrase: ")?;
                if first != again {
                    anyhow::bail!("The passphrases do not match");
                }
                first
            };
            let report = CredentialStore::enable_disk_storage(&passphrase)?;
            let mut settings = Settings::load()?;
            settings.allow_disk_credentials = true;
            settings.save()?;
            println!("✓ Encrypted file storage is on");
            if report.to_keychain + report.to_encrypted_file + report.remaining > 0 {
                println!(
                    "  Moved credentials out of the old file: {} to the keychain, {} to the encrypted file, {} could not be moved",
                    report.to_keychain, report.to_encrypted_file, report.remaining
                );
            }
        }

        ProfileCommands::DisableEncryptedStorage { delete_file } => {
            CredentialStore::disable_disk_storage(delete_file)?;
            let mut settings = Settings::load()?;
            settings.allow_disk_credentials = false;
            settings.save()?;
            println!("✓ Encrypted file storage is off");
            if !delete_file && CredentialStore::storage_status().vault_exists {
                println!("  The encrypted file was kept; it cannot be read while this is off. Use --delete-file to remove it.");
            }
        }

        ProfileCommands::StorageStatus => {
            let status = CredentialStore::storage_status();
            println!(
                "Encrypted file storage: {}",
                if status.disk_enabled { "on" } else { "off" }
            );
            println!(
                "Encrypted file: {}",
                if status.vault_exists {
                    "present"
                } else {
                    "none"
                }
            );
            if status.legacy_entries > 0 {
                println!(
                    "⚠ {} credential(s) are still in the old unencrypted file. Run: nc-export profile enable-encrypted-storage",
                    status.legacy_entries
                );
            }
        }

        ProfileCommands::SetCredentials { name } => {
            println!("Enter JWT token for profile '{}': ", name);
            let mut jwt = String::new();
            io::stdin().lock().read_line(&mut jwt)?;
            let jwt = jwt.trim();

            if jwt.is_empty() {
                anyhow::bail!("JWT cannot be empty");
            }

            // With encrypted file storage on, storing needs the passphrase.
            unlock_for_cli()?;
            let storage = CredentialStore::store_jwt(&name, jwt)?;
            println!("✓ Credentials stored for profile '{}'", name);
            if let Some(warning) = storage.warning() {
                eprintln!("⚠ {}", warning);
            }
        }
    }

    Ok(())
}

/// Run connection test
async fn run_test(
    server: Option<String>,
    profile_name: Option<String>,
    args: TestArgs,
) -> anyhow::Result<()> {
    let (base_url, jwt) = resolve_connection(server, profile_name, args.jwt.as_deref()).await?;

    println!("Testing connection to {}...", base_url);

    let client = NcClient::new(&base_url);

    match client.authenticate(&jwt).await {
        Ok(()) => {
            println!("✓ Authentication successful");

            if let Ok(info) = client.get_server_info().await {
                if let Some(version) = info.version {
                    println!("  Server version: {}", version);
                }
            }
        }
        Err(e) => {
            println!("✗ Authentication failed: {}", e);
        }
    }

    Ok(())
}

/// Resolve connection details from CLI args or profile
async fn resolve_connection(
    server: Option<String>,
    profile_name: Option<String>,
    jwt: Option<&str>,
) -> anyhow::Result<(String, String)> {
    // If explicit server provided, use it
    if let Some(server) = server {
        let jwt = match jwt {
            Some(j) => j.to_string(),
            None => {
                anyhow::bail!("JWT required when using --server. Use --jwt or set NC_JWT env var")
            }
        };
        return Ok((crate::commands::connection::server_url(&server), jwt));
    }

    // Otherwise, try to load from profile
    let settings = Settings::load()?;
    if jwt.is_none() {
        // The credential may be in the encrypted file.
        unlock_for_cli()?;
    }

    let profile_name = profile_name
        .or(settings.active_profile.clone())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No profile specified and no active profile set. Use --server or --profile"
            )
        })?;

    let profile = settings
        .profiles
        .iter()
        .find(|p| p.name == profile_name)
        .ok_or_else(|| anyhow::anyhow!("Profile '{}' not found", profile_name))?;

    let jwt = match jwt {
        Some(j) => j.to_string(),
        None => CredentialStore::get_jwt(&profile_name)?.ok_or_else(|| {
            anyhow::anyhow!(
                "No credentials stored for profile '{}'. Use: nc-export profile set-credentials {}",
                profile_name,
                profile_name
            )
        })?,
    };

    Ok((profile.base_url(), jwt))
}
