use anyhow::Result;
use clap::Parser;
use reqwest::Proxy;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Mutex;
use tokio::time::Instant;

#[derive(Debug, Clone, Parser)]
#[command(name = "omni-converge", about = "HTTP concurrency convergence runner")]
struct Cli {
    #[arg(long)]
    url: String,

    #[arg(long, default_value_t = 2000)]
    requests: usize,

    #[arg(long, default_value_t = 128)]
    concurrency: usize,

    #[arg(long)]
    proxy: Option<String>,

    #[arg(long, default_value_t = 8000)]
    timeout_ms: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    run_mode("http1", &cli, true).await?;
    run_mode("http2pref", &cli, false).await?;
    Ok(())
}

async fn run_mode(name: &str, cli: &Cli, http1_only: bool) -> Result<()> {
    let mut builder =
        reqwest::Client::builder().timeout(std::time::Duration::from_millis(cli.timeout_ms));
    if http1_only {
        builder = builder.http1_only();
    }
    if let Some(px) = &cli.proxy {
        builder = builder.proxy(Proxy::all(px)?);
    }
    let client = builder.build()?;

    let ok = Arc::new(AtomicUsize::new(0));
    let fail = Arc::new(AtomicUsize::new(0));
    let next = Arc::new(AtomicUsize::new(0));
    let lats = Arc::new(Mutex::new(Vec::<u128>::with_capacity(cli.requests)));
    let t0 = Instant::now();

    let mut tasks = Vec::new();
    for _ in 0..cli.concurrency {
        let ok = ok.clone();
        let fail = fail.clone();
        let next = next.clone();
        let lats = lats.clone();
        let client = client.clone();
        let url = cli.url.clone();
        let max = cli.requests;
        tasks.push(tokio::spawn(async move {
            loop {
                let idx = next.fetch_add(1, Ordering::Relaxed);
                if idx >= max {
                    break;
                }
                let t = Instant::now();
                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        ok.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(_) | Err(_) => {
                        fail.fetch_add(1, Ordering::Relaxed);
                    }
                }
                lats.lock().await.push(t.elapsed().as_millis());
            }
        }));
    }
    for t in tasks {
        let _ = t.await;
    }

    let elapsed_ms = t0.elapsed().as_millis() as f64;
    let mut lat = lats.lock().await.clone();
    lat.sort_unstable();
    let p95 = percentile(&lat, 95.0);
    let p99 = percentile(&lat, 99.0);
    let total = ok.load(Ordering::Relaxed) + fail.load(Ordering::Relaxed);
    let err_rate = if total == 0 {
        0.0
    } else {
        (fail.load(Ordering::Relaxed) as f64) / (total as f64)
    };
    let rps = if elapsed_ms <= 0.0 {
        0.0
    } else {
        (total as f64) / (elapsed_ms / 1000.0)
    };

    println!("mode={}", name);
    println!("requests={}", cli.requests);
    println!("concurrency={}", cli.concurrency);
    println!("ok={}", ok.load(Ordering::Relaxed));
    println!("fail={}", fail.load(Ordering::Relaxed));
    println!("error_rate={:.6}", err_rate);
    println!("rps={:.2}", rps);
    println!("p95_ms={}", p95);
    println!("p99_ms={}", p99);
    println!("---");
    Ok(())
}

fn percentile(sorted: &[u128], p: f64) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((p / 100.0) * ((sorted.len() - 1) as f64)).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}
