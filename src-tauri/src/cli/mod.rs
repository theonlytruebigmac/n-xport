//! CLI interface for N-Central Data Export Tool
//!
//! Provides command-line access to all export functionality.

pub mod runner;

use std::path::PathBuf;
use clap::{Parser, Subcommand, Args};

/// N-Central Data Export Tool - Export data from N-Central via REST API
#[derive(Parser, Debug)]
#[command(name = "nc-export")]
#[command(author = "FrazierSystems")]
#[command(version)]
#[command(about = "Export N-Central data via REST API", long_about = None)]
pub struct Cli {
    /// Server FQDN (e.g., ncentral.example.com)
    #[arg(short, long, global = true)]
    pub server: Option<String>,

    /// Profile name to use (loads settings from saved profile)
    #[arg(short, long, global = true)]
    pub profile: Option<String>,

    /// Enable verbose output
    #[arg(short, long, global = true, default_value = "false")]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Export data from N-Central
    Export(ExportArgs),
    
    /// Manage connection profiles
    Profile(ProfileArgs),
    
    /// Test connection to N-Central server
    Test(TestArgs),
}

/// Arguments for the export command
#[derive(Args, Debug)]
pub struct ExportArgs {
    /// JWT token for authentication (or set NC_JWT env var)
    #[arg(short, long, env = "NC_JWT")]
    pub jwt: Option<String>,

    /// Service Organization ID to export from
    #[arg(long)]
    pub service_org: i64,

    /// Output directory for exported files
    #[arg(short, long, default_value = "./nc_export")]
    pub output: PathBuf,

    /// Export formats (csv, json)
    #[arg(short, long, value_delimiter = ',', default_value = "csv")]
    pub format: Vec<String>,

    /// Export all data types
    #[arg(long)]
    pub all: bool,

    /// Export service organizations
    #[arg(long)]
    pub service_orgs: bool,

    /// Export customers
    #[arg(long)]
    pub customers: bool,

    /// Export sites
    #[arg(long)]
    pub sites: bool,

    /// Export devices
    #[arg(long)]
    pub devices: bool,

    /// Export access groups
    #[arg(long)]
    pub access_groups: bool,

    /// Export user roles
    #[arg(long)]
    pub user_roles: bool,

    /// Export organization properties
    #[arg(long)]
    pub org_properties: bool,

    /// Export device properties (may be slow for large datasets)
    #[arg(long)]
    pub device_properties: bool,
}

impl ExportArgs {
    /// Check if any export type is explicitly selected
    pub fn has_explicit_selection(&self) -> bool {
        self.service_orgs || self.customers || self.sites || self.devices ||
        self.access_groups || self.user_roles || self.org_properties || self.device_properties
    }

    /// Returns true for all types if --all is set or no explicit selection
    pub fn should_export(&self, export_type: &str) -> bool {
        if self.all || !self.has_explicit_selection() {
            // Default: export main types (not device properties by default)
            match export_type {
                "device_properties" => self.device_properties,
                _ => true,
            }
        } else {
            match export_type {
                "service_orgs" => self.service_orgs,
                "customers" => self.customers,
                "sites" => self.sites,
                "devices" => self.devices,
                "access_groups" => self.access_groups,
                "user_roles" => self.user_roles,
                "org_properties" => self.org_properties,
                "device_properties" => self.device_properties,
                _ => false,
            }
        }
    }
}

/// Arguments for profile management
#[derive(Args, Debug)]
pub struct ProfileArgs {
    #[command(subcommand)]
    pub command: ProfileCommands,
}

#[derive(Subcommand, Debug)]
pub enum ProfileCommands {
    /// List all saved profiles
    List,
    
    /// Add or update a profile
    Add {
        /// Profile name
        name: String,
        /// Server FQDN
        #[arg(short, long)]
        server: String,
        /// Service Organization ID
        #[arg(long)]
        service_org: Option<i64>,
    },
    
    /// Delete a profile
    Delete {
        /// Profile name to delete
        name: String,
    },
    
    /// Set the active profile
    Use {
        /// Profile name to activate
        name: String,
    },
    
    /// Store JWT credentials for a profile (reads from stdin)
    SetCredentials {
        /// Profile name
        name: String,
    },
}

/// Arguments for connection testing
#[derive(Args, Debug)]
pub struct TestArgs {
    /// JWT token for authentication (or set NC_JWT env var)
    #[arg(short, long, env = "NC_JWT")]
    pub jwt: Option<String>,
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            server: None,
            profile: None,
            verbose: false,
            command: None,
        }
    }
}
