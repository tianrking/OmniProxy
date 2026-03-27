use super::{HttpFilter, WebSocketFilter};
use crate::{
    api::{ApiEvent, ApiHub, now_ms},
    plugins::WasmPluginHost,
    rules::{RequestMeta, RuleEngine},
};
use anyhow::Result;
use async_trait::async_trait;
use base64::Engine as _;
use http_body_util::{BodyExt, Full};
use hudsucker::hyper::StatusCode;
use hudsucker::hyper::header::{HeaderName, HeaderValue};
use hudsucker::{Body, HttpContext, RequestOrResponse, hyper::Request, hyper::Response};
use hudsucker::{WebSocketContext, tokio_tungstenite::tungstenite::Message};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct RequestIdFilter;

#[async_trait]
impl HttpFilter for RequestIdFilter {
    async fn on_request(
        &self,
        _ctx: &HttpContext,
        mut req: Request<Body>,
    ) -> Result<RequestOrResponse> {
        if !req.headers().contains_key("x-omni-request-id") {
            let req_id = Uuid::new_v4().to_string();
            req.headers_mut().insert(
                "x-omni-request-id",
                req_id.parse().expect("valid uuid header"),
            );
        }
        Ok(req.into())
    }
}

#[derive(Clone, Default)]
pub struct AccessLogFilter {
    pub hub: Option<ApiHub>,
    pub capture_body_max_bytes: usize,
    inflight_req_ids: Arc<Mutex<HashMap<String, VecDeque<String>>>>,
}

impl AccessLogFilter {
    pub fn with_hub(hub: Option<ApiHub>, capture_body_max_bytes: usize) -> Self {
        Self {
            hub,
            capture_body_max_bytes,
            inflight_req_ids: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn push_req_id(&self, client: &str, req_id: &str) {
        if let Ok(mut m) = self.inflight_req_ids.lock() {
            m.entry(client.to_string())
                .or_insert_with(VecDeque::new)
                .push_back(req_id.to_string());
        }
    }

    fn pop_req_id(&self, client: &str) -> Option<String> {
        let mut guard = self.inflight_req_ids.lock().ok()?;
        let q = guard.get_mut(client)?;
        let val = q.pop_front();
        if q.is_empty() {
            guard.remove(client);
        }
        val
    }
}

#[async_trait]
impl HttpFilter for AccessLogFilter {
    async fn on_request(&self, ctx: &HttpContext, req: Request<Body>) -> Result<RequestOrResponse> {
        let (req, req_body) = capture_request_body(req, self.capture_body_max_bytes).await?;
        let client = ctx.client_addr.to_string();
        let request_id = req
            .headers()
            .get("x-omni-request-id")
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned);
        if let Some(req_id) = request_id.as_deref() {
            self.push_req_id(&client, req_id);
        }
        if let Some(hub) = &self.hub {
            hub.publish(ApiEvent::HttpRequest {
                timestamp_ms: now_ms(),
                request_id,
                client: client.clone(),
                method: req.method().to_string(),
                uri: req.uri().to_string(),
                headers: req
                    .headers()
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.to_string(),
                            String::from_utf8_lossy(v.as_bytes()).to_string(),
                        )
                    })
                    .collect(),
                body_b64: req_body.body_b64,
                body_truncated: req_body.body_truncated,
                body_size: req_body.body_size,
            });
        }
        info!(
            client = %ctx.client_addr,
            method = %req.method(),
            uri = %req.uri(),
            "request"
        );
        Ok(req.into())
    }

    async fn on_response(&self, ctx: &HttpContext, res: Response<Body>) -> Result<Response<Body>> {
        let (mut res, res_body) = capture_response_body(res, self.capture_body_max_bytes).await?;
        let client = ctx.client_addr.to_string();
        let from_header = res
            .headers()
            .get("x-omni-request-id")
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned);
        let request_id = from_header.or_else(|| self.pop_req_id(&client));
        if let Some(req_id) = request_id.as_deref() {
            res.headers_mut().insert(
                "x-omni-request-id",
                req_id.parse().expect("valid request id header"),
            );
        }
        if let Some(hub) = &self.hub {
            hub.publish(ApiEvent::HttpResponse {
                timestamp_ms: now_ms(),
                request_id,
                client: client.clone(),
                status: res.status().as_u16(),
                headers: res
                    .headers()
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.to_string(),
                            String::from_utf8_lossy(v.as_bytes()).to_string(),
                        )
                    })
                    .collect(),
                body_b64: res_body.body_b64,
                body_truncated: res_body.body_truncated,
                body_size: res_body.body_size,
            });
        }
        info!(
            client = %ctx.client_addr,
            status = %res.status(),
            "response"
        );
        Ok(res)
    }
}

#[derive(Clone)]
pub struct WasmFilter {
    host: Arc<WasmPluginHost>,
}

impl WasmFilter {
    pub fn new(host: Arc<WasmPluginHost>) -> Self {
        Self { host }
    }
}

