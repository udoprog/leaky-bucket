use leaky_bucket::RateLimiter;
use tokio::time::{Duration, Instant};

#[tokio::test(start_paused = true)]
async fn test_issue5_a() {
    let limiter = RateLimiter::builder()
        .refill(1)
        .interval(Duration::from_millis(100))
        .build();

    let begin = Instant::now();

    for _ in 0..10 {
        limiter.acquire_one().await;
    }

    let elapsed = Instant::now().duration_since(begin);
    println!("Elapsed: {:?}", elapsed);
    assert!((elapsed.as_secs_f64() - 1.).abs() < 0.1);
}

#[tokio::test(start_paused = true)]
async fn test_issue5_b() {
    let limiter = RateLimiter::builder()
        .refill(1)
        .interval(Duration::from_secs(2))
        .build();

    let begin = Instant::now();

    for _ in 0..2 {
        limiter.acquire_one().await;
    }

    let elapsed = Instant::now().duration_since(begin);
    println!("Elapsed: {:?}", elapsed);
    // once per 2 seconds => 4 seconds for 2 permits
    assert!((elapsed.as_secs_f64() - 4.).abs() < 0.1);
}
