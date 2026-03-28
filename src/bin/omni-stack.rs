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

    #[arg(long, default_value_t = false)]
    vpn: bool,

    #[arg(long, default_value = "OmniProxy VPN")]
    vpn_service_name: String,

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
    let workspace_root = detect_workspace_root(&bin_dir);
    let profile = detect_profile(&bin_dir);
    let omni_global = bin_path(&bin_dir, "omni-global");
    let omni_transparent = bin_path(&bin_dir, "omni-transparent");
    let omni_transparentd = bin_path(&bin_dir, "omni-transparentd");
    let omni_vpn = bin_path(&bin_dir, "omni-vpn");

    let mut required_bins = vec!["omni-global"];
    if cli.transparent {
        required_bins.push("omni-transparent");
        required_bins.push("omni-transparentd");
    }
    if cli.vpn {
        required_bins.push("omni-vpn");
    }
    ensure_bins_present(&bin_dir, workspace_root.as_deref(), profile, &required_bins).await?;

    if !omni_global.exists() {
        bail!("missing binary after auto-build: {}", omni_global.display());
    }
    if cli.vpn {
        ensure_exec(&omni_vpn)?;
        let doctor_args = vec![
            "--service-name".to_string(),
            cli.vpn_service_name.clone(),
            "doctor".to_string(),
        ];
        run_once_owned(&omni_vpn, doctor_args).await?;

        let up_args = vec![
            "--service-name".to_string(),
            cli.vpn_service_name.clone(),
            "up".to_string(),
        ];
        run_once_owned(&omni_vpn, up_args).await?;
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
    println!("vpn={}", cli.vpn);
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
    if cli.vpn {
        let _ = run_once_owned(
            &omni_vpn,
            vec![
                "--service-name".to_string(),
                cli.vpn_service_name.clone(),
                "down".to_string(),
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

fn detect_profile(bin_dir: &Path) -> &'static str {
    match bin_dir.file_name().and_then(|s| s.to_str()) {
        Some("release") => "release",
        _ => "debug",
    }
}

fn detect_workspace_root(bin_dir: &Path) -> Option<PathBuf> {
    let mut cur = Some(bin_dir.to_path_buf());
    while let Some(path) = cur {
        if path.join("Cargo.toml").exists() {
            return Some(path);
        }
        cur = path.parent().map(|p| p.to_path_buf());
    }
    None
}

async fn ensure_bins_present(
    bin_dir: &Path,
    workspace_root: Option<&Path>,
    profile: &str,
    required_bins: &[&str],
) -> Result<()> {
    let missing = required_bins
        .iter()
        .filter_map(|name| {
            let path = bin_path(bin_dir, name);
            if path.exists() { None } else { Some(*name) }
        })
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }

    let root = workspace_root
        .context("required binaries are missing and Cargo workspace root is not discoverable")?;

    let mut cmd = Command::new("cargo");
    cmd.arg("build");
    if profile == "release" {
        cmd.arg("--release");
    }
    for name in &missing {
        cmd.args(["--bin", name]);
    }
    let status = cmd
        .current_dir(root)
        .status()
        .await
        .with_context(|| format!("auto-build missing binaries: {}", missing.join(", ")))?;
    if !status.success() {
        bail!(
            "auto-build failed for missing binaries: {}",
            missing.join(", ")
        );
    }
    Ok(())
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

async fn run_once_owned(bin: &Path, args: Vec<String>) -> Result<()> {
    let display = args.join(" ");
    let status = Command::new(bin)
        .args(args)
        .status()
        .await
        .with_context(|| format!("run {} {}", bin.display(), display))?;
    if !status.success() {
        bail!("command failed: {} {}", bin.display(), display);
    }
    Ok(())
}

fn mode_name(mode: &Mode) -> &'static str {
    match mode {
        Mode::Local => "local",
        Mode::Lan => "lan",
    }
}
