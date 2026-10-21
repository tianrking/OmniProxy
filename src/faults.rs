//! A temporary, per-run HTTP fault gateway. Faults never alter the saved baseline.
use crate::scenario::{RunReport, Scenario, is_transport_header, run_scenario, validate_origin};
use anyhow::{Context, Result, bail};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{Response, StatusCode},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{sync::Arc, time::Duration};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaultSpec {
    /// Exact request path (without query). Every matching request is affected.
    pub path: String,
    pub kind: FaultKind,
    #[serde(default)]
    pub delay_ms: u64,
    #[serde(default = "default_status")]
    pub status: u16,
    #[serde(default)]
    pub pointer: String,
    #[serde(default)]
    pub value: Value,
}
fn default_status() -> u16 {
    503
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultKind {
    Delay,
    Status,
    Json,
    InvalidJson,
    Disconnect,
}
impl FaultSpec {
    pub fn validate(&self) -> Result<()> {
        if !self.path.starts_with('/')
            || self.path.starts_with("//")
            || self.path.contains(['?', '#'])
        {
            bail!("fault path must be an exact relative path without a query");
        }
        if self.delay_ms > 30_000 {
            bail!("fault delay must be at most 30000 ms");
        }
        if !(100..=599).contains(&self.status) {
            bail!("invalid fault status");
        }
        if matches!(self.kind, FaultKind::Json) && !self.pointer.starts_with('/') {
            bail!("JSON fault requires a JSON Pointer, e.g. /total");
        }
        Ok(())
    }
}
#[derive(Clone)]
struct GatewayState {
    origin: reqwest::Url,
    client: reqwest::Client,
    fault: FaultSpec,
    cancel: Arc<tokio::sync::Notify>,
}
struct Gateway {
    task: tokio::task::JoinHandle<()>,
    cancel: Arc<tokio::sync::Notify>,
}
impl Drop for Gateway {
    fn drop(&mut self) {
        self.cancel.notify_waiters();
        self.task.abort();
    }
}

pub async fn run_with_fault(
    scenario: &Scenario,
    target: Option<&str>,
    variables: std::collections::BTreeMap<String, Value>,
    fault: Option<FaultSpec>,
) -> Result<RunReport> {
    let Some(fault) = fault else {
        return run_scenario(scenario, target, variables).await;
    };
    scenario.validate()?;
    fault.validate()?;
    let origin = validate_origin(target.unwrap_or(&scenario.base_url))?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_millis(scenario.timeout_ms))
        .build()?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let cancel = Arc::new(tokio::sync::Notify::new());
    let state = GatewayState {
        origin: origin.clone(),
        client,
        fault: fault.clone(),
        cancel: cancel.clone(),
    };
    let app = Router::new().fallback(gateway).with_state(state);
    let _gateway = Gateway {
        cancel,
        task: tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        }),
    };
    let mut report = run_scenario(scenario, Some(&format!("http://{address}")), variables).await?;
    report.target = origin.origin().ascii_serialization();
    report.fault = Some(fault);
    Ok(report)
}
async fn gateway(State(state): State<GatewayState>, request: Request) -> Response<Body> {
    tokio::select! {
        _=state.cancel.notified()=>Response::builder().status(499).body(Body::empty()).unwrap(),
        result=forward(&state,request)=>match result{
            Ok(response)=>response,
            Err(_)=>Response::builder().status(StatusCode::BAD_GATEWAY).header("content-type","application/json").body(Body::from(r#"{"error":"fault gateway upstream request failed"}"#)).unwrap()
        }
    }
}
async fn forward(state: &GatewayState, request: Request) -> Result<Response<Body>> {
    let affected = request.uri().path() == state.fault.path;
    let fault = &state.fault;
    if affected {
        match fault.kind {
            FaultKind::Delay => tokio::time::sleep(Duration::from_millis(fault.delay_ms)).await,
            FaultKind::Status => {
                return Ok(Response::builder()
                    .status(fault.status)
                    .header("content-type", "application/json")
                    .header("x-omni-fault", "status")
                    .body(Body::from(r#"{"error":"injected HTTP failure"}"#))?);
            }
            FaultKind::InvalidJson => {
                return Ok(Response::builder()
                    .header("content-type", "application/json")
                    .header("x-omni-fault", "invalid_json")
                    .body(Body::from("{broken"))?);
            }
            FaultKind::Disconnect => {
                let stream = futures_util::stream::once(async {
                    Err::<Vec<u8>, _>(std::io::Error::new(
                        std::io::ErrorKind::ConnectionReset,
                        "injected body interruption",
                    ))
                });
                return Ok(Response::builder()
                    .header("content-length", "128")
                    .body(Body::from_stream(stream))?);
            }
            FaultKind::Json => {}
        }
    }
    let (parts, body) = request.into_parts();
    let path = parts
        .uri
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/");
    let url = state.origin.join(path)?;
    if url.origin() != state.origin.origin() {
        bail!("request left gateway origin");
    }
    let body = to_bytes(body, 2 * 1024 * 1024).await?;
    let mut outgoing = state.client.request(parts.method, url).body(body);
    for (key, value) in &parts.headers {
        if !is_transport_header(key.as_str()) {
            outgoing = outgoing.header(key, value);
        }
    }
    let mut upstream = outgoing.send().await?;
    let mut response = Response::builder().status(upstream.status());
    for (key, value) in upstream.headers() {
        if !is_transport_header(key.as_str()) {
            response = response.header(key, value);
        }
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = upstream.chunk().await? {
        if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
            bail!("upstream response too large");
        }
        bytes.extend_from_slice(&chunk);
    }
    if affected && matches!(fault.kind, FaultKind::Json) {
        let mut json: Value =
            serde_json::from_slice(&bytes).context("JSON fault needs a JSON response")?;
        *json
            .pointer_mut(&fault.pointer)
            .context("JSON fault pointer is missing")? = fault.value.clone();
        bytes = serde_json::to_vec(&json)?;
        response = response.header("x-omni-fault", "json");
    }
    Ok(response.body(Body::from(bytes))?)
}
