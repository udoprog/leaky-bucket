# tokio-based leaky-bucket rate limiter

[![Build Status](https://travis-ci.org/udoprog/leaky-bucket.svg?branch=master)](https://travis-ci.org/udoprog/leaky-bucket)

This project is a rate limiter based on the [leaky bucket] algorithm.

[leaky bucket]: https://en.wikipedia.org/wiki/Leaky_bucket

## Usage

This library requires the user to add the following dependencies to use:

```toml
leaky-bucket = "0.5.0"
```

## Examples

```rust
use leaky_bucket::LeakyBuckets;
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
    rate_limiter.acquire(100).await?;
    println!("I made it!");

    Ok(())
}
```