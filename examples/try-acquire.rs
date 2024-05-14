use leaky_bucket::RateLimiter;

#[tokio::main]
async fn main() {
    let limiter = RateLimiter::builder().refill(1).initial(1).build();

    assert!(limiter.try_acquire(1));
    assert!(!limiter.try_acquire(1));

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    assert!(limiter.try_acquire(1));
    assert!(limiter.try_acquire(1));
    assert!(!limiter.try_acquire(1));
}
