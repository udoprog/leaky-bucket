use leaky_bucket::LeakyBucket;
use std::{error::Error, time::Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let rate_limiter = LeakyBucket::builder()
        .max(6)
        .refill_interval(Duration::from_secs(1))
        .refill_amount(1)
        .tokens(8)
        .build()?;

    println!("Tokens: {}", rate_limiter.tokens());

    println!("Waiting for permit...");
    // should take about ten seconds to get a permit.
    rate_limiter.acquire(10).await?;
    println!("I made it with {} tokens left!", rate_limiter.tokens());
    Ok(())
}
