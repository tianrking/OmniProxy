use anyhow::{Context, Result};
use clap::Parser;
use std::{net::SocketAddr, path::PathBuf};

#[derive(Debug, Parser)]
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

    #[arg(long, env = "OMNI_LOG", default_value = "info")]
    pub log_level: String,

    #[arg(long, env = "OMNI_WASM_TIMEOUT_MS", default_value_t = 20_u64)]
    pub wasm_timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub listen_addr: SocketAddr,
    pub api_listen_addr: SocketAddr,
    pub ca_cert_path: PathBuf,
    pub ca_key_path: PathBuf,
    pub plugin_dir: PathBuf,
    pub wasm_timeout_ms: u64,
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
            wasm_timeout_ms: cli.wasm_timeout_ms,
        })
    }
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
