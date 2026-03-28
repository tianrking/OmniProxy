use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use omni_proxy::{
    cert::load_or_init_issuer,
    config::{AppConfig, Cli as CoreCli},
    platform::system_proxy::{
        platform_name, set_proxy_hint, set_system_proxy, unset_proxy_hint, unset_system_proxy,
    },
    proxy,
};
use std::net::{SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command as TokioCommand};
use tokio::task::JoinHandle;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone, ValueEnum)]
enum Mode {
    Local,
    Lan,
}

#[derive(Debug, Parser)]
#[command(
    name = "omni-global",
    about = "One-command OmniProxy bootstrap + global/lan capture helper"
)]
struct Cli {
    #[arg(long, value_enum, default_value = "local")]
    mode: Mode,

    #[arg(long)]
    listen: Option<String>,

    #[arg(long)]
    api_listen: Option<String>,

    #[arg(long, default_value = ".omni-proxy/ca.crt")]
    ca_cert: PathBuf,

    #[arg(long, default_value = ".omni-proxy/ca.key")]
    ca_key: PathBuf,

    #[arg(long, default_value = ".omni-proxy/plugins")]
    plugin_dir: PathBuf,

    #[arg(long, default_value = ".omni-proxy/rules.txt")]
    rule_file: PathBuf,

    #[arg(long, default_value = ".omni-proxy/flows.jsonl")]
    flow_log: PathBuf,

    #[arg(long, default_value = "Wi-Fi")]
    network_service: String,

    #[arg(long, default_value_t = false)]
    set_system_proxy: bool,

    #[arg(long, default_value_t = false)]
    unset_system_proxy: bool,

    #[arg(long, default_value_t = true)]
    print_shell_proxy: bool,

    #[arg(long, default_value_t = false)]
    kernel_capture: bool,

    #[arg(long, default_value = "tcp or udp")]
    kernel_capture_filter: String,

    #[arg(long, default_value = ".omni-proxy/kernel_capture.log")]
    kernel_capture_out: PathBuf,

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

    let (listen, api_listen) = resolve_addrs(&cli)?;
    let listen_addr = listen
        .parse::<SocketAddr>()
        .with_context(|| format!("invalid --listen: {}", listen))?;

    bootstrap_layout(&cli).await?;

    if cli.unset_system_proxy {
        if let Err(err) = unset_system_proxy(&cli.network_service) {
            println!("system_proxy=manual");
            println!(
                "unset_proxy_hint={}",
                unset_proxy_hint(&cli.network_service)
            );
            warn!(error = %err, "auto unset system proxy failed");
            return Ok(());
        }
        println!("system_proxy=disabled");
        return Ok(());
    }

    if cli.set_system_proxy {
        let host_for_system_proxy = if listen_addr.ip().is_unspecified() {
            "127.0.0.1".to_string()
        } else {
            listen_addr.ip().to_string()
        };
        if let Err(err) = set_system_proxy(
            &cli.network_service,
            &host_for_system_proxy,
            listen_addr.port(),
        ) {
            println!("system_proxy=manual");
            println!(
                "set_proxy_hint={}",
                set_proxy_hint(
                    &host_for_system_proxy,
                    listen_addr.port(),
                    &cli.network_service
                )
            );
            warn!(error = %err, "auto set system proxy failed, continue with manual mode");
        } else {
            println!("system_proxy=enabled");
        }
    }

    print_quick_hints(&cli, listen_addr);

    let core_cli = CoreCli {
        listen,
        api_listen,
        api_max_lag: 8192,
        ca_cert: cli.ca_cert.clone(),
        ca_key: cli.ca_key.clone(),
        plugin_dir: cli.plugin_dir.clone(),
        rule_file: cli.rule_file.clone(),
        flow_log: cli.flow_log.clone(),
        log_level: cli.log_level.clone(),
        wasm_timeout_ms: 20,
        wasm_max_failures: 100,
        check_rules: false,
        diagnose_ca: false,
        bootstrap: false,
        ws_preview_bytes: 256,
        ws_drop_ping: false,
        ws_text_rewrite: Vec::new(),
        capture_body_max_bytes: 65536,
        capture_body_sample_rate: 1.0,
        capture_body_compressed: false,
        flow_log_rotate_bytes: 134_217_728,
        flow_log_max_files: 5,
    };

    let app = AppConfig::from_cli(core_cli)?;
    let mut capture_guard = if cli.kernel_capture {
        match start_kernel_capture(&cli).await {
            Ok(v) => Some(v),
            Err(err) => {
                warn!(error = %err, "kernel capture start failed, continue without it");
                None
            }
        }
    } else {
        None
    };
    info!(listen = %app.listen_addr, api = %app.api_listen_addr, "omni-global launching proxy core");
    let run_result = proxy::run(app).await;
    if let Some(mut guard) = capture_guard.take() {
        guard.stop().await;
    }
    run_result
}

