use anyhow::{Context, Result};
use clap::Parser;
use omni_proxy::api::ApiEvent;
use std::collections::{HashMap, VecDeque};
use std::{fs::File, io::BufRead, io::BufReader, path::PathBuf};

#[derive(Debug, Parser)]
#[command(name = "omni-analyze", about = "Analyze OmniProxy flow logs for diagnostics")]
struct Cli {
    #[arg(long, default_value = "~/.omni-proxy/flows.jsonl")]
    flow_log: PathBuf,

    #[arg(long, default_value_t = 10)]
    top: usize,

    #[arg(long, default_value_t = 1000)]
    slow_ms: u64,

    #[arg(long, default_value_t = false)]
    include_connect: bool,
}

#[derive(Debug, Clone)]
struct PendingReq {
    request_id: Option<String>,
    client: String,
    method: String,
    uri: String,
    host: String,
    req_ts: u64,
}

#[derive(Debug, Clone)]
struct CompletedReq {
    method: String,
    uri: String,
    host: String,
    status: u16,
    latency_ms: u64,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let path = expand_home(cli.flow_log);
    let file = File::open(&path).with_context(|| format!("open flow log {}", path.display()))?;
    let reader = BufReader::new(file);

    let mut req_by_id: HashMap<String, PendingReq> = HashMap::new();
    let mut req_queue_by_client: HashMap<String, VecDeque<PendingReq>> = HashMap::new();
    let mut completed: Vec<CompletedReq> = Vec::new();
    let mut ws_frames: usize = 0;
    let mut ws_bytes: usize = 0;

    for line in reader.lines() {
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
                ..
            } => {
                let host = parse_host_from_uri(&uri).unwrap_or("-").to_string();
                let p = PendingReq {
                    request_id: request_id.clone(),
                    client: client.clone(),
                    method,
                    uri,
                    host,
                    req_ts: timestamp_ms,
                };
                if let Some(id) = request_id {
                    req_by_id.insert(id, p.clone());
                }
                req_queue_by_client.entry(client).or_default().push_back(p);
            }
            ApiEvent::HttpResponse {
                timestamp_ms,
                request_id,
                client,
                status,
                ..
            } => {
                let pending = request_id
                    .as_deref()
                    .and_then(|id| req_by_id.remove(id))
                    .or_else(|| req_queue_by_client.get_mut(&client).and_then(|q| q.pop_front()));

                if let Some(p) = pending {
                    let req_ts = p.req_ts;
                    let req_uri = p.uri.clone();
                    let latency_ms = timestamp_ms.saturating_sub(p.req_ts);
                    completed.push(CompletedReq {
                        method: p.method,
                        uri: p.uri,
                        host: p.host,
                        status,
                        latency_ms,
                    });
                    if let Some(req_id) = p.request_id {
                        req_by_id.remove(&req_id);
                    }
                    if let Some(q) = req_queue_by_client.get_mut(&p.client)
                        && let Some(pos) = q
                            .iter()
                            .position(|x| x.req_ts == req_ts && x.uri == req_uri)
                    {
                        let _ = q.remove(pos);
                    }
                }
            }
            ApiEvent::WebSocketFrame { payload_len, .. } => {
                ws_frames = ws_frames.saturating_add(1);
                ws_bytes = ws_bytes.saturating_add(payload_len);
            }
        }
    }

    print_report(
        &completed,
        ws_frames,
        ws_bytes,
        cli.top,
        cli.slow_ms,
        cli.include_connect,
    );

    Ok(())
}

fn print_report(
    rows: &[CompletedReq],
    ws_frames: usize,
    ws_bytes: usize,
    top: usize,
    slow_ms: u64,
    include_connect: bool,
) {
    let filtered: Vec<&CompletedReq> = rows
        .iter()
        .filter(|r| include_connect || !r.method.eq_ignore_ascii_case("CONNECT"))
        .collect();
    let total = filtered.len();
    let status_4xx_5xx = filtered.iter().filter(|r| r.status >= 400).count();
    let error_rate = if total == 0 {
        0.0
    } else {
        (status_4xx_5xx as f64 * 100.0) / total as f64
    };

    let mut latencies: Vec<u64> = filtered.iter().map(|r| r.latency_ms).collect();
    latencies.sort_unstable();

    println!("=== Omni Analyze ===");
    println!("total_http: {}", total);
    println!("http_error(>=400): {} ({:.2}%)", status_4xx_5xx, error_rate);
    println!("latency_p50_ms: {}", percentile(&latencies, 50.0));
    println!("latency_p95_ms: {}", percentile(&latencies, 95.0));
    println!("latency_p99_ms: {}", percentile(&latencies, 99.0));
    println!("websocket_frames: {}", ws_frames);
    println!("websocket_bytes: {}", ws_bytes);
    if total == 0 && !include_connect {
        println!("note: no non-CONNECT rows; try --include-connect");
    }
    println!();

    let mut by_host: HashMap<String, usize> = HashMap::new();
    let mut by_status: HashMap<u16, usize> = HashMap::new();
    let mut by_method: HashMap<String, usize> = HashMap::new();
    let mut slow: Vec<&CompletedReq> = filtered
        .iter()
        .copied()
        .filter(|r| r.latency_ms >= slow_ms)
        .collect();

    for r in filtered {
        *by_host.entry(r.host.clone()).or_insert(0) += 1;
        *by_status.entry(r.status).or_insert(0) += 1;
        *by_method.entry(r.method.clone()).or_insert(0) += 1;
    }

    println!("-- top hosts --");
    for (host, cnt) in top_k(by_host, top) {
        println!("{}\t{}", cnt, host);
    }

    println!("-- status distribution --");
    let mut statuses: Vec<(u16, usize)> = by_status.into_iter().collect();
    statuses.sort_by_key(|(s, _)| *s);
    for (status, cnt) in statuses {
        println!("{}\t{}", cnt, status);
    }

    println!("-- method distribution --");
    for (method, cnt) in top_k(by_method, top) {
        println!("{}\t{}", cnt, method);
    }

    println!("-- slow requests (>= {} ms, top {}) --", slow_ms, top);
    slow.sort_by(|a, b| b.latency_ms.cmp(&a.latency_ms));
    for r in slow.into_iter().take(top) {
        println!("{}ms\t{}\t{}\t{}", r.latency_ms, r.status, r.method, r.uri);
    }
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (p / 100.0) * (sorted.len().saturating_sub(1) as f64);
    let idx = rank.round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn top_k<K: Ord + Clone + std::hash::Hash + Eq>(map: HashMap<K, usize>, k: usize) -> Vec<(K, usize)> {
    let mut v: Vec<(K, usize)> = map.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.into_iter().take(k.max(1)).collect()
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

fn parse_host_from_uri(uri: &str) -> Option<&str> {
    let rest = uri
        .strip_prefix("http://")
        .or_else(|| uri.strip_prefix("https://"))?;
    Some(rest.split('/').next()?.split(':').next().unwrap_or(""))
}
