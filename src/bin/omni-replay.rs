use anyhow::{Context, Result, bail};
use clap::Parser;
use omni_proxy::replay::{ReplayCandidate, expand_home, load_requests};
use reqwest::Method;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use sha2::{Digest, Sha256};
use std::{io::Write, path::PathBuf};

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

    #[arg(long = "drop-header")]
    drop_headers: Vec<String>,

    #[arg(long = "query")]
    query_overrides: Vec<String>,

    #[arg(long)]
    body_text: Option<String>,

    #[arg(long)]
    body_file: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    interactive: bool,

    #[arg(long, default_value_t = false)]
    dry_run: bool,

    #[arg(long, default_value_t = false)]
    print_curl: bool,

    #[arg(long, default_value_t = false)]
    no_body: bool,

    #[arg(long)]
    session_client: Option<String>,

    #[arg(long, default_value_t = 20)]
    session_limit: usize,

    #[arg(long)]
    client: Option<String>,

    #[arg(long)]
    since_ms: Option<u64>,

    #[arg(long)]
    until_ms: Option<u64>,

    #[arg(long, default_value_t = false)]
    exclude_connect: bool,

    #[arg(long, default_value_t = 20)]
    batch_limit: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let path = expand_home(cli.flow_log.clone());
    let requests = load_requests(&path)?;
    let filtered = filtered_candidates(&cli, &requests);

    if cli.list {
        for req in filtered {
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

    if let Some(client_key) = &cli.session_client {
        let selected: Vec<&ReplayCandidate> = filtered_candidates(&cli, &requests)
            .into_iter()
            .filter(|r| &r.client == client_key)
            .take(cli.session_limit.max(1))
            .collect();
        if selected.is_empty() {
            bail!("session client '{}' not found", client_key);
        }
        println!(
            "session replay start: client={} count={}",
            client_key,
            selected.len()
        );
        for req in selected {
            replay_one(&cli, req).await?;
        }
        println!("session replay finished");
        return Ok(());
    }

    if let Some(request_id) = &cli.request_id {
        let candidate = requests
            .iter()
            .find(|x| x.request_id.as_deref() == Some(request_id.as_str()))
            .with_context(|| format!("request_id {} not found", request_id))?;
        return replay_one(&cli, candidate).await;
    }

    if let Some(idx) = cli.index {
        let candidate = requests
            .iter()
            .find(|x| x.index == idx)
            .with_context(|| format!("index {} not found", idx))?;
        return replay_one(&cli, candidate).await;
    }

    let selected: Vec<&ReplayCandidate> = filtered_candidates(&cli, &requests)
        .into_iter()
        .take(cli.batch_limit.max(1))
        .collect();
    if selected.is_empty() {
        bail!(
            "no candidate found. try --list or relax filters (client/since-ms/until-ms/exclude-connect)"
        );
    }

    println!("batch replay start: count={}", selected.len());
    for req in selected {
        replay_one(&cli, req).await?;
    }
    println!("batch replay finished");
    Ok(())
}

async fn replay_one(cli: &Cli, candidate: &ReplayCandidate) -> Result<()> {
    let mut method = cli
        .method_override
        .as_deref()
        .unwrap_or(&candidate.method)
        .to_uppercase();
    let mut uri = cli
        .url_override
        .as_deref()
        .unwrap_or(&candidate.uri)
        .to_string();
    if !cli.query_overrides.is_empty() {
        uri = apply_query_overrides(&uri, &cli.query_overrides)?;
    }
    if !uri.starts_with("http://") && !uri.starts_with("https://") {
        bail!(
            "URI is not absolute and cannot be replayed directly: {}",
            uri
        );
    }
    let mut extra_headers = cli.headers.clone();
    let mut drop_headers = cli.drop_headers.clone();
    let mut body = resolve_body(cli, candidate)?;

    if cli.interactive {
        interactive_edit(
            &mut method,
            &mut uri,
            &mut extra_headers,
            &mut drop_headers,
            &mut body,
        )?;
    }

    let method = Method::from_bytes(method.as_bytes())
        .with_context(|| format!("invalid method: {}", method))?;
    let headers = build_headers(&candidate.headers, &extra_headers, &drop_headers)?;

    if cli.print_curl || cli.dry_run {
        println!(
            "{}",
            render_curl(&method.to_string(), &uri, &headers, body.as_deref())
        );
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

fn resolve_body(cli: &Cli, candidate: &ReplayCandidate) -> Result<Option<Vec<u8>>> {
    if cli.no_body {
        return Ok(None);
    }
    if let Some(v) = &cli.body_text {
        return Ok(Some(v.as_bytes().to_vec()));
    }
    if let Some(path) = &cli.body_file {
        let resolved = expand_home(path.clone());
        return std::fs::read(&resolved)
            .with_context(|| format!("read body file {}", resolved.display()))
            .map(Some);
    }
    Ok(candidate.body.clone())
}

fn filtered_candidates<'a>(cli: &Cli, requests: &'a [ReplayCandidate]) -> Vec<&'a ReplayCandidate> {
    requests
        .iter()
        .filter(|r| {
            if let Some(client) = &cli.client
                && &r.client != client
            {
                return false;
            }
            if let Some(since) = cli.since_ms
                && r.timestamp_ms < since
            {
                return false;
            }
            if let Some(until) = cli.until_ms
                && r.timestamp_ms > until
            {
                return false;
            }
            if cli.exclude_connect && r.method.eq_ignore_ascii_case("CONNECT") {
                return false;
            }
            true
        })
        .collect()
}

fn build_headers(
    captured: &[(String, String)],
    overrides: &[String],
    drop_headers: &[String],
) -> Result<HeaderMap> {
    let mut map = HeaderMap::new();
    let drop_set: std::collections::HashSet<String> = drop_headers
        .iter()
        .map(|h| h.trim().to_ascii_lowercase())
        .collect();
    for (k, v) in captured {
        if is_hop_by_hop(k) {
            continue;
        }
        if drop_set.contains(&k.to_ascii_lowercase()) {
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

fn render_curl(method: &str, uri: &str, headers: &HeaderMap, body: Option<&[u8]>) -> String {
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
    if let Some(bytes) = body {
        if let Ok(s) = std::str::from_utf8(bytes) {
            out.push_str(&format!(" --data-raw '{}'", shell_escape_single(s)));
        } else {
            out.push_str(" --data-binary '<non-utf8-body>'");
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

fn apply_query_overrides(uri: &str, pairs: &[String]) -> Result<String> {
    let mut url = reqwest::Url::parse(uri).with_context(|| format!("invalid url: {}", uri))?;
    for item in pairs {
        let (k, v) = item
            .split_once('=')
            .with_context(|| format!("invalid --query '{}', expected key=value", item))?;
        let mut qp = url.query_pairs_mut();
        qp.append_pair(k.trim(), v.trim());
    }
    Ok(url.to_string())
}

fn interactive_edit(
    method: &mut String,
    uri: &mut String,
    headers: &mut Vec<String>,
    drop_headers: &mut Vec<String>,
    body: &mut Option<Vec<u8>>,
) -> Result<()> {
    println!("\n[interactive replay edit]");
    if let Some(v) = prompt_default("method", method)? {
        *method = v.to_uppercase();
    }
    if let Some(v) = prompt_default("uri", uri)? {
        *uri = v;
    }

    loop {
        let line = prompt("add header (Key: Value), empty to continue")?;
        if line.trim().is_empty() {
            break;
        }
        headers.push(line);
    }

    loop {
        let line = prompt("drop header name, empty to continue")?;
        if line.trim().is_empty() {
            break;
        }
        drop_headers.push(line);
    }

    println!("body mode: [k]eep, [e]mpty, [t]ext, [f]ile");
    let mode = prompt("choose body mode (default: keep)")?;
    match mode.trim() {
        "" | "k" | "keep" => {}
        "e" | "empty" => *body = None,
        "t" | "text" => {
            let text = prompt("body text")?;
            *body = Some(text.into_bytes());
        }
        "f" | "file" => {
            let p = prompt("body file path")?;
            if !p.trim().is_empty() {
                let file = expand_home(PathBuf::from(p.trim()));
                *body =
                    Some(std::fs::read(&file).with_context(|| {
                        format!("read interactive body file {}", file.display())
                    })?);
            }
        }
        other => {
            bail!("invalid body mode: {}", other);
        }
    }

    Ok(())
}

fn prompt(label: &str) -> Result<String> {
    print!("{}: ", label);
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

fn prompt_default(label: &str, current: &str) -> Result<Option<String>> {
    print!("{} [{}]: ", label, current);
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let line = line.trim_end_matches(['\r', '\n']).trim().to_string();
    if line.is_empty() {
        Ok(None)
    } else {
        Ok(Some(line))
    }
}
