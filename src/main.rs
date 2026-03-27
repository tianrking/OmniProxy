use anyhow::Result;
use clap::Parser;
use omni_proxy::config::Cli;
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

    let app = omni_proxy::config::AppConfig::from_cli(cli)?;
    info!(listen = %app.listen_addr, "starting OmniProxy core");

    omni_proxy::proxy::run(app).await
}
