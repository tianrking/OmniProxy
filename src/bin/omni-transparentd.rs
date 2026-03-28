use anyhow::{Context, Result};
use clap::Parser;
use hudsucker::{
    certificate_authority::{CertificateAuthority, RcgenAuthority},
    rustls::crypto::aws_lc_rs,
};
use omni_proxy::cert::load_or_init_issuer;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional};
use tokio::net::{TcpListener, TcpStream};
use tokio_native_tls::TlsConnector as NativeTlsConnector;
use tokio_rustls::{LazyConfigAcceptor, rustls::server::Acceptor};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "omni-transparentd",
    about = "Transparent HTTP/HTTPS MITM forwarder"
)]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:10080")]
    http_listen: String,

    #[arg(long, default_value = "127.0.0.1:10443")]
    https_listen: String,

    #[arg(long, default_value = ".omni-proxy/ca.crt")]
    ca_cert: std::path::PathBuf,

    #[arg(long, default_value = ".omni-proxy/ca.key")]
    ca_key: std::path::PathBuf,

    #[arg(long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&cli.log_level)),
        )
        .with_target(false)
        .compact()
        .init();

    let issuer = load_or_init_issuer(&cli.ca_cert, &cli.ca_key).await?;
    let ca = Arc::new(RcgenAuthority::new(
        issuer,
        10_000,
        aws_lc_rs::default_provider(),
    ));

    let http_addr = cli.http_listen.parse::<std::net::SocketAddr>()?;
    let https_addr = cli.https_listen.parse::<std::net::SocketAddr>()?;
    let http_listener = TcpListener::bind(http_addr).await?;
    let https_listener = TcpListener::bind(https_addr).await?;

    info!(http = %http_addr, https = %https_addr, "transparentd listening");

    let ca_https = Arc::clone(&ca);
    tokio::spawn(async move {
        loop {
            match https_listener.accept().await {
                Ok((stream, peer)) => {
                    let ca = Arc::clone(&ca_https);
                    tokio::spawn(async move {
                        if let Err(err) = handle_https(stream, ca).await {
                            warn!(peer = %peer, error = %err, "transparent https failed");
                        }
                    });
                }
                Err(err) => warn!(error = %err, "https accept failed"),
            }
        }
    });

    loop {
        let (stream, peer) = http_listener.accept().await?;
        tokio::spawn(async move {
            if let Err(err) = handle_http(stream).await {
                warn!(peer = %peer, error = %err, "transparent http failed");
            }
        });
    }
}

async fn handle_http(mut inbound: TcpStream) -> Result<()> {
    let mut buf = vec![0_u8; 16 * 1024];
    let n = inbound.read(&mut buf).await?;
    if n == 0 {
        return Ok(());
    }
    let head = String::from_utf8_lossy(&buf[..n]);
    let host = parse_host_header(&head).context("http host header missing")?;
    let target = format!("{}:80", host);
    let mut upstream = TcpStream::connect(&target)
        .await
        .with_context(|| format!("connect upstream {}", target))?;
    upstream.write_all(&buf[..n]).await?;
    let _ = copy_bidirectional(&mut inbound, &mut upstream).await?;
    Ok(())
}

async fn handle_https(inbound: TcpStream, ca: Arc<RcgenAuthority>) -> Result<()> {
    let acceptor = LazyConfigAcceptor::new(Acceptor::default(), inbound);
    let start: tokio_rustls::StartHandshake<TcpStream> = acceptor.await?;
    let client_hello = start.client_hello();
    let sni = client_hello
        .server_name()
        .map(|s: &str| s.to_string())
        .context("tls client hello has no server_name")?;
    let authority = format!("{}:443", sni).parse::<hudsucker::hyper::http::uri::Authority>()?;
    let server_cfg = ca.gen_server_config(&authority).await;
    let mut client_tls = start.into_stream(server_cfg).await?;

    let upstream_tcp = TcpStream::connect((sni.as_str(), 443)).await?;
    let connector = native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .build()?;
    let connector = NativeTlsConnector::from(connector);
    let mut upstream_tls = connector.connect(&sni, upstream_tcp).await?;

    let _ = copy_bidirectional(&mut client_tls, &mut upstream_tls).await?;
    Ok(())
}

fn parse_host_header(head: &str) -> Option<String> {
    for line in head.lines() {
        if let Some(v) = line.strip_prefix("Host:") {
            let host = v.trim().split(':').next()?.trim();
            if !host.is_empty() {
                return Some(host.to_string());
            }
        }
    }
    None
}
