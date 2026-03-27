use anyhow::Result;
use async_trait::async_trait;
use hudsucker::{Body, HttpContext, RequestOrResponse, hyper::Request, hyper::Response};
use std::sync::Arc;

pub mod standard;

#[async_trait]
pub trait HttpFilter: Send + Sync {
    async fn on_request(
        &self,
        _ctx: &HttpContext,
        req: Request<Body>,
    ) -> Result<RequestOrResponse> {
        Ok(req.into())
    }

    async fn on_response(&self, _ctx: &HttpContext, res: Response<Body>) -> Result<Response<Body>> {
        Ok(res)
    }
}

#[derive(Clone, Default)]
pub struct FilterChain {
    filters: Arc<Vec<Arc<dyn HttpFilter>>>,
}

impl FilterChain {
    pub fn new(filters: Vec<Arc<dyn HttpFilter>>) -> Self {
        Self {
            filters: Arc::new(filters),
        }
    }

    pub async fn handle_request(
        &self,
        ctx: &HttpContext,
        mut req: Request<Body>,
    ) -> Result<RequestOrResponse> {
        for filter in self.filters.iter() {
            match filter.on_request(ctx, req).await? {
                RequestOrResponse::Request(next_req) => {
                    req = next_req;
                }
                RequestOrResponse::Response(resp) => {
                    return Ok(resp.into());
                }
            }
        }
        Ok(req.into())
    }

    pub async fn handle_response(
        &self,
        ctx: &HttpContext,
        mut res: Response<Body>,
    ) -> Result<Response<Body>> {
        for filter in self.filters.iter().rev() {
            res = filter.on_response(ctx, res).await?;
        }
        Ok(res)
    }
}
