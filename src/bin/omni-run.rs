use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use omni_proxy::platform::system_proxy::{set_system_proxy, unset_system_proxy};
use serde_json::json;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, ValueEnum)]
enum CaptureMode {
    Env,
    System,
}

#[derive(Debug, Parser)]
#[command(
    name = "omni-run",
    about = "Run a target program with OmniProxy capture modes (per-process MITM)"
)]
struct Cli {
    #[arg(long, value_enum, default_value = "env")]
    mode: CaptureMode,

    #[arg(long, default_value = "http://127.0.0.1:9090")]
    proxy_url: String,

    #[arg(long, default_value = "127.0.0.1,localhost,::1")]
    no_proxy: String,

    #[arg(long, default_value = "Wi-Fi")]
    network_service: String,

    #[arg(long, default_value_t = false)]
    clear_env: bool,

    #[arg(long, default_value_t = false)]
    print_only: bool,

    #[arg(long, default_value_t = false)]
    trace_sockets: bool,

    #[arg(long, default_value = ".omni-proxy/process_flows.jsonl")]
    trace_file: PathBuf,

    #[arg(long, default_value_t = 1500)]
    trace_interval_ms: u64,

    #[arg(last = true, required = true)]
    cmd: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let (prog, args) = split_cmd(&cli.cmd)?;

    if cli.print_only {
        println!("mode=print_only");
        println!("capture_mode={}", mode_name(&cli.mode));
        println!("proxy_url={}", cli.proxy_url);
        println!("network_service={}", cli.network_service);
        println!("no_proxy={}", cli.no_proxy);
        println!("trace_sockets={}", cli.trace_sockets);
        println!("trace_file={}", cli.trace_file.display());
        println!("cmd={}", cli.cmd.join(" "));
        return Ok(());
    }

    let mut system_proxy_guard = None;
    if let CaptureMode::System = cli.mode {
        let (host, port) = parse_proxy_endpoint(&cli.proxy_url)?;
        set_system_proxy(&cli.network_service, &host, port).with_context(|| {
            format!(
                "set temporary system proxy on service '{}' to {}:{}",
                cli.network_service, host, port
            )
        })?;
        system_proxy_guard = Some(SystemProxyGuard {
            network_service: cli.network_service.clone(),
        });
        println!("system_proxy=enabled");
    }

    let mut command = Command::new(prog);
    command.args(args);

    if cli.clear_env {
        command.env_clear();
    }

    command.env("HTTP_PROXY", &cli.proxy_url);
    command.env("HTTPS_PROXY", &cli.proxy_url);
    command.env("ALL_PROXY", &cli.proxy_url);
    command.env("http_proxy", &cli.proxy_url);
    command.env("https_proxy", &cli.proxy_url);
    command.env("all_proxy", &cli.proxy_url);
    command.env("NO_PROXY", &cli.no_proxy);
    command.env("no_proxy", &cli.no_proxy);

    command.stdin(Stdio::inherit());
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());

    let mut child = command
        .spawn()
        .with_context(|| format!("spawn target program: {}", prog))?;

    let target_pid = child.id();
    println!("target_pid={}", target_pid);

    let mut trace_handle = None;
    if cli.trace_sockets {
        trace_handle = Some(start_socket_trace(
            target_pid,
            cli.trace_file.clone(),
            cli.trace_interval_ms,
        )?);
        println!("socket_trace=enabled");
        println!("socket_trace_file={}", cli.trace_file.display());
    }

    let status = child.wait().context("wait target process")?;

    if let Some(mut h) = trace_handle {
        h.stop();
    }

    drop(system_proxy_guard);

    if let Some(code) = status.code() {
        println!("target_exit_code={}", code);
        std::process::exit(code);
    }

    bail!("target process terminated by signal")
}

struct SystemProxyGuard {
    network_service: String,
}

impl Drop for SystemProxyGuard {
    fn drop(&mut self) {
        let _ = unset_system_proxy(&self.network_service);
    }
}

struct TraceHandle {
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl TraceHandle {
    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn start_socket_trace(pid: u32, file: PathBuf, interval_ms: u64) -> Result<TraceHandle> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = Arc::clone(&stop);

    let handle = thread::spawn(move || {
        while !stop2.load(Ordering::SeqCst) {
            let rec = sample_lsof(pid);
            let ts_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_else(|_| Duration::from_millis(0))
                .as_millis();

            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&file) {
                let line = json!({
                    "ts_ms": ts_ms,
                    "pid": pid,
                    "sample": rec,
                });
                let _ = writeln!(f, "{}", line);
            }
            thread::sleep(Duration::from_millis(interval_ms.max(200)));
        }
    });

    Ok(TraceHandle {
        stop,
        join: Some(handle),
    })
}

fn sample_lsof(pid: u32) -> serde_json::Value {
    let out = Command::new("lsof")
        .args(["-nP", "-a", "-p", &pid.to_string(), "-i"])
        .output();

    match out {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            let lines = stdout
                .lines()
                .skip(1)
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.to_string())
                .collect::<Vec<_>>();
            json!({
                "ok": o.status.success(),
                "line_count": lines.len(),
                "lines": lines,
                "stderr": stderr.trim(),
            })
        }
        Err(e) => json!({
            "ok": false,
            "line_count": 0,
            "lines": [],
            "stderr": e.to_string(),
        }),
    }
}

fn split_cmd(cmd: &[String]) -> Result<(&str, &[String])> {
    if cmd.is_empty() {
        bail!("missing target command after --");
    }
    Ok((&cmd[0], &cmd[1..]))
}

fn mode_name(m: &CaptureMode) -> &'static str {
    match m {
        CaptureMode::Env => "env",
        CaptureMode::System => "system",
    }
}

fn parse_proxy_endpoint(proxy_url: &str) -> Result<(String, u16)> {
    let no_scheme = proxy_url
        .split_once("://")
        .map(|(_, rhs)| rhs)
        .unwrap_or(proxy_url);
    let host_port = no_scheme.split('/').next().unwrap_or(no_scheme);

    if let Some((h, p)) = host_port.rsplit_once(':') {
        let host = h.trim();
        let port = p
            .parse::<u16>()
            .with_context(|| format!("invalid proxy port in {}", proxy_url))?;
        if host.is_empty() {
            bail!("invalid proxy host in {}", proxy_url);
        }
        return Ok((host.to_string(), port));
    }

    bail!("invalid proxy url/endpoint: {}", proxy_url)
}
