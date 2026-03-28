use crate::api::ApiEvent;
use anyhow::{Context, Result};
use base64::Engine as _;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::PathBuf,
};

#[derive(Debug, Clone)]
pub struct ReplayCandidate {
    pub index: usize,
    pub timestamp_ms: u64,
    pub request_id: Option<String>,
    pub client: String,
    pub method: String,
    pub uri: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub captured_response: Option<ResponseSnapshot>,
}

#[derive(Debug, Clone)]
pub struct ResponseSnapshot {
    pub status: u16,
    pub body_size: Option<usize>,
    pub headers_hash: String,
    pub body_hash: Option<String>,
}

pub fn expand_home(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(stripped);
        }
    }
    path
}

pub fn load_requests(path: &PathBuf) -> Result<Vec<ReplayCandidate>> {
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
