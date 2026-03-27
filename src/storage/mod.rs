use crate::api::ApiEvent;
use anyhow::{Context, Result};
use tokio::{
    fs::{self, OpenOptions},
    io::AsyncWriteExt,
    sync::broadcast,
};
use tracing::{error, info};

#[derive(Debug, Clone, Copy)]
pub struct FlowLogOptions {
    pub rotate_bytes: u64,
    pub max_files: usize,
}

pub async fn run_flow_logger(
    path: &std::path::Path,
    mut rx: broadcast::Receiver<ApiEvent>,
    opts: FlowLogOptions,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .with_context(|| format!("create flow log dir {}", parent.display()))?;
    }

    let mut file = open_append(path).await?;
    let mut current_size = file.metadata().await.map(|m| m.len()).unwrap_or(0);

    info!(
        path = %path.display(),
        rotate_bytes = opts.rotate_bytes,
        max_files = opts.max_files,
        "flow logger enabled"
    );

    loop {
        match rx.recv().await {
            Ok(event) => {
                let line = serde_json::to_string(&event)?;
                file.write_all(line.as_bytes()).await?;
                file.write_all(b"\n").await?;
                current_size = current_size.saturating_add(line.len() as u64 + 1);

                if opts.rotate_bytes > 0 {
                    if current_size >= opts.rotate_bytes {
                        file.flush().await?;
                        rotate_logs(path, opts.max_files).await?;
                        file = open_append(path).await?;
                        current_size = file.metadata().await.map(|m| m.len()).unwrap_or(0);
                    }
                }
            }
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                error!(skipped, "flow logger lagged");
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }

    Ok(())
}

async fn open_append(path: &std::path::Path) -> Result<tokio::fs::File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
        .with_context(|| format!("open flow log file {}", path.display()))
}

async fn rotate_logs(path: &std::path::Path, max_files: usize) -> Result<()> {
    if max_files == 0 {
        return Ok(());
    }

    for idx in (1..=max_files).rev() {
        let src = numbered_path(path, idx);
        let dst = numbered_path(path, idx + 1);
        if fs::try_exists(&src).await.unwrap_or(false) {
            if idx == max_files {
                let _ = fs::remove_file(&src).await;
            } else {
                let _ = fs::rename(&src, &dst).await;
            }
        }
    }

    let first = numbered_path(path, 1);
    if fs::try_exists(path).await.unwrap_or(false) {
        fs::rename(path, &first).await.with_context(|| {
            format!("rotate flow log {} -> {}", path.display(), first.display())
        })?;
    }
    Ok(())
}

fn numbered_path(path: &std::path::Path, idx: usize) -> std::path::PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(format!(".{}", idx));
    std::path::PathBuf::from(s)
}
