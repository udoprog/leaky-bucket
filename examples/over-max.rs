use std::time;

use leaky_bucket::RateLimiter;

#[tokio::main]
async fn main() {
    helpers::init_logging();

    // Note: that despite max being 0, we can still acquire more than max tokens
    // because each acquisition is tracked separately.
    let limiter = RateLimiter::builder().max(0).initial(0).refill(5).build();

    let start = time::Instant::now();

    println!("Waiting for permit...");

    // Should take ~400 ms to acquire in total.
    let a = limiter.acquire(7);
    let b = limiter.acquire(3);
    let c = limiter.acquire(10);

    let ((), (), ()) = tokio::join!(a, b, c);

    println!(
        "I made it in {:?}!",
        time::Instant::now().duration_since(start)
    );
}
