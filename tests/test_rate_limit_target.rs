use leaky_bucket::LeakyBuckets;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Test that a bunch of threads spinning on a rate limiter refilling a
/// reasonable amount of tokens at a slowish rate reaches the given target.
#[tokio::test]
async fn test_rate_limit_target() {
    let mut buckets = LeakyBuckets::new();
    let coordinator = buckets.coordinate().unwrap();
    tokio::spawn(async move { coordinator.await.unwrap() });

    let rate_limiter = buckets
        .rate_limiter()
        .refill_amount(50)
        .refill_interval(Duration::from_millis(200))
        .build()
        .expect("LeakyBucket builder failed");

    let rate_limiter = Arc::new(rate_limiter);
    let c = Arc::new(AtomicUsize::new(0));

    let start = Instant::now();

    let mut tasks = Vec::new();

    for _ in 0..100 {
        let rate_limiter = rate_limiter.clone();
        let c = c.clone();

        tasks.push(tokio::spawn(async move {
            while c.fetch_add(1, Ordering::SeqCst) < 500 {
                rate_limiter.acquire_one().await.unwrap();
            }
        }));
    }

    for t in tasks {
        t.await.unwrap();
    }

    let duration = Instant::now().duration_since(start);

    let diff = duration.as_millis() as f32 - 2000f32;
    assert!(
        diff.abs() < 10f32,
        "diff must be less than 10ms, but was {}ms",
        diff
    );
}
