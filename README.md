# A leaky-bucket rate limiter

[![Documentation](https://docs.rs/leaky-bucket/badge.svg)](https://docs.rs/leaky-bucket)
[![Crates](https://img.shields.io/crates/v/leaky-bucket.svg)](https://crates.io/crates/leaky-bucket)
[![Actions Status](https://github.com/udoprog/leaky-bucket/workflows/Rust/badge.svg)](https://github.com/udoprog/leaky-bucket/actions)

A token-based rate limiter based on the [leaky bucket] algorithm.

[leaky bucket]: https://en.wikipedia.org/wiki/Leaky_bucket

## Usage

This library requires the user to add the following dependencies to use:

```toml
leaky-bucket = "0.7.1"
```

## Example

```rust
use leaky_bucket::LeakyBucket;
use std::{error::Error, time::Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let rate_limiter = LeakyBucket::builder()
        .max(100)
        .refill_interval(Duration::from_secs(10))
        .refill_amount(100)
        .build()?;

    println!("Waiting for permit...");
    // should take about ten seconds to get a permit.
    rate_limiter.acquire_one().await?;
    println!("I made it!");

    Ok(())
}
```

## Example with custom coordinator

Leaky buckets require coordination. By default, this will happen through a static coordinator spawned through `tokio::spawn` at first use.
If you want to spawn the coordinator yourself, you can do the following with `LeakyBuckets`:

```rust
use leaky_bucket::LeakyBuckets;
use std::{error::Error, time::Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut buckets = LeakyBuckets::new();
    let coordinator = buckets.coordinate()?;
    // spawn the coordinate thread to refill the rate limiter.
    tokio::spawn(async move { coordinator.await.expect("coordinate thread errored") });

    let rate_limiter = buckets
        .rate_limiter()
        .max(100)
        .refill_interval(Duration::from_secs(10))
        .refill_amount(100)
        .build()?;

    println!("Waiting for permit...");
    // should take about ten seconds to get a permit.
    rate_limiter.acquire_one().await?;
    println!("I made it!");

    Ok(())
}
```
