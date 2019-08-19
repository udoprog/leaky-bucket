#![feature(async_await)]

use futures::prelude::*;
use leaky_bucket::LeakyBucket;
use std::{error::Error, time::Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let rate_limiter = LeakyBucket::builder()
        .max(100)
        .refill_interval(Duration::from_secs(10))
        .refill_amount(100)
        .build();

    let coordinator = rate_limiter.coordinate().boxed();

    // spawn the coordinate thread to refill the rate limiter.
    tokio::spawn(async move { coordinator.await.expect("coordinate thread errored") });

    println!("Waiting for permit...");
    // should take about ten seconds to get a permit.
    rate_limiter.acquire(100).await;
    println!("I made it!");

    Ok(())
}
