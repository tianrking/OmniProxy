use anyhow::{Context, Result};
use clap::Parser;
use std::{net::SocketAddr, path::PathBuf};

#[derive(Debug, Clone, Parser)]
#[command(name = "omni-proxy", version, about = "OmniProxy MITM core")]
pub struct Cli {
    #[arg(long, env = "OMNI_LISTEN", default_value = "127.0.0.1:9090")]
    pub listen: String,

    #[arg(long, env = "OMNI_API_LISTEN", default_value = "127.0.0.1:9091")]
    pub api_listen: String,

    #[arg(long, env = "OMNI_CA_CERT", default_value = ".omni-proxy/ca.crt")]
    pub ca_cert: PathBuf,

    #[arg(long, env = "OMNI_CA_KEY", default_value = ".omni-proxy/ca.key")]
    pub ca_key: PathBuf,

    #[arg(long, env = "OMNI_PLUGIN_DIR", default_value = ".omni-proxy/plugins")]
    pub plugin_dir: PathBuf,

    #[arg(long, env = "OMNI_RULE_FILE", default_value = ".omni-proxy/rules.txt")]
    pub rule_file: PathBuf,

    #[arg(long, env = "OMNI_FLOW_LOG", default_value = ".omni-proxy/flows.jsonl")]
    pub flow_log: PathBuf,

    #[arg(long, env = "OMNI_LOG", default_value = "info")]
    pub log_level: String,

    #[arg(long, env = "OMNI_WASM_TIMEOUT_MS", default_value_t = 20_u64)]
    pub wasm_timeout_ms: u64,

    #[arg(long, env = "OMNI_CHECK_RULES", default_value_t = false)]
    pub check_rules: bool,

    #[arg(long, env = "OMNI_WS_PREVIEW_BYTES", default_value_t = 256_usize)]
    pub ws_preview_bytes: usize,

    #[arg(long, env = "OMNI_WS_DROP_PING", default_value_t = false)]
    pub ws_drop_ping: bool,

    #[arg(long = "ws-text-rewrite", env = "OMNI_WS_TEXT_REWRITE")]
    pub ws_text_rewrite: Vec<String>,

    #[arg(
        long,
        env = "OMNI_CAPTURE_BODY_MAX_BYTES",
        default_value_t = 65536_usize
    )]
    pub capture_body_max_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub listen_addr: SocketAddr,
    pub api_listen_addr: SocketAddr,
    pub ca_cert_path: PathBuf,
    pub ca_key_path: PathBuf,
    pub plugin_dir: PathBuf,
    pub rule_file_path: PathBuf,
    pub flow_log_path: PathBuf,
    pub wasm_timeout_ms: u64,
    pub ws_preview_bytes: usize,
    pub ws_drop_ping: bool,
    pub ws_text_rewrite: Vec<(String, String)>,
    pub capture_body_max_bytes: usize,
}

impl AppConfig {
    pub fn from_cli(cli: Cli) -> Result<Self> {
        let listen_addr = cli
            .listen
            .parse::<SocketAddr>()
            .with_context(|| format!("invalid listen addr: {}", cli.listen))?;
        let api_listen_addr = cli
            .api_listen
            .parse::<SocketAddr>()
            .with_context(|| format!("invalid api listen addr: {}", cli.api_listen))?;

        Ok(Self {
            listen_addr,
            api_listen_addr,
            ca_cert_path: expand_home(cli.ca_cert),
            ca_key_path: expand_home(cli.ca_key),
            plugin_dir: expand_home(cli.plugin_dir),
            rule_file_path: expand_home(cli.rule_file),
            flow_log_path: expand_home(cli.flow_log),
            wasm_timeout_ms: cli.wasm_timeout_ms,
            ws_preview_bytes: cli.ws_preview_bytes,
            ws_drop_ping: cli.ws_drop_ping,
            ws_text_rewrite: parse_rewrite_rules(&cli.ws_text_rewrite)?,
            capture_body_max_bytes: cli.capture_body_max_bytes,
        })
    }
}

fn parse_rewrite_rules(raw: &[String]) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for item in raw {
        let (from, to) = item
            .split_once("=>")
            .with_context(|| format!("invalid ws rewrite '{}', expect 'from=>to'", item))?;
        out.push((from.to_string(), to.to_string()));
    }
    Ok(out)
}

fn expand_home(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(stripped);
        }
    }
    path
}
