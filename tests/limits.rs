use leaky_bucket::RateLimiter;
use tokio::time::{Duration, Instant};

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn test_numerical_limits() {
    let limiter = RateLimiter::builder().refill(usize::MAX).initial(0).build();
    let start = Instant::now();

    limiter.acquire(usize::MAX).await;
    // Drain the remainder, this should not block.
    limiter.acquire(isize::MAX as usize - 1).await;

    // This takes 300ms because isize::MAX is one off from half of usize::MAX,
    // so we need to wait for three periods to satisfy usize::MAX.
    assert_eq!(
        Instant::now().duration_since(start),
        Duration::from_millis(300)
    );

    // This will block for 100ms to refill the bucket.
    limiter.acquire(1).await;
    assert_eq!(
        Instant::now().duration_since(start),
        Duration::from_millis(400)
    );
}
