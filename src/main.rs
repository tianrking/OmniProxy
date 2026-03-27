mod api;
mod cert;
mod config;
mod filter;
mod plugins;
mod proxy;
mod query;

use anyhow::Result;
use clap::Parser;
use config::Cli;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&cli.log_level)),
        )
        .with_target(false)
        .compact()
        .init();

    let app = config::AppConfig::from_cli(cli)?;
    info!(listen = %app.listen_addr, "starting OmniProxy core");

    proxy::run(app).await
}
