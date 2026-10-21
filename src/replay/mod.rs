//! Capture loading with explicit integrity and correlation diagnostics.
use crate::api::ApiEvent;
use anyhow::{Context, Result, bail};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseSnapshot {
    pub status: u16,
    pub body_size: Option<usize>,
    pub headers_hash: String,
    pub body_hash: Option<String>,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub timestamp_ms: u64,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Capture {
    pub requests: Vec<ReplayCandidate>,
    pub issues: Vec<String>,
    pub skipped_events: u64,
}

impl ReplayCandidate {
    pub fn replayable(&self) -> bool {
        self.issues.is_empty() && !self.method.eq_ignore_ascii_case("CONNECT")
    }
}

pub fn expand_home(path: PathBuf) -> PathBuf {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
    {
        return PathBuf::from(home).join(rest);
    }
    path
}

pub fn load_requests(path: &PathBuf) -> Result<Vec<ReplayCandidate>> {
    Ok(load_capture(path)?.requests)
}

pub fn load_capture(path: &Path) -> Result<Capture> {
    let file = File::open(path).with_context(|| format!("open capture {}", path.display()))?;
    if file.metadata()?.len() > 64 * 1024 * 1024 {
        bail!("capture exceeds 64 MiB; select a smaller recording or rotated segment");
    }
    parse_capture(BufReader::new(file))
}

pub fn parse_capture(reader: impl BufRead) -> Result<Capture> {
    let mut capture = Capture::default();
    let mut by_id = HashMap::<String, usize>::new();
    let mut by_client = HashMap::<String, VecDeque<usize>>::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > 2 * 1024 * 1024 {
            capture
                .issues
                .push(format!("line {} exceeds 2 MiB", index + 1));
            continue;
        }
        let value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                capture
                    .issues
                    .push(format!("invalid JSON at line {}", index + 1));
                continue;
            }
        };
        if value["type"] == "CaptureGap" {
            let skipped = value["skipped"].as_u64().unwrap_or(1);
            capture.skipped_events = capture.skipped_events.saturating_add(skipped);
            capture.issues.push(format!(
                "capture lost {skipped} events at line {}",
                index + 1
            ));
            continue;
        }
        let event: ApiEvent = match serde_json::from_value(value) {
            Ok(event) => event,
            Err(_) => {
                capture
                    .issues
                    .push(format!("unrecognized capture event at line {}", index + 1));
                continue;
            }
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
                body_truncated,
                body_size,
                body_capture_reason,
            } => {
                let (body, mut issues) =
                    decode_body(body_b64, body_truncated, body_size, body_capture_reason);
                if let Some(id) = &request_id
                    && let Some(previous) = by_id.insert(id.clone(), capture.requests.len())
                {
                    issues.push("duplicate request ID".into());
                    capture.requests[previous]
                        .issues
                        .push("duplicate request ID".into());
                }
                let position = capture.requests.len();
                by_client
                    .entry(client.clone())
                    .or_default()
                    .push_back(position);
                capture.requests.push(ReplayCandidate {
                    index,
                    timestamp_ms,
                    request_id,
                    client,
                    method,
                    uri,
                    headers,
                    body,
                    captured_response: None,
                    issues,
                });
            }
            ApiEvent::HttpResponse {
                timestamp_ms,
                request_id,
                client,
                status,
                headers,
                body_b64,
                body_truncated,
                body_size,
                body_capture_reason,
            } => {
                // An explicit unknown ID must never steal another request's response.
                let position = if let Some(id) = &request_id {
                    by_id.remove(id)
                } else {
                    let queue = by_client.entry(client.clone()).or_default();
                    let pending: Vec<_> = queue
                        .iter()
                        .copied()
                        .filter(|i| capture.requests[*i].captured_response.is_none())
                        .collect();
                    if pending.len() > 1 {
                        capture.issues.push(format!(
                            "ambiguous response without request ID at line {}",
                            index + 1
                        ));
                        None
                    } else {
                        pending.first().copied()
                    }
                };
                if let Some(position) = position {
                    let request = &mut capture.requests[position];
                    if request.client != client || request.captured_response.is_some() {
                        capture
                            .issues
                            .push(format!("conflicting response at line {}", index + 1));
                        continue;
                    }
                    if let Some(id) = &request.request_id {
                        by_id.remove(id);
                    }
                    if let Some(queue) = by_client.get_mut(&client) {
                        queue.retain(|i| *i != position);
                    }
                    let (body, issues) =
                        decode_body(body_b64, body_truncated, body_size, body_capture_reason);
                    let mut normalized = headers.clone();
                    normalized.sort();
                    // Preserve the digest contract used by omni-replay.
                    let mut digest = Sha256::new();
                    for (name, value) in &normalized {
                        digest.update(name.as_bytes());
                        digest.update(b":");
                        digest.update(value.as_bytes());
                        digest.update(b"\n");
                    }
                    let headers_hash = digest
                        .finalize()
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect();
                    request.captured_response = Some(ResponseSnapshot {
                        status,
                        body_size,
                        headers_hash,
                        body_hash: body.as_ref().map(|v| hash_bytes(v)),
                        headers,
                        body,
                        timestamp_ms,
                        issues,
                    });
                } else {
                    capture
                        .issues
                        .push(format!("unpaired response at line {}", index + 1));
                }
            }
            ApiEvent::WebSocketFrame { .. } => {}
        }
    }
    for request in &capture.requests {
        if request.captured_response.is_none() && !request.method.eq_ignore_ascii_case("CONNECT") {
            capture.issues.push(format!(
                "request at line {} has no captured response",
                request.index + 1
            ));
        }
    }
    Ok(capture)
}

