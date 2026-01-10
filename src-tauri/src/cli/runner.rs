//! CLI command runner

use crate::api::NcClient;
use crate::config::{Profile, Settings};
use crate::credentials::CredentialStore;
use crate::export::{export_to_csv, export_to_json};
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
        match client.get_sites().await {
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
        match client.get_devices().await {
            Ok(data) => {
                let count = data.len();
                if export_csv {
                    export_to_csv(&data, args.output.join("devices.csv"))?;
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
                    export_to_csv(&data, args.output.join("access_groups.csv"))?;
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
                    export_to_csv(&data, args.output.join("user_roles.csv"))?;
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
                    export_to_csv(&data, args.output.join("org_properties.csv"))?;
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

    println!(
        "\n✓ Export complete: {} total records to {}",
        total_records,
        args.output.display()
    );

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
            let _ = CredentialStore::delete_jwt(&name);
            settings.save()?;
            println!("✓ Profile '{}' deleted", name);
        }

        ProfileCommands::Use { name } => {
            let mut settings = Settings::load()?;
            settings.set_active_profile(&name)?;
            settings.save()?;
            println!("✓ Active profile set to '{}'", name);
        }

        ProfileCommands::SetCredentials { name } => {
            println!("Enter JWT token for profile '{}': ", name);
            let mut jwt = String::new();
            io::stdin().lock().read_line(&mut jwt)?;
            let jwt = jwt.trim();

            if jwt.is_empty() {
                anyhow::bail!("JWT cannot be empty");
            }

            CredentialStore::store_jwt(&name, jwt)?;
            println!("✓ Credentials stored for profile '{}'", name);
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

            match client.get_server_info().await {
                Ok(info) => {
                    if let Some(version) = info.version {
                        println!("  Server version: {}", version);
                    }
                }
                Err(_) => {}
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
        return Ok((format!("https://{}", server), jwt));
    }

    // Otherwise, try to load from profile
    let settings = Settings::load()?;

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
