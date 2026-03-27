use super::HttpFilter;
use crate::{api::ApiEvent, api::ApiHub, plugins::WasmPluginHost};
use anyhow::Result;
use async_trait::async_trait;
use hudsucker::{Body, HttpContext, RequestOrResponse, hyper::Request, hyper::Response};
use std::sync::Arc;
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
}

#[async_trait]
impl HttpFilter for AccessLogFilter {
    async fn on_request(&self, ctx: &HttpContext, req: Request<Body>) -> Result<RequestOrResponse> {
        if let Some(hub) = &self.hub {
            hub.publish(ApiEvent::HttpRequest {
                client: ctx.client_addr.to_string(),
                method: req.method().to_string(),
                uri: req.uri().to_string(),
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
        if let Some(hub) = &self.hub {
            hub.publish(ApiEvent::HttpResponse {
                client: ctx.client_addr.to_string(),
                status: res.status().as_u16(),
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
