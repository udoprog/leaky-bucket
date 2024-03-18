//! Stolen from:
//! https://github.com/Gelbpunkt/leaky-bucket-lite/blob/main/tests/test_ao_issue.rs

use leaky_bucket::RateLimiter;
use tokio::time;

#[tokio::test(start_paused = true)]
async fn test_idle_1() {
    // We expect 20 seconds to pass, because sleeping prior to attempting to
    // acquire permits does not "accumulate" more permits and the first five
    // permits are initialized, while we need to wait two seconds each for the
    // remaining five to accumulate plus the time we're sleeping of 4 seconds.
    const EXPECTED: f32 = 14000.0;

    let limiter = RateLimiter::builder()
        .refill(1)
        .interval(time::Duration::from_secs(2))
        .max(5)
        .initial(5)
        .build();

    let start = time::Instant::now();
    time::sleep(time::Duration::from_millis(4000)).await;

    for _ in 0..10 {
        limiter.acquire_one().await;
    }

    let elapsed = time::Instant::now().duration_since(start);
    println!("elapsed: {:?}", elapsed);
    assert!((elapsed.as_millis() as f32 - EXPECTED).abs() < 10.0);
}

#[tokio::test(start_paused = true)]
async fn test_idle_2() {
    // We expect 2000 milliseconds to pass, because sleeping prior to
    // attempting to acquire permits does not "accumulate" more permits.
    const EXPECTED: f32 = 2000.0;

    let limiter = RateLimiter::builder()
        .refill(1)
        .interval(time::Duration::from_secs(2))
        .max(5)
        .initial(5)
        .build();

    time::sleep(time::Duration::from_millis(4000)).await;

    limiter.acquire_one().await;
    let start = time::Instant::now();

    for _ in 0..4 {
        limiter.acquire_one().await;
    }

    limiter.acquire_one().await;

    let elapsed = time::Instant::now().duration_since(start);
    println!("elapsed: {:?}", elapsed);
    assert!((elapsed.as_millis() as f32 - EXPECTED).abs() < 10.0);
}

#[tokio::test(start_paused = true)]
async fn test_idle_3() {
    // We expect 100 milliseconds to pass, because the first five permits
    // acquired falls within one time window, which after 100 milliseconds rolls
    // over into the next.
    const EXPECTED: f32 = 100.0;

    let limiter = RateLimiter::builder()
        .refill(1)
        .interval(time::Duration::from_secs(2))
        .max(5)
        .initial(5)
        .build();

    limiter.acquire_one().await;
    time::sleep(time::Duration::from_millis(3900)).await;

    limiter.acquire_one().await;

    let start = time::Instant::now();

    for _ in 0..4 {
        limiter.acquire_one().await;
    }

    limiter.acquire_one().await;

    let elapsed = time::Instant::now().duration_since(start);
    println!("elapsed: {:?}", elapsed);
    assert!((elapsed.as_millis() as f32 - EXPECTED).abs() < 10.0);
}
