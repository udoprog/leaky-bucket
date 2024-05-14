//! Stolen from:
//! https://github.com/Gelbpunkt/leaky-bucket-lite/blob/main/tests/test_ao_issue.rs

use leaky_bucket::RateLimiter;
use tokio::time::{self, Duration};

#[tokio::test(start_paused = true)]
async fn test_idle_1() {
    // We expect 20 seconds to pass, because sleeping prior to attempting to
    // acquire permits does not "accumulate" more permits and the first five
    // permits are initialized, while we need to wait two seconds each for the
    // remaining five to accumulate plus the time we're sleeping of 4 seconds.
    const EXPECTED: Duration = Duration::from_millis(14000);

    let limiter = RateLimiter::builder()
        .refill(1)
        .interval(Duration::from_secs(2))
        .max(5)
        .initial(5)
        .build();

    let start = time::Instant::now();
    time::sleep(Duration::from_millis(4000)).await;

    for _ in 0..10 {
        limiter.acquire_one().await;
    }

    let elapsed = time::Instant::now().duration_since(start);
    assert_eq!(elapsed, EXPECTED);
}

#[tokio::test(start_paused = true)]
async fn test_idle_2() {
    // We expect 2000 milliseconds to pass, because sleeping prior to
    // attempting to acquire permits does not "accumulate" more permits.
    const EXPECTED: Duration = Duration::from_millis(2000);

    let limiter = RateLimiter::builder()
        .refill(1)
        .interval(Duration::from_secs(2))
        .max(5)
        .initial(5)
        .build();

    time::sleep(Duration::from_millis(4000)).await;

    limiter.acquire_one().await;
    let start = time::Instant::now();

    for _ in 0..4 {
        limiter.acquire_one().await;
    }

    limiter.acquire_one().await;

    let elapsed = time::Instant::now().duration_since(start);
    println!("elapsed: {:?}", elapsed);
    assert_eq!(elapsed, EXPECTED);
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

    let start = time::Instant::now();

    for _ in 0..4 {
        limiter.acquire_one().await;
    }

    limiter.acquire_one().await;

    let elapsed = time::Instant::now().duration_since(start);
    assert_eq!(elapsed, EXPECTED);
}
