use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use std::path::{Path, PathBuf};
use tokio::process::{Child, Command};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone, ValueEnum)]
enum Mode {
    Local,
    Lan,
}

#[derive(Debug, Parser)]
#[command(
    name = "omni-stack",
    about = "One-command full OmniProxy stack runner with auto cleanup"
)]
struct Cli {
    #[arg(long, value_enum, default_value = "local")]
    mode: Mode,

    #[arg(long, default_value = "Wi-Fi")]
    network_service: String,

    #[arg(long, default_value_t = true)]
    system_proxy: bool,

    #[arg(long, default_value_t = true)]
    transparent: bool,

    #[arg(long, default_value_t = true)]
    kernel_capture: bool,

    #[arg(long, default_value = "info")]
    log_level: String,
}

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

    let bin_dir = current_bin_dir()?;
    let omni_global = bin_path(&bin_dir, "omni-global");
    let omni_transparent = bin_path(&bin_dir, "omni-transparent");
    let omni_transparentd = bin_path(&bin_dir, "omni-transparentd");

    if !omni_global.exists() {
        bail!("missing binary: {}", omni_global.display());
    }

    let mut transparentd_child: Option<Child> = None;

    if cli.transparent {
        ensure_exec(&omni_transparent)?;
        ensure_exec(&omni_transparentd)?;

        transparentd_child = Some(
            Command::new(&omni_transparentd)
                .args([
                    "--http-listen",
                    "127.0.0.1:10080",
                    "--https-listen",
                    "127.0.0.1:10443",
                ])
                .spawn()
                .with_context(|| format!("spawn {}", omni_transparentd.display()))?,
        );

        run_once(
            &omni_transparent,
            &[
                "up",
                "--apply",
                "--transparent-http-port",
                "10080",
                "--transparent-https-port",
                "10443",
            ],
        )
        .await?;
    }

    let mut global_args = vec!["--mode".to_string(), mode_name(&cli.mode).to_string()];
    if cli.system_proxy {
        global_args.push("--set-system-proxy".into());
        global_args.push("--network-service".into());
        global_args.push(cli.network_service.clone());
    }
    if cli.kernel_capture {
        global_args.push("--kernel-capture".into());
    }

    let mut global_child = Command::new(&omni_global)
        .args(global_args)
        .spawn()
        .with_context(|| format!("spawn {}", omni_global.display()))?;

    println!("stack_up=true");
    println!("mode={}", mode_name(&cli.mode));
    println!("system_proxy={}", cli.system_proxy);
    println!("transparent={}", cli.transparent);
    println!("kernel_capture={}", cli.kernel_capture);
    println!("stop=Ctrl+C");

    tokio::signal::ctrl_c().await?;
    info!("ctrl-c received, cleaning up stack");

    let _ = global_child.start_kill();
    let _ = global_child.wait().await;

    if let Some(mut child) = transparentd_child {
        let _ = child.start_kill();
        let _ = child.wait().await;
    }

    if cli.transparent {
        let _ = run_once(
            &omni_transparent,
            &[
                "down",
                "--apply",
                "--transparent-http-port",
                "10080",
                "--transparent-https-port",
                "10443",
            ],
        )
        .await;
    }

    if cli.system_proxy {
        let _ = run_once(
            &omni_global,
            &[
                "--unset-system-proxy",
                "--network-service",
                &cli.network_service,
            ],
        )
        .await;
    }

    println!("stack_down=true");
    Ok(())
}

fn current_bin_dir() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let dir = exe
        .parent()
        .context("current_exe has no parent directory")?;
    Ok(dir.to_path_buf())
}

fn bin_path(bin_dir: &Path, name: &str) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        return bin_dir.join(format!("{}.exe", name));
    }
    #[cfg(not(target_os = "windows"))]
    {
        bin_dir.join(name)
    }
}

fn ensure_exec(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("missing binary: {}", path.display());
    }
    Ok(())
}

async fn run_once(bin: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new(bin)
        .args(args)
        .status()
        .await
        .with_context(|| format!("run {} {}", bin.display(), args.join(" ")))?;
    if !status.success() {
        bail!("command failed: {} {}", bin.display(), args.join(" "));
    }
    Ok(())
}

fn mode_name(mode: &Mode) -> &'static str {
    match mode {
        Mode::Local => "local",
        Mode::Lan => "lan",
    }
}
