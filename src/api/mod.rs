use anyhow::{Result, bail};
use futures_util::SinkExt;
use serde::{Deserialize, Serialize};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::broadcast,
};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tracing::{debug, error, info};

#[derive(Clone)]
pub struct ApiHub {
    tx: broadcast::Sender<ApiEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ApiEvent {
    HttpRequest {
        #[serde(default)]
        timestamp_ms: u64,
        #[serde(default)]
        request_id: Option<String>,
        client: String,
        method: String,
        uri: String,
        #[serde(default)]
        headers: Vec<(String, String)>,
        #[serde(default)]
        body_b64: Option<String>,
        #[serde(default)]
        body_truncated: bool,
        #[serde(default)]
        body_size: Option<usize>,
    },
    HttpResponse {
        #[serde(default)]
        timestamp_ms: u64,
        #[serde(default)]
        request_id: Option<String>,
        client: String,
        status: u16,
        #[serde(default)]
        headers: Vec<(String, String)>,
        #[serde(default)]
        body_b64: Option<String>,
        #[serde(default)]
        body_truncated: bool,
        #[serde(default)]
        body_size: Option<usize>,
    },
    WebSocketFrame {
        #[serde(default)]
        timestamp_ms: u64,
        #[serde(default)]
        client: Option<String>,
        kind: String,
        payload_len: usize,
        #[serde(default)]
        preview: Option<String>,
    },
}

impl ApiHub {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    pub fn publish(&self, event: ApiEvent) {
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ApiEvent> {
        self.tx.subscribe()
    }
}

pub fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub async fn serve_ws_api(listen: std::net::SocketAddr, hub: ApiHub, max_lag: u64) -> Result<()> {
    let listener = TcpListener::bind(listen).await?;
    info!(listen = %listen, max_lag, "ws api listening");

    loop {
        let (stream, peer) = listener.accept().await?;
        let hub = hub.clone();
        let max_lag = max_lag;
        tokio::spawn(async move {
            if let Err(err) = handle_connection(stream, peer.to_string(), hub, max_lag).await {
                debug!(peer = %peer, error = %err, "ws client closed");
            }
        });
    }
}

async fn handle_connection(
    stream: TcpStream,
    peer: String,
    hub: ApiHub,
    max_lag: u64,
) -> Result<()> {
    let mut ws = accept_async(stream).await?;
    let mut rx = hub.subscribe();
    let mut lagged_total: u64 = 0;

    info!(peer, max_lag, "ws api client connected");
    loop {
        match rx.recv().await {
            Ok(event) => {
                let body = serde_json::to_string(&event)?;
                ws.send(Message::Text(body.into())).await?;
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                lagged_total = lagged_total.saturating_add(skipped);
                error!(peer, skipped, lagged_total, max_lag, "ws api client lagged");
                if lagged_total > max_lag {
                    bail!(
                        "ws api lag exceeded threshold: lagged_total={} max_lag={}",
                        lagged_total,
                        max_lag
                    );
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
    Ok(())
}
