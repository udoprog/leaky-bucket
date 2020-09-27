use leaky_bucket::LeakyBuckets;
use std::time::{Duration, Instant};

#[tokio::test]
async fn test_issue5_a() {
    let mut buckets = LeakyBuckets::new();
    let coordinator = buckets.coordinate().unwrap();
    tokio::spawn(async move { coordinator.await.unwrap() });

    let rate_limiter = buckets
        .rate_limiter()
        .refill_amount(1)
        .refill_interval(Duration::from_millis(100))
        .build()
        .expect("LeakyBucket builder failed");

    let begin = Instant::now();

    for _ in 0..10 {
        rate_limiter.acquire_one().await.expect("No reason to fail");
    }

    let elapsed = Instant::now().duration_since(begin);
    println!("Elapsed: {:?}", elapsed);
    assert!((elapsed.as_secs_f64() - 1.).abs() < 0.1);
}

#[tokio::test]
async fn test_issue5_b() {
    let mut buckets = LeakyBuckets::new();
    let coordinator = buckets.coordinate().unwrap();
    tokio::spawn(async move { coordinator.await.unwrap() });

    let rate_limiter = buckets
        .rate_limiter()
        .refill_amount(1)
        .refill_interval(Duration::from_secs(2))
        .build()
        .expect("LeakyBucket builder failed");

    let begin = Instant::now();

    for _ in 0..2 {
        rate_limiter.acquire_one().await.expect("No reason to fail");
    }

    let elapsed = Instant::now().duration_since(begin);
    println!("Elapsed: {:?}", elapsed);
    // once per 2 seconds => 4 seconds for 2 permits
    assert!((elapsed.as_secs_f64() - 4.).abs() < 0.1);
}
