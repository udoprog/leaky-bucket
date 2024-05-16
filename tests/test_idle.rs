//! Stolen from:
//! https://github.com/Gelbpunkt/leaky-bucket-lite/blob/main/tests/test_ao_issue.rs

use leaky_bucket::RateLimiter;
use tokio::time::{self, Duration, Instant};

#[tokio::test(start_paused = true)]
async fn test_idle_1() {
    let limiter = RateLimiter::builder()
        .refill(1)
        .interval(Duration::from_secs(2))
        .max(5)
        .initial(5)
        .build();

    time::sleep(Duration::from_millis(10000)).await;

    let start = Instant::now();
    // This one is "free", since we've slept before acquiring it.
    limiter.acquire_one().await;

    // These ones drain the available permits.
    for _ in 0..5 {
        limiter.acquire_one().await;
    }

    assert_eq!(Instant::now().duration_since(start), Duration::from_secs(0));

    // These ones need to sleep for 2 seconds for each permit.
    for _ in 0..5 {
        limiter.acquire_one().await;
    }

    assert_eq!(
        Instant::now().duration_since(start),
        Duration::from_secs(10)
    );
}

#[tokio::test(start_paused = true)]
async fn test_idle_2() {
    let limiter = RateLimiter::builder()
        .refill(1)
        .interval(Duration::from_secs(2))
        .max(5)
        .initial(5)
        .build();

    time::sleep(Duration::from_millis(10000)).await;

    // This one is "free", since it is within the time window we've slept.
    limiter.acquire_one().await;
    let start = Instant::now();

    for _ in 0..5 {
        limiter.acquire_one().await;
    }

    assert_eq!(Instant::now().duration_since(start), Duration::from_secs(0));
    // This one will have to wait for 2 seconds.
    limiter.acquire_one().await;
    assert_eq!(Instant::now().duration_since(start), Duration::from_secs(2));
}

#[tokio::test(start_paused = true)]
async fn test_idle_3() {
    // We expect 100 milliseconds to pass, because the first five permits
    // acquired falls within one time window, which after 100 milliseconds rolls
    // over into the next.
    const EXPECTED: Duration = Duration::from_millis(100);

    let limiter = RateLimiter::builder()
        .refill(1)
        .interval(Duration::from_secs(2))
        .max(5)
        .initial(5)
        .build();

    limiter.acquire_one().await;
    time::sleep(Duration::from_millis(3900)).await;

    limiter.acquire_one().await;

    let start = Instant::now();

    for _ in 0..4 {
        limiter.acquire_one().await;
    }

    limiter.acquire_one().await;

    let elapsed = Instant::now().duration_since(start);
    assert_eq!(elapsed, EXPECTED);
}