fn resolve_addrs(cli: &Cli) -> Result<(String, String)> {
    if let (Some(listen), Some(api)) = (&cli.listen, &cli.api_listen) {
        return Ok((listen.clone(), api.clone()));
    }

    let (default_listen, default_api) = match cli.mode {
        Mode::Local => ("127.0.0.1:9090".to_string(), "127.0.0.1:9091".to_string()),
        Mode::Lan => ("0.0.0.0:9090".to_string(), "0.0.0.0:9091".to_string()),
    };

    Ok((
        cli.listen.clone().unwrap_or(default_listen),
        cli.api_listen.clone().unwrap_or(default_api),
    ))
}

async fn bootstrap_layout(cli: &Cli) -> Result<()> {
    if let Some(parent) = cli.ca_cert.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if let Some(parent) = cli.ca_key.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if let Some(parent) = cli.rule_file.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if let Some(parent) = cli.flow_log.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::create_dir_all(&cli.plugin_dir).await?;

    let _ = load_or_init_issuer(&cli.ca_cert, &cli.ca_key).await?;
    if !cli.rule_file.exists() {
        tokio::fs::write(
            &cli.rule_file,
            "# OmniProxy rules\n# deny req.method == \"TRACE\"\n",
        )
        .await?;
    }
    if !cli.flow_log.exists() {
        tokio::fs::write(&cli.flow_log, b"").await?;
    }
    Ok(())
}

fn print_quick_hints(cli: &Cli, listen_addr: SocketAddr) {
    println!(
        "bootstrap_ok=true\\nca_cert={}\\nca_key={}\\nrule_file={}\\nflow_log={}",
        cli.ca_cert.display(),
        cli.ca_key.display(),
        cli.rule_file.display(),
        cli.flow_log.display()
    );

    #[cfg(target_os = "macos")]
    {
        println!(
            "macos_trust_ca=sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain {}",
            cli.ca_cert.display()
        );
    }

    if cli.print_shell_proxy {
        println!(
            "shell_proxy=export HTTP_PROXY=http://{} && export HTTPS_PROXY=http://{}",
            listen_addr, listen_addr
        );
    }

    println!("platform={}", platform_name());
    println!(
        "set_proxy_hint={}",
        set_proxy_hint("127.0.0.1", listen_addr.port(), &cli.network_service)
    );
    println!(
        "unset_proxy_hint={}",
        unset_proxy_hint(&cli.network_service)
    );

    match cli.mode {
        Mode::Local => {
            println!("mode=local");
        }
        Mode::Lan => {
            println!("mode=lan");
            if let Some(ip) = detect_primary_ip() {
                println!(
                    "lan_hint=Set other devices proxy to {}:{} and trust CA from {}",
                    ip,
                    listen_addr.port(),
                    cli.ca_cert.display()
                );
            } else {
                warn!("could not auto-detect primary LAN ip for hint output");
            }
        }
    }
}

fn detect_primary_ip() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    let _ = socket.connect("8.8.8.8:80");
    let local = socket.local_addr().ok()?;
    Some(local.ip().to_string())
}

struct KernelCaptureGuard {
    child: Child,
    reader_task: JoinHandle<()>,
}

impl KernelCaptureGuard {
    async fn stop(&mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
        self.reader_task.abort();
    }
}

async fn start_kernel_capture(cli: &Cli) -> Result<KernelCaptureGuard> {
    if let Some(parent) = cli.kernel_capture_out.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&cli.kernel_capture_out)
        .await?;

    let mut cmd = TokioCommand::new("tcpdump");
    cmd.args(["-l", "-n", "-i", "any", &cli.kernel_capture_filter]);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);
    let mut child = cmd
        .spawn()
        .with_context(|| "failed to spawn tcpdump. try sudo or install tcpdump/libpcap")?;

    let stdout = child
        .stdout
        .take()
        .context("tcpdump stdout not available")?;
    let mut reader = BufReader::new(stdout).lines();
    let mut writer = file;

    let task = tokio::spawn(async move {
        loop {
            match reader.next_line().await {
                Ok(Some(line)) => {
                    let _ = writer.write_all(line.as_bytes()).await;
                    let _ = writer.write_all(b"\n").await;
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
        let _ = writer.flush().await;
    });

    println!(
        "kernel_capture=enabled\nkernel_capture_filter={}\nkernel_capture_out={}",
        cli.kernel_capture_filter,
        cli.kernel_capture_out.display()
    );
    Ok(KernelCaptureGuard {
        child,
        reader_task: task,
    })
}