fn decode_body(
    encoded: Option<String>,
    truncated: bool,
    size: Option<usize>,
    reason: Option<String>,
) -> (Option<Vec<u8>>, Vec<String>) {
    let mut issues = Vec::new();
    if truncated {
        issues.push("body capture is truncated".into());
    }
    let body = match encoded {
        Some(value) => match base64::engine::general_purpose::STANDARD.decode(value) {
            Ok(body) => Some(body),
            Err(_) => {
                issues.push("invalid base64 body".into());
                None
            }
        },
        None => None,
    };
    if let Some(size) = size
        && size > 0
        && body.as_ref().map(Vec::len) != Some(size)
    {
        issues.push("captured body size does not match payload".into());
    }
    if body.is_none()
        && let Some(reason) = reason
        && !matches!(reason.as_str(), "empty" | "no-body")
    {
        issues.push(format!("body unavailable: {reason}"));
    }
    (body, issues)
}

fn hash_bytes(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    fn req(id: &str) -> Value {
        json!({"type":"HttpRequest","request_id":id,"client":"c","method":"POST","uri":"http://localhost/item","body_b64":"e30=","body_size":2})
    }
    fn res(id: &str, status: u16) -> Value {
        json!({"type":"HttpResponse","request_id":id,"client":"c","status":status,"body_b64":"e30=","body_size":2})
    }
    fn read(events: Vec<Value>) -> Capture {
        parse_capture(std::io::Cursor::new(
            events
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        ))
        .unwrap()
    }
    #[test]
    fn multiplexed_responses_pair_by_id_not_queue_order() {
        let capture = read(vec![req("a"), req("b"), res("b", 202), res("a", 201)]);
        assert_eq!(
            capture.requests[0]
                .captured_response
                .as_ref()
                .unwrap()
                .status,
            201
        );
        assert_eq!(
            capture.requests[1]
                .captured_response
                .as_ref()
                .unwrap()
                .status,
            202
        );
        assert!(capture.issues.is_empty());
    }
    #[test]
    fn unknown_id_does_not_consume_another_request() {
        let capture = read(vec![req("a"), res("unknown", 500), res("a", 201)]);
        assert_eq!(
            capture.requests[0]
                .captured_response
                .as_ref()
                .unwrap()
                .status,
            201
        );
        assert_eq!(capture.issues.len(), 1);
    }
    #[test]
    fn truncated_or_invalid_request_is_not_replayable() {
        let mut request = req("a");
        request["body_truncated"] = json!(true);
        request["body_b64"] = json!("!");
        let capture = read(vec![request, res("a", 200)]);
        assert!(!capture.requests[0].replayable());
        assert_eq!(capture.requests[0].issues.len(), 3);
    }
    #[test]
    fn duplicate_ids_and_capture_gaps_are_visible() {
        let capture = read(vec![
            req("a"),
            req("a"),
            json!({"type":"CaptureGap","skipped":12}),
        ]);
        assert!(!capture.requests[0].replayable());
        assert!(!capture.requests[1].replayable());
        assert_eq!(capture.skipped_events, 12);
    }
    #[test]
    fn ambiguous_response_is_not_assigned_a_false_baseline() {
        let mut response = res("a", 200);
        response.as_object_mut().unwrap().remove("request_id");
        let capture = read(vec![req("a"), req("b"), response]);
        assert!(
            capture
                .requests
                .iter()
                .all(|r| r.captured_response.is_none())
        );
    }
    #[test]
    fn malformed_json_is_reported() {
        let capture = parse_capture(std::io::Cursor::new("not json\n")).unwrap();
        assert!(capture.issues[0].contains("invalid JSON"));
    }
}
