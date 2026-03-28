use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Platform {
    Auto,
    Macos,
    Linux,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    Up,
    Down,
    Status,
}

#[derive(Debug, Parser)]
#[command(
    name = "omni-transparent",
    about = "Transparent HTTP redirect helper (macOS/Linux)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,

    #[arg(long, value_enum, default_value = "auto")]
    platform: Platform,

    #[arg(long, default_value_t = 9090)]
    proxy_port: u16,

    #[arg(long, default_value = "en0")]
    interface: String,

    #[arg(long, default_value = ".omni-proxy/pf.omni.conf")]
    pf_anchor_file: PathBuf,

    #[arg(long, default_value_t = false)]
    apply: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let platform = detect_platform(cli.platform)?;

    let actions = match (&cli.cmd, platform) {
        (Cmd::Up, Platform::Macos) => build_macos_up(&cli),
        (Cmd::Down, Platform::Macos) => build_macos_down(&cli),
        (Cmd::Status, Platform::Macos) => build_macos_status(),
        (Cmd::Up, Platform::Linux) => build_linux_up(&cli),
        (Cmd::Down, Platform::Linux) => build_linux_down(&cli),
        (Cmd::Status, Platform::Linux) => build_linux_status(),
        _ => bail!("unsupported platform for transparent helper"),
    };

    println!("transparent_platform={}", platform_name(platform));
    println!("transparent_apply={}", cli.apply);

    for step in &actions {
        println!("cmd={}", step.render());
    }

    if !cli.apply {
        println!("dry_run=true (pass --apply to execute)");
        return Ok(());
    }

    if matches!(platform, Platform::Macos) {
        if let Some(parent) = cli.pf_anchor_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
    }

    for step in &actions {
        run_step(step)?;
    }

    println!("transparent_ok=true");
    Ok(())
}

#[derive(Debug, Clone)]
struct Step {
    cmd: String,
    args: Vec<String>,
}

impl Step {
    fn new(cmd: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            cmd: cmd.into(),
            args,
        }
    }

    fn render(&self) -> String {
        if self.args.is_empty() {
            self.cmd.clone()
        } else {
            format!("{} {}", self.cmd, self.args.join(" "))
        }
    }
}

fn run_step(step: &Step) -> Result<()> {
    let status = Command::new(&step.cmd)
        .args(&step.args)
        .status()
        .with_context(|| format!("execute failed: {}", step.render()))?;
    if !status.success() {
        bail!("command failed: {}", step.render());
    }
    Ok(())
}

fn build_macos_up(cli: &Cli) -> Vec<Step> {
    let rule = format!(
        "rdr pass on {} inet proto tcp from any to any port 80 -> 127.0.0.1 port {}",
        cli.interface, cli.proxy_port
    );
    vec![
        Step::new(
            "sh",
            vec![
                "-c".into(),
                format!(
                    "printf '%s\\n' \"{}\" > {}",
                    rule,
                    cli.pf_anchor_file.display()
                ),
            ],
        ),
        Step::new(
            "sudo",
            vec![
                "pfctl".into(),
                "-a".into(),
                "com.omniproxy.http".into(),
                "-f".into(),
                cli.pf_anchor_file.display().to_string(),
            ],
        ),
        Step::new("sudo", vec!["pfctl".into(), "-E".into()]),
    ]
}

fn build_macos_down(_cli: &Cli) -> Vec<Step> {
    vec![Step::new(
        "sudo",
        vec![
            "pfctl".into(),
            "-a".into(),
            "com.omniproxy.http".into(),
            "-F".into(),
            "all".into(),
        ],
    )]
}

fn build_macos_status() -> Vec<Step> {
    vec![Step::new(
        "sudo",
        vec![
            "pfctl".into(),
            "-a".into(),
            "com.omniproxy.http".into(),
            "-s".into(),
            "rules".into(),
        ],
    )]
}

fn build_linux_up(cli: &Cli) -> Vec<Step> {
    vec![Step::new(
        "sudo",
        vec![
            "iptables".into(),
            "-t".into(),
            "nat".into(),
            "-A".into(),
            "OUTPUT".into(),
            "-p".into(),
            "tcp".into(),
            "--dport".into(),
            "80".into(),
            "-j".into(),
            "REDIRECT".into(),
            "--to-ports".into(),
            cli.proxy_port.to_string(),
        ],
    )]
}

fn build_linux_down(cli: &Cli) -> Vec<Step> {
    vec![Step::new(
        "sudo",
        vec![
            "iptables".into(),
            "-t".into(),
            "nat".into(),
            "-D".into(),
            "OUTPUT".into(),
            "-p".into(),
            "tcp".into(),
            "--dport".into(),
            "80".into(),
            "-j".into(),
            "REDIRECT".into(),
            "--to-ports".into(),
            cli.proxy_port.to_string(),
        ],
    )]
}

fn build_linux_status() -> Vec<Step> {
    vec![Step::new(
        "sudo",
        vec![
            "iptables".into(),
            "-t".into(),
            "nat".into(),
            "-S".into(),
            "OUTPUT".into(),
        ],
    )]
}

fn detect_platform(p: Platform) -> Result<Platform> {
    match p {
        Platform::Auto => {
            if cfg!(target_os = "macos") {
                Ok(Platform::Macos)
            } else if cfg!(target_os = "linux") {
                Ok(Platform::Linux)
            } else {
                bail!("auto platform detection supports macOS/Linux only")
            }
        }
        v => Ok(v),
    }
}

fn platform_name(p: Platform) -> &'static str {
    match p {
        Platform::Auto => "auto",
        Platform::Macos => "macos",
        Platform::Linux => "linux",
    }
}
