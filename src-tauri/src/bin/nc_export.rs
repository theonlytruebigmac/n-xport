// N-Central Data Export Tool - CLI Binary
// Run with: cargo run --bin nc-export -- [args]

use clap::Parser;
use nc_data_export_lib::cli::{Cli, runner};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into())
        )
        .init();

    let cli = Cli::parse();
    
    if cli.verbose {
        tracing::info!("Verbose mode enabled");
    }

    runner::run(cli).await
}