#[async_trait]
impl HttpFilter for WasmFilter {
    async fn on_request(
        &self,
        _ctx: &HttpContext,
        req: Request<Body>,
    ) -> Result<RequestOrResponse> {
        if let Err(err) = self.host.eval_request_isolated(&req).await {
            // Fail-open: plugin faults must never crash or block the proxy core.
            warn!(error = %err, "wasm request hook failed");
        }
        Ok(req.into())
    }

    async fn on_response(&self, _ctx: &HttpContext, res: Response<Body>) -> Result<Response<Body>> {
        if let Err(err) = self.host.eval_response_isolated(&res).await {
            warn!(error = %err, "wasm response hook failed");
        }
        Ok(res)
    }
}

#[derive(Clone)]
pub struct RuleFilter {
    rules: Arc<RuleEngine>,
    inflight_req_meta: Arc<Mutex<HashMap<String, VecDeque<RequestMeta>>>>,
    inflight_req_meta_by_id: Arc<Mutex<HashMap<String, RequestMeta>>>,
}

impl RuleFilter {
    pub fn new(rules: Arc<RuleEngine>) -> Self {
        Self {
            rules,
            inflight_req_meta: Arc::new(Mutex::new(HashMap::new())),
            inflight_req_meta_by_id: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn push_meta(&self, client: &str, meta: RequestMeta) {
        if let Ok(mut m) = self.inflight_req_meta.lock() {
            m.entry(client.to_string())
                .or_insert_with(VecDeque::new)
                .push_back(meta);
        }
    }

    fn pop_meta(&self, client: &str) -> Option<RequestMeta> {
        let mut guard = self.inflight_req_meta.lock().ok()?;
        let q = guard.get_mut(client)?;
        let val = q.pop_front();
        if q.is_empty() {
            guard.remove(client);
        }
        val
    }

    fn put_meta_by_req_id(&self, req_id: &str, meta: RequestMeta) {
        if let Ok(mut m) = self.inflight_req_meta_by_id.lock() {
            m.insert(req_id.to_string(), meta);
        }
    }

    fn take_meta_by_req_id(&self, req_id: &str) -> Option<RequestMeta> {
        let mut guard = self.inflight_req_meta_by_id.lock().ok()?;
        guard.remove(req_id)
    }
}

#[async_trait]
impl HttpFilter for RuleFilter {
    async fn on_request(
        &self,
        ctx: &HttpContext,
        mut req: Request<Body>,
    ) -> Result<RequestOrResponse> {
        let meta = RequestMeta {
            method: req.method().as_str().to_string(),
            uri: req.uri().to_string(),
            host: extract_host(&req),
        };
        let decision = self.rules.eval_request(&meta);
        if decision.denied {
            let denied = Response::builder()
                .status(403)
                .header("content-type", "text/plain; charset=utf-8")
                .body(Body::from("blocked by OmniProxy rule-engine"))
                .expect("build deny response");
            return Ok(denied.into());
        }

        for (k, v) in decision.add_headers {
            if let (Ok(name), Ok(value)) = (k.parse::<HeaderName>(), v.parse::<HeaderValue>()) {
                req.headers_mut().insert(name, value);
            }
        }

        if let Some(req_id) = req
            .headers()
            .get("x-omni-request-id")
            .and_then(|v| v.to_str().ok())
        {
            self.put_meta_by_req_id(req_id, meta.clone());
        }
        self.push_meta(&ctx.client_addr.to_string(), meta);
        Ok(req.into())
    }

    async fn on_response(
        &self,
        ctx: &HttpContext,
        mut res: Response<Body>,
    ) -> Result<Response<Body>> {
        let client = ctx.client_addr.to_string();
        let fallback_meta = RequestMeta {
            method: String::new(),
            uri: String::new(),
            host: String::new(),
        };
        let req_id = res
            .headers()
            .get("x-omni-request-id")
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned);
        let meta = req_id
            .as_deref()
            .and_then(|id| self.take_meta_by_req_id(id))
            .or_else(|| self.pop_meta(&client))
            .unwrap_or(fallback_meta);
        let outcome = self.rules.eval_response(&meta, res.status().as_u16());
        for (k, v) in outcome.add_headers {
            if let (Ok(name), Ok(value)) = (k.parse::<HeaderName>(), v.parse::<HeaderValue>()) {
                res.headers_mut().insert(name, value);
            }
        }
        if let Some(code) = outcome.override_status {
            if let Ok(status) = StatusCode::from_u16(code) {
                *res.status_mut() = status;
            }
        }
        if let Some(body) = outcome.replace_body {
            *res.body_mut() = Body::from(body);
        }
        Ok(res)
    }
}

fn extract_host(req: &Request<Body>) -> String {
    if let Some(host) = req.uri().host() {
        return host.to_string();
    }
    req.headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned)
        .unwrap_or_default()
}

