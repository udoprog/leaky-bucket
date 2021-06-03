use leaky_bucket::RateLimiter;
use std::time;

#[tokio::main]
async fn main() {
    helpers::init_logging();

    let limiter = RateLimiter::builder().max(10).initial(0).refill(5).build();

    let start = time::Instant::now();

    println!("Waiting for permit...");

    // Should take about 5 seconds to acquire in total.
    let a = limiter.acquire(7);
    let b = limiter.acquire(3);
    let c = limiter.acquire(10);

    let ((), (), ()) = tokio::join!(a, b, c);

    println!(
        "I made it in {:?}!",
        time::Instant::now().duration_since(start)
    );
}
