//! Stolen from:
//! https://github.com/Gelbpunkt/leaky-bucket-lite/blob/main/tests/test_ao_issue.rs

use leaky_bucket::RateLimiter;
use tokio::time;

#[tokio::test]
async fn test_idle_1() {
    let limiter = RateLimiter::builder()
        .refill(1)
        .interval(time::Duration::from_secs(2))
        .max(5)
        .initial(5)
        .build();

    let start = time::Instant::now();
    time::sleep(time::Duration::from_secs(5)).await;

    for _ in 0..10 {
        limiter.acquire_one().await;
    }

    let elapsed = time::Instant::now().duration_since(start);
    println!("Elapsed: {:?}", elapsed);
    assert!(elapsed.as_millis() >= 20000 && elapsed.as_millis() <= 20050);
}

#[tokio::test]
async fn test_idle_2() {
    let limiter = RateLimiter::builder()
        .refill(1)
        .interval(time::Duration::from_secs(2))
        .max(5)
        .initial(5)
        .build();

    time::sleep(time::Duration::from_secs(5)).await;

    limiter.acquire_one().await;
    let start = time::Instant::now();

    for _ in 0..4 {
        limiter.acquire_one().await;
    }

    limiter.acquire_one().await;

    let elapsed = time::Instant::now().duration_since(start);
    println!("Elapsed: {:?}", elapsed);
    assert!(elapsed.as_millis() >= 2000 && elapsed.as_millis() <= 2050);
}

#[tokio::test]
async fn test_idle_3() {
    let limiter = RateLimiter::builder()
        .refill(1)
        .interval(time::Duration::from_secs(2))
        .max(5)
        .initial(5)
        .build();

    limiter.acquire_one().await;
    time::sleep(time::Duration::from_secs(5)).await;

    limiter.acquire_one().await;
    let start = time::Instant::now();

    for _ in 0..4 {
        limiter.acquire_one().await;
    }

    limiter.acquire_one().await;

    let elapsed = time::Instant::now().duration_since(start);
    println!("Elapsed: {:?}", elapsed);
    assert!(elapsed.as_millis() >= 2000 && elapsed.as_millis() <= 2050);
}
