use anyhow::{Context, Result};
use clap::Parser;
use reqwest::Proxy;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Mutex;
use tokio::time::Instant;

#[derive(Debug, Parser)]
#[command(name = "omni-bench", about = "Simple HTTP benchmark for OmniProxy")]
struct Cli {
    #[arg(long)]
    url: String,

    #[arg(long, default_value_t = 1000)]
    requests: usize,

    #[arg(long, default_value_t = 64)]
    concurrency: usize,

    #[arg(long)]
    proxy: Option<String>,

    #[arg(long, default_value_t = 10_000)]
    timeout_ms: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.requests == 0 || cli.concurrency == 0 {
        anyhow::bail!("--requests and --concurrency must be > 0");
    }

    let mut builder =
        reqwest::Client::builder().timeout(std::time::Duration::from_millis(cli.timeout_ms));
    if let Some(px) = &cli.proxy {
        builder = builder.proxy(Proxy::all(px).with_context(|| format!("invalid proxy: {}", px))?);
    }
    let client = builder.build()?;

    let counter = Arc::new(AtomicUsize::new(0));
    let ok = Arc::new(AtomicUsize::new(0));
    let fail = Arc::new(AtomicUsize::new(0));
    let latencies = Arc::new(Mutex::new(Vec::<u128>::with_capacity(cli.requests)));
    let started = Instant::now();

    let mut tasks = Vec::new();
    for _ in 0..cli.concurrency {
        let client = client.clone();
        let url = cli.url.clone();
        let counter = counter.clone();
        let ok = ok.clone();
        let fail = fail.clone();
        let latencies = latencies.clone();
        let max = cli.requests;
        tasks.push(tokio::spawn(async move {
            loop {
                let idx = counter.fetch_add(1, Ordering::Relaxed);
                if idx >= max {
                    break;
                }
                let t0 = Instant::now();
                match client.get(&url).send().await {
                    Ok(resp) => {
                        if resp.status().is_success() {
                            ok.fetch_add(1, Ordering::Relaxed);
                        } else {
                            fail.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Err(_) => {
                        fail.fetch_add(1, Ordering::Relaxed);
                    }
                }
                let ms = t0.elapsed().as_millis();
                latencies.lock().await.push(ms);
            }
        }));
    }

    for t in tasks {
        let _ = t.await;
    }

    let total_elapsed_ms = started.elapsed().as_millis() as f64;
    let mut lats = latencies.lock().await.clone();
    lats.sort_unstable();

    let p50 = percentile(&lats, 50.0);
    let p95 = percentile(&lats, 95.0);
    let p99 = percentile(&lats, 99.0);
    let avg = if lats.is_empty() {
        0.0
    } else {
        (lats.iter().sum::<u128>() as f64) / (lats.len() as f64)
    };
    let rps = if total_elapsed_ms <= 0.0 {
        0.0
    } else {
        (lats.len() as f64) / (total_elapsed_ms / 1000.0)
    };

    println!("url={}", cli.url);
    println!("requests={}", cli.requests);
    println!("concurrency={}", cli.concurrency);
    println!("ok={}", ok.load(Ordering::Relaxed));
    println!("fail={}", fail.load(Ordering::Relaxed));
    println!("elapsed_ms={:.2}", total_elapsed_ms);
    println!("rps={:.2}", rps);
    println!("latency_avg_ms={:.2}", avg);
    println!("latency_p50_ms={}", p50);
    println!("latency_p95_ms={}", p95);
    println!("latency_p99_ms={}", p99);

    Ok(())
}

fn percentile(sorted: &[u128], p: f64) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((p / 100.0) * ((sorted.len() - 1) as f64)).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}