#[derive(Clone, Default)]
pub struct WsAccessLogFilter {
    pub hub: Option<ApiHub>,
    pub preview_bytes: usize,
}

impl WsAccessLogFilter {
    pub fn with_hub(hub: Option<ApiHub>, preview_bytes: usize) -> Self {
        Self { hub, preview_bytes }
    }
}

#[async_trait]
impl WebSocketFilter for WsAccessLogFilter {
    async fn on_message(&self, _ctx: &WebSocketContext, msg: Message) -> Result<Option<Message>> {
        let (kind, payload_len, preview) = match &msg {
            Message::Text(text) => (
                "text".to_string(),
                text.len(),
                Some(truncate_preview(text.as_str(), self.preview_bytes)),
            ),
            Message::Binary(bin) => (
                "binary".to_string(),
                bin.len(),
                Some(format!("<binary:{} bytes>", bin.len())),
            ),
            Message::Ping(bin) => ("ping".to_string(), bin.len(), None),
            Message::Pong(bin) => ("pong".to_string(), bin.len(), None),
            Message::Close(frame) => (
                "close".to_string(),
                0,
                frame
                    .as_ref()
                    .map(|f| format!("code={} reason={}", f.code, f.reason)),
            ),
            Message::Frame(_) => ("frame".to_string(), 0, None),
        };

        if let Some(hub) = &self.hub {
            hub.publish(ApiEvent::WebSocketFrame {
                timestamp_ms: now_ms(),
                client: None,
                kind,
                payload_len,
                preview,
            });
        }
        Ok(Some(msg))
    }
}

#[derive(Clone, Default)]
pub struct WsMutationFilter {
    drop_ping: bool,
    text_rewrites: Vec<(String, String)>,
}

impl WsMutationFilter {
    pub fn new(drop_ping: bool, text_rewrites: Vec<(String, String)>) -> Self {
        Self {
            drop_ping,
            text_rewrites,
        }
    }
}

#[async_trait]
impl WebSocketFilter for WsMutationFilter {
    async fn on_message(&self, _ctx: &WebSocketContext, msg: Message) -> Result<Option<Message>> {
        let msg = match msg {
            Message::Ping(_) if self.drop_ping => return Ok(None),
            Message::Text(text) => {
                let mut out = text.to_string();
                for (from, to) in &self.text_rewrites {
                    if !from.is_empty() {
                        out = out.replace(from, to);
                    }
                }
                Message::Text(out.into())
            }
            other => other,
        };
        Ok(Some(msg))
    }
}

fn truncate_preview(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_string();
    }
    let mut end = max_bytes.min(input.len());
    while !input.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &input[..end])
}

#[derive(Debug, Clone, Default)]
struct CapturedBody {
    body_b64: Option<String>,
    body_truncated: bool,
    body_size: Option<usize>,
}

async fn capture_request_body(
    req: Request<Body>,
    max_bytes: usize,
) -> Result<(Request<Body>, CapturedBody)> {
    let (parts, body) = req.into_parts();
    let Some(content_len) = content_length(parts.headers.get("content-length")) else {
        return Ok((Request::from_parts(parts, body), CapturedBody::default()));
    };
    if max_bytes == 0 || content_len > max_bytes {
        return Ok((
            Request::from_parts(parts, body),
            CapturedBody {
                body_b64: None,
                body_truncated: true,
                body_size: Some(content_len),
            },
        ));
    }

    let bytes = body.collect().await?.to_bytes();
    let encoded = if bytes.is_empty() {
        None
    } else {
        Some(base64::engine::general_purpose::STANDARD.encode(bytes.as_ref()))
    };
    Ok((
        Request::from_parts(parts, Body::from(Full::new(bytes))),
        CapturedBody {
            body_b64: encoded,
            body_truncated: false,
            body_size: Some(content_len),
        },
    ))
}

async fn capture_response_body(
    res: Response<Body>,
    max_bytes: usize,
) -> Result<(Response<Body>, CapturedBody)> {
    let (parts, body) = res.into_parts();
    let Some(content_len) = content_length(parts.headers.get("content-length")) else {
        return Ok((Response::from_parts(parts, body), CapturedBody::default()));
    };
    if max_bytes == 0 || content_len > max_bytes {
        return Ok((
            Response::from_parts(parts, body),
            CapturedBody {
                body_b64: None,
                body_truncated: true,
                body_size: Some(content_len),
            },
        ));
    }

    let bytes = body.collect().await?.to_bytes();
    let encoded = if bytes.is_empty() {
        None
    } else {
        Some(base64::engine::general_purpose::STANDARD.encode(bytes.as_ref()))
    };
    Ok((
        Response::from_parts(parts, Body::from(Full::new(bytes))),
        CapturedBody {
            body_b64: encoded,
            body_truncated: false,
            body_size: Some(content_len),
        },
    ))
}

fn content_length(v: Option<&HeaderValue>) -> Option<usize> {
    v.and_then(|x| x.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok())
}
