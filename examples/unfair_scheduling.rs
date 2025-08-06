use std::sync::Arc;

use anyhow::Result;
use leaky_bucket::RateLimiter;
use tokio::time::Instant;

/// Grind a rate limiter.
async fn grind(what: &str, limiter: &Arc<RateLimiter>) -> Result<()> {
    let mut tasks = Vec::new();

    for _ in 0..1000 {
        let limiter = limiter.clone();

        tasks.push(tokio::spawn(async move {
            let start = Instant::now();
            limiter.acquire(1).await;
            Instant::now().saturating_duration_since(start).as_millis() as i64
        }));
    }

    let mut results = Vec::new();

    let start = Instant::now();

    for task in tasks {
        results.push(task.await?);
    }

    let total = Instant::now().saturating_duration_since(start).as_millis() as i64;

    let max = results.iter().max().unwrap();

    println!("# {what}");
    println! {
        "Max: {max}ms, Total: {total}ms"
    };

    println!("Timings:");

    for (i, n) in results.iter().enumerate().take(5) {
        println!(" {i}: {n}ms");
    }

    println!(" ...");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let fair = Arc::new(RateLimiter::builder().refill(100).build());
    grind("fair", &fair).await?;

    let unfair = Arc::new(RateLimiter::builder().refill(100).fair(false).build());
    grind("unfair", &unfair).await?;

    Ok(())
}
