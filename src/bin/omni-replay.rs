use anyhow::{Context, Result, bail};
use clap::Parser;
use omni_proxy::api::ApiEvent;
use reqwest::Method;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
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
    request_id: Option<String>,

    #[arg(long)]
    method_override: Option<String>,

    #[arg(long)]
    url_override: Option<String>,

    #[arg(long = "header")]
    headers: Vec<String>,
}

#[derive(Debug, Clone)]
struct ReplayCandidate {
    index: usize,
    request_id: Option<String>,
    client: String,
    method: String,
    uri: String,
    headers: Vec<(String, String)>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let path = expand_home(cli.flow_log);
    let requests = load_requests(&path)?;

    if cli.list {
        for req in &requests {
            println!(
                "#{:04}  {:6}  {}  ({})  req_id={}",
                req.index,
                req.method,
                req.uri,
                req.client,
                req.request_id.as_deref().unwrap_or("-")
            );
        }
        return Ok(());
    }

    let candidate = if let Some(request_id) = &cli.request_id {
        requests
            .iter()
            .find(|x| x.request_id.as_deref() == Some(request_id.as_str()))
            .with_context(|| format!("request_id {} not found", request_id))?
    } else {
        let idx = cli
            .index
            .context("please pass --index N, --request-id, or use --list")?;
        requests
            .iter()
            .find(|x| x.index == idx)
            .with_context(|| format!("index {} not found", idx))?
    };

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

    let headers = build_headers(&candidate.headers, &cli.headers)?;

    let client = reqwest::Client::builder().build()?;
    let resp = client
        .request(method.clone(), &uri)
        .headers(headers)
        .send()
        .await?;
    let status = resp.status();
    let bytes = resp.bytes().await?;

    println!("replayed index: {}", candidate.index);
    println!(
        "replayed request_id: {}",
        candidate.request_id.as_deref().unwrap_or("-")
    );
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
            request_id,
            client,
            method,
            uri,
            headers,
        } = event
        {
            out.push(ReplayCandidate {
                index: i,
                request_id,
                client,
                method,
                uri,
                headers,
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

fn build_headers(captured: &[(String, String)], overrides: &[String]) -> Result<HeaderMap> {
    let mut map = HeaderMap::new();
    for (k, v) in captured {
        if is_hop_by_hop(k) {
            continue;
        }
        let name = HeaderName::from_bytes(k.as_bytes())
            .with_context(|| format!("invalid captured header name: {}", k))?;
        let value = HeaderValue::from_str(v)
            .with_context(|| format!("invalid captured header value: {}", k))?;
        map.insert(name, value);
    }

    for item in overrides {
        let (k, v) = item
            .split_once(':')
            .with_context(|| format!("invalid --header '{}', expected 'Key: Value'", item))?;
        let name = HeaderName::from_bytes(k.trim().as_bytes())
            .with_context(|| format!("invalid override header name: {}", k.trim()))?;
        let value = HeaderValue::from_str(v.trim())
            .with_context(|| format!("invalid override header value for {}", k.trim()))?;
        map.insert(name, value);
    }

    Ok(map)
}

fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "proxy-connection"
            | "keep-alive"
            | "transfer-encoding"
            | "upgrade"
            | "te"
            | "trailer"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "host"
            | "content-length"
    )
}
