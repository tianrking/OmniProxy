use super::HttpFilter;
use crate::{
    api::{ApiEvent, ApiHub, now_ms},
    plugins::WasmPluginHost,
};
use anyhow::Result;
use async_trait::async_trait;
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
