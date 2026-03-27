use super::HttpFilter;
use crate::{
    api::{ApiEvent, ApiHub, now_ms},
    plugins::WasmPluginHost,
    rules::{RequestMeta, RuleEngine},
};
use anyhow::Result;
use async_trait::async_trait;
use hudsucker::hyper::StatusCode;
use hudsucker::hyper::header::{HeaderName, HeaderValue};
use hudsucker::{Body, HttpContext, RequestOrResponse, hyper::Request, hyper::Response};
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
    inflight_req_ids: Arc<Mutex<HashMap<String, VecDeque<String>>>>,
}

impl AccessLogFilter {
    pub fn with_hub(hub: Option<ApiHub>) -> Self {
        Self {
            hub,
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

    async fn on_response(
        &self,
        ctx: &HttpContext,
        mut res: Response<Body>,
    ) -> Result<Response<Body>> {
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
