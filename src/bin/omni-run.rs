use anyhow::{Context, Result, bail};
use clap::Parser;
use std::process::{Command, Stdio};

#[derive(Debug, Parser)]
#[command(
    name = "omni-run",
    about = "Run a target program with OmniProxy env injection (per-process capture)"
)]
struct Cli {
    #[arg(long, default_value = "http://127.0.0.1:9090")]
    proxy_url: String,

    #[arg(long, default_value = "127.0.0.1,localhost,::1")]
    no_proxy: String,

    #[arg(long, default_value_t = false)]
    clear_env: bool,

    #[arg(long, default_value_t = false)]
    print_only: bool,

    #[arg(last = true, required = true)]
    cmd: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let (prog, args) = split_cmd(&cli.cmd)?;

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

    if cli.print_only {
        println!("mode=print_only");
        println!("proxy_url={}", cli.proxy_url);
        println!("no_proxy={}", cli.no_proxy);
        println!("cmd={}", cli.cmd.join(" "));
        return Ok(());
    }

    command.stdin(Stdio::inherit());
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());

    let status = command
        .status()
        .with_context(|| format!("spawn target program: {}", prog))?;

    if let Some(code) = status.code() {
        println!("target_exit_code={}", code);
        std::process::exit(code);
    }

    bail!("target process terminated by signal")
}

fn split_cmd(cmd: &[String]) -> Result<(&str, &[String])> {
    if cmd.is_empty() {
        bail!("missing target command after --");
    }
    Ok((&cmd[0], &cmd[1..]))
}
