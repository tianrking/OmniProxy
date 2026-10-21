use anyhow::Result;
use clap::Parser;
use std::{net::SocketAddr, path::PathBuf};
#[derive(Parser)]
#[command(
    version,
    about = "Local OmniProxy capture, fault reproduction and regression workbench"
)]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:9092")]
    listen: SocketAddr,
    #[arg(long, default_value = ".omni-workbench")]
    workspace: PathBuf,
}
#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    anyhow::ensure!(
        cli.listen.ip().is_loopback(),
        "workbench must listen on loopback"
    );
    let listener = tokio::net::TcpListener::bind(cli.listen).await?;
    let address = listener.local_addr()?;
    let state = omni_proxy::workbench::Workbench::new(cli.workspace, address).await?;
    println!("OmniProxy Workbench → http://{address}\nPress Ctrl+C to stop.");
    axum::serve(listener, omni_proxy::workbench::router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
