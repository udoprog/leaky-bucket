//! Stolen from:
//! https://github.com/Gelbpunkt/leaky-bucket-lite/blob/main/tests/test_overflow.rs

use leaky_bucket::RateLimiter;
use tokio::time::{Duration, Instant};

#[tokio::test(start_paused = true)]
async fn test_overflow() {
    let limiter = RateLimiter::builder()
        .max(5)
        .initial(5)
        .refill(1)
        .interval(Duration::from_millis(100))
        .build();

    let begin = Instant::now();

    for _ in 0..10 {
        limiter.acquire_one().await;
    }

    let elapsed = Instant::now().duration_since(begin);
    println!("Elapsed: {elapsed:?}");
    assert!(elapsed.as_millis() >= 500 && elapsed.as_millis() <= 550);
}

#[tokio::test(start_paused = true)]
async fn test_overflow_2() {
    let limiter = RateLimiter::builder()
        .max(5)
        .initial(5)
        .refill(1)
        .interval(Duration::from_millis(100))
        .build();

    let begin = Instant::now();

    limiter.acquire(10).await;

    let elapsed = Instant::now().duration_since(begin);
    println!("Elapsed: {elapsed:?}");
    assert!(elapsed.as_millis() >= 500 && elapsed.as_millis() <= 550);
}
