use anyhow::{Context, Result, bail};
use base64::Engine as _;
use clap::Parser;
use omni_proxy::api::ApiEvent;
use reqwest::Method;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
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

    #[arg(long, default_value_t = false)]
    dry_run: bool,

    #[arg(long, default_value_t = false)]
    print_curl: bool,

    #[arg(long, default_value_t = false)]
    no_body: bool,
}

#[derive(Debug, Clone)]
struct ReplayCandidate {
    index: usize,
    timestamp_ms: u64,
    request_id: Option<String>,
    client: String,
    method: String,
    uri: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    captured_response: Option<ResponseSnapshot>,
}

#[derive(Debug, Clone)]
struct ResponseSnapshot {
    status: u16,
    body_size: Option<usize>,
    headers_hash: String,
    body_hash: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let path = expand_home(cli.flow_log);
    let requests = load_requests(&path)?;

    if cli.list {
        for req in &requests {
            println!(
                "#{:04}  {:6}  {}  ({})  req_id={}  ts={}",
                req.index,
                req.method,
                req.uri,
                req.client,
                req.request_id.as_deref().unwrap_or("-"),
                req.timestamp_ms,
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
    let body = if cli.no_body {
        None
    } else {
        candidate.body.clone()
    };

    if cli.print_curl || cli.dry_run {
        println!("{}", render_curl(&method.to_string(), &uri, &headers));
    }

    if cli.dry_run {
        println!("dry-run enabled, skip actual request");
        return Ok(());
    }

    let client = reqwest::Client::builder().build()?;
    let mut req = client.request(method.clone(), &uri).headers(headers);
    if let Some(body) = body.clone() {
        req = req.body(body);
    }
    let resp = req.send().await?;
    let status = resp.status();
    let live_headers = normalize_headers(headers_to_pairs(resp.headers()));
    let bytes = resp.bytes().await?;
    let live_headers_hash = hash_headers(&live_headers);
    let live_body_hash = if bytes.is_empty() {
        None
    } else {
        Some(hash_bytes(&bytes))
    };

    println!("replayed index: {}", candidate.index);
    println!(
        "replayed request_id: {}",
        candidate.request_id.as_deref().unwrap_or("-")
    );
    println!("request: {} {}", method, uri);
    println!(
        "request body bytes: {}",
        body.as_ref().map(|b| b.len()).unwrap_or(0)
    );
    println!("response status: {}", status);
    println!("response bytes: {}", bytes.len());
    if let Some(captured) = &candidate.captured_response {
        println!(
            "captured response status: {} (diff={})",
            captured.status,
            if captured.status as u16 == status.as_u16() {
                "no"
            } else {
                "yes"
            }
        );
        println!(
            "captured response bytes: {} (diff={})",
            captured
                .body_size
                .map(|x| x.to_string())
                .unwrap_or_else(|| "-".into()),
            match captured.body_size {
                Some(n) if n == bytes.len() => "no",
                Some(_) => "yes",
                None => "unknown",
            }
        );
        println!(
            "captured response headers_hash: {} (diff={})",
            captured.headers_hash,
            if captured.headers_hash == live_headers_hash {
                "no"
            } else {
                "yes"
            }
        );
        println!(
            "captured response body_hash: {} (diff={})",
            captured.body_hash.as_deref().unwrap_or("-"),
            match (&captured.body_hash, &live_body_hash) {
                (Some(a), Some(b)) if a == b => "no",
                (Some(_), Some(_)) => "yes",
                (None, None) => "no",
                _ => "unknown",
            }
        );
    }

    Ok(())
}

fn load_requests(path: &PathBuf) -> Result<Vec<ReplayCandidate>> {
    let file = File::open(path).with_context(|| format!("open flow log {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    let mut req_index_by_request_id: HashMap<String, usize> = HashMap::new();
    let mut req_indexes_by_client: HashMap<String, VecDeque<usize>> = HashMap::new();

    for (i, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let event: ApiEvent = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        match event {
            ApiEvent::HttpRequest {
                timestamp_ms,
                request_id,
                client,
                method,
                uri,
                headers,
                body_b64,
                ..
            } => {
                let body = body_b64
                    .as_deref()
                    .and_then(|v| base64::engine::general_purpose::STANDARD.decode(v).ok());
                out.push(ReplayCandidate {
                    index: i,
                    timestamp_ms,
                    request_id: request_id.clone(),
                    client: client.clone(),
                    method,
                    uri,
                    headers,
                    body,
                    captured_response: None,
                });
                let idx = out.len() - 1;
                if let Some(req_id) = request_id {
                    req_index_by_request_id.insert(req_id, idx);
                }
                req_indexes_by_client
                    .entry(client)
                    .or_default()
                    .push_back(idx);
            }
            ApiEvent::HttpResponse {
                request_id,
                client,
                status,
                headers,
                body_b64,
                body_size,
                ..
            } => {
                let target = request_id
                    .as_deref()
                    .and_then(|id| req_index_by_request_id.get(id).copied())
                    .or_else(|| {
                        req_indexes_by_client
                            .get_mut(&client)
                            .and_then(|q| q.pop_front())
                    });
                if let Some(idx) = target {
                    if let Some(req) = out.get_mut(idx) {
                        let normalized_headers = normalize_headers(headers);
                        let body_hash = body_b64
                            .as_deref()
                            .and_then(|v| base64::engine::general_purpose::STANDARD.decode(v).ok())
                            .map(|v| hash_bytes(&v));
                        req.captured_response = Some(ResponseSnapshot {
                            status,
                            body_size,
                            headers_hash: hash_headers(&normalized_headers),
                            body_hash,
                        });
                    }
                }
            }
            ApiEvent::WebSocketFrame { .. } => {}
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

fn render_curl(method: &str, uri: &str, headers: &HeaderMap) -> String {
    let mut out = format!("curl -X {} '{}'", method, uri);
    for (k, v) in headers {
        if let Ok(val) = v.to_str() {
            out.push_str(&format!(
                " -H '{}: {}'",
                k.as_str(),
                shell_escape_single(val)
            ));
        }
    }
    out
}

fn shell_escape_single(s: &str) -> String {
    s.replace('\'', "'\"'\"'")
}

fn headers_to_pairs(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(k, v)| {
            (
                k.to_string(),
                String::from_utf8_lossy(v.as_bytes()).to_string(),
            )
        })
        .collect()
}

fn normalize_headers(mut headers: Vec<(String, String)>) -> Vec<(String, String)> {
    headers.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    headers
}

fn hash_headers(headers: &[(String, String)]) -> String {
    let mut hasher = Sha256::new();
    for (k, v) in headers {
        hasher.update(k.as_bytes());
        hasher.update(b":");
        hasher.update(v.as_bytes());
        hasher.update(b"\n");
    }
    let out = hasher.finalize();
    to_hex(out.as_slice())
}

fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let out = hasher.finalize();
    to_hex(out.as_slice())
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
