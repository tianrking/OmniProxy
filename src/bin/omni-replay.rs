use anyhow::{Context, Result, bail};
use clap::Parser;
use omni_proxy::api::ApiEvent;
use reqwest::Method;
use std::{fs::File, io::BufRead, io::BufReader, path::PathBuf};

#[derive(Debug, Parser)]
#[command(name = "omni-replay", about = "Replay flows from OmniProxy JSONL logs")]
struct Cli {
    #[arg(long, default_value = "~/.omni-proxy/flows.jsonl")]
    flow_log: PathBuf,

    #[arg(long, default_value_t = false)]
    list: bool,

    #[arg(long)]
    index: Option<usize>,

    #[arg(long)]
    method_override: Option<String>,

    #[arg(long)]
    url_override: Option<String>,
}

#[derive(Debug, Clone)]
struct ReplayCandidate {
    index: usize,
    client: String,
    method: String,
    uri: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let path = expand_home(cli.flow_log);
    let requests = load_requests(&path)?;

    if cli.list {
        for req in &requests {
            println!(
                "#{:04}  {:6}  {}  ({})",
                req.index, req.method, req.uri, req.client
            );
        }
        return Ok(());
    }

    let idx = cli.index.context("please pass --index N, or use --list")?;

    let candidate = requests
        .iter()
        .find(|x| x.index == idx)
        .with_context(|| format!("index {} not found", idx))?;

    let method = cli
        .method_override
        .as_deref()
        .unwrap_or(&candidate.method)
        .to_uppercase();
    let uri = cli
        .url_override
        .as_deref()
        .unwrap_or(&candidate.uri)
        .to_string();

    if !uri.starts_with("http://") && !uri.starts_with("https://") {
        bail!(
            "URI is not absolute and cannot be replayed directly: {}",
            uri
        );
    }

    let method = Method::from_bytes(method.as_bytes())
        .with_context(|| format!("invalid method: {}", method))?;

    let client = reqwest::Client::builder().build()?;
    let resp = client.request(method.clone(), &uri).send().await?;
    let status = resp.status();
    let bytes = resp.bytes().await?;

    println!("replayed index: {}", idx);
    println!("request: {} {}", method, uri);
    println!("response status: {}", status);
    println!("response bytes: {}", bytes.len());

    Ok(())
}

fn load_requests(path: &PathBuf) -> Result<Vec<ReplayCandidate>> {
    let file = File::open(path).with_context(|| format!("open flow log {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut out = Vec::new();

    for (i, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let event: ApiEvent = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if let ApiEvent::HttpRequest {
            client,
            method,
            uri,
        } = event
        {
            out.push(ReplayCandidate {
                index: i,
                client,
                method,
                uri,
            });
        }
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
