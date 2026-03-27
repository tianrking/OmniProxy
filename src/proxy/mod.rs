use crate::{
    api::{ApiHub, serve_ws_api},
    cert::load_or_init_issuer,
    config::AppConfig,
    filter::{
        FilterChain, WebSocketFilterChain,
        standard::{
            AccessLogFilter, RequestIdFilter, RuleFilter, WasmFilter, WsAccessLogFilter,
            WsMutationFilter,
        },
    },
    plugins::WasmPluginHost,
    rules::RuleEngine,
    storage::{FlowLogOptions, run_flow_logger},
};
use anyhow::{Context, Result};
use hudsucker::{
    Body, HttpContext, HttpHandler, Proxy, RequestOrResponse, WebSocketContext, WebSocketHandler,
    certificate_authority::RcgenAuthority, hyper::Request, hyper::Response,
    rustls::crypto::aws_lc_rs, tokio_tungstenite::tungstenite::Message,
};
use std::sync::Arc;
use tracing::{error, info};

#[derive(Clone)]
struct OmniHandler {
    chain: FilterChain,
    ws_chain: WebSocketFilterChain,
}

impl HttpHandler for OmniHandler {
    async fn handle_request(&mut self, ctx: &HttpContext, req: Request<Body>) -> RequestOrResponse {
        match self.chain.handle_request(ctx, req).await {
            Ok(decision) => decision,
            Err(err) => {
                error!(error = %err, "request filter failed");
                Response::builder()
                    .status(500)
                    .body(Body::from(format!("OmniProxy request error: {err}")))
                    .expect("build response")
                    .into()
            }
        }
    }

    async fn handle_response(&mut self, ctx: &HttpContext, res: Response<Body>) -> Response<Body> {
        match self.chain.handle_response(ctx, res).await {
            Ok(res) => res,
            Err(err) => {
                error!(error = %err, "response filter failed");
                Response::builder()
                    .status(500)
                    .body(Body::from(format!("OmniProxy response error: {err}")))
                    .expect("build response")
            }
        }
    }
}

impl WebSocketHandler for OmniHandler {
    async fn handle_message(&mut self, ctx: &WebSocketContext, msg: Message) -> Option<Message> {
        match self.ws_chain.handle_message(ctx, msg).await {
            Ok(v) => v,
            Err(err) => {
                error!(error = %err, "websocket filter failed");
                None
            }
        }
    }
}

pub async fn run(config: AppConfig) -> Result<()> {
    if !config.plugin_dir.exists() {
        tokio::fs::create_dir_all(&config.plugin_dir)
            .await
            .with_context(|| format!("create plugin dir {}", config.plugin_dir.display()))?;
    }
    if let Some(parent) = config.rule_file_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("create rules dir {}", parent.display()))?;
    }

    let api_hub = ApiHub::new(4096);

    let flow_log = config.flow_log_path.clone();
    let flow_rx = api_hub.subscribe();
    let flow_opts = FlowLogOptions {
        rotate_bytes: config.flow_log_rotate_bytes,
        max_files: config.flow_log_max_files,
    };
    tokio::spawn(async move {
        if let Err(err) = run_flow_logger(&flow_log, flow_rx, flow_opts).await {
            error!(error = %err, "flow logger exited");
        }
    });

    let api_addr = config.api_listen_addr;
    let api_hub_for_server = api_hub.clone();
    let api_max_lag = config.api_max_lag;
    tokio::spawn(async move {
        if let Err(err) = serve_ws_api(api_addr, api_hub_for_server, api_max_lag).await {
            error!(error = %err, "ws api server exited");
        }
    });

    let issuer = load_or_init_issuer(&config.ca_cert_path, &config.ca_key_path).await?;
    let authority = RcgenAuthority::new(issuer, 10_000, aws_lc_rs::default_provider());
    info!(
        cert = %config.ca_cert_path.display(),
        key = %config.ca_key_path.display(),
        "CA ready"
    );

    let wasm_host = Arc::new(WasmPluginHost::load(
        &config.plugin_dir,
        config.wasm_timeout_ms,
    )?);
    let rules = Arc::new(RuleEngine::load(&config.rule_file_path)?);
    let rs = rules.stats();
    info!(
        rule_file = %config.rule_file_path.display(),
        rule_count = rules.count(),
        deny_rules = rs.deny_rules,
        req_header_rules = rs.req_header_rules,
        res_header_rules = rs.res_header_rules,
        "rule engine ready"
    );
    let chain = FilterChain::new(vec![
        Arc::new(RequestIdFilter),
        Arc::new(RuleFilter::new(rules)),
        Arc::new(AccessLogFilter::with_hub(
            Some(api_hub.clone()),
            config.capture_body_max_bytes,
            config.capture_body_sample_rate,
            config.capture_body_compressed,
        )),
        Arc::new(WasmFilter::new(wasm_host)),
    ]);
    let ws_chain = WebSocketFilterChain::new(vec![
        Arc::new(WsMutationFilter::new(
            config.ws_drop_ping,
            config.ws_text_rewrite.clone(),
        )),
        Arc::new(WsAccessLogFilter::with_hub(
            Some(api_hub.clone()),
            config.ws_preview_bytes,
        )),
    ]);

    let handler = OmniHandler { chain, ws_chain };

    let proxy = Proxy::builder()
        .with_addr(config.listen_addr)
        .with_ca(authority)
        .with_rustls_connector(aws_lc_rs::default_provider())
        .with_http_handler(handler.clone())
        .with_websocket_handler(handler)
        .with_graceful_shutdown(shutdown_signal())
        .build()
        .context("build proxy")?;

    info!(
        listen = %config.listen_addr,
        api = %config.api_listen_addr,
        "OmniProxy running"
    );
    proxy.start().await.map_err(|e| anyhow::anyhow!(e))
}

async fn shutdown_signal() {
    if let Err(err) = tokio::signal::ctrl_c().await {
        error!(error = %err, "failed to install ctrl-c handler");
    }
}
