use leaky_bucket::RateLimiter;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::time;

/// Test that a bunch of threads spinning on a rate limiter refilling a
/// reasonable amount of tokens at a slowish rate reaches the given target.
#[tokio::test(start_paused = true)]
async fn test_rate_limit_target() {
    const TARGET: usize = 1000;
    const INTERVALS: usize = 10;
    const DURATION: u64 = 2000;
    const TARGET_DIFFERENCE: u32 = 20;

    let limiter = RateLimiter::builder()
        .refill(TARGET / INTERVALS)
        .interval(time::Duration::from_millis(DURATION / INTERVALS as u64))
        .build();

    let limiter = Arc::new(limiter);
    let c = Arc::new(AtomicUsize::new(0));

    let start = time::Instant::now();

    let mut tasks = Vec::new();

    for _ in 0..100 {
        let limiter = limiter.clone();
        let c = c.clone();

        tasks.push(tokio::spawn(async move {
            while c.fetch_add(1, Ordering::SeqCst) < TARGET {
                limiter.acquire_one().await;
            }
        }));
    }

    for t in tasks {
        t.await.unwrap();
    }

    let duration = time::Instant::now().duration_since(start);

    let diff = duration.as_millis() as f32 - DURATION as f32;

    assert! {
        diff.abs() < TARGET_DIFFERENCE as f32,
        "diff must be less than {}ms, but was {}ms",
        TARGET_DIFFERENCE,
        diff,
    };
}
