use crate::api::ApiEvent;
use anyhow::{Context, Result};
use tokio::{
    fs::{self, OpenOptions},
    io::AsyncWriteExt,
    sync::broadcast,
};
use tracing::{error, info};

pub async fn run_flow_logger(
    path: &std::path::Path,
    mut rx: broadcast::Receiver<ApiEvent>,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .with_context(|| format!("create flow log dir {}", parent.display()))?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
        .with_context(|| format!("open flow log file {}", path.display()))?;

    info!(path = %path.display(), "flow logger enabled");

    loop {
        match rx.recv().await {
            Ok(event) => {
                let line = serde_json::to_string(&event)?;
                file.write_all(line.as_bytes()).await?;
                file.write_all(b"\n").await?;
            }
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                error!(skipped, "flow logger lagged");
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }

    Ok(())
}
