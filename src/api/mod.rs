use anyhow::Result;
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
        client: String,
        method: String,
        uri: String,
    },
    HttpResponse {
        client: String,
        status: u16,
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

pub async fn serve_ws_api(listen: std::net::SocketAddr, hub: ApiHub) -> Result<()> {
    let listener = TcpListener::bind(listen).await?;
    info!(listen = %listen, "ws api listening");

    loop {
        let (stream, peer) = listener.accept().await?;
        let hub = hub.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_connection(stream, peer.to_string(), hub).await {
                debug!(peer = %peer, error = %err, "ws client closed");
            }
        });
    }
}

async fn handle_connection(stream: TcpStream, peer: String, hub: ApiHub) -> Result<()> {
    let mut ws = accept_async(stream).await?;
    let mut rx = hub.subscribe();

    info!(peer, "ws api client connected");
    loop {
        match rx.recv().await {
            Ok(event) => {
                let body = serde_json::to_string(&event)?;
                ws.send(Message::Text(body.into())).await?;
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                error!(peer, skipped, "ws api client lagged");
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
    Ok(())
}
