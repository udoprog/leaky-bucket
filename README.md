# tokio-based leaky-bucket rate limiter

[![Build Status](https://travis-ci.org/udoprog/leaky-bucket.svg?branch=master)](https://travis-ci.org/udoprog/leaky-bucket)

This project is a rate limiter based on the [leaky bucket] algorithm.

[leaky bucket]: https://en.wikipedia.org/wiki/Leaky_bucket

## Usage

This library requires the user to add the following dependencies to use:

```toml
leaky-bucket = { version = "0.3.0", features = ["tokio02"] }
```

Note: this project has to be built with the feature corresponding to the tokio version to use: `tokio02` for `tokio-0.2` and `tokio01` for `tokio-0.1`.

## Examples

```rust
#![feature(async_await)]

use futures::prelude::*;
use leaky_bucket::LeakyBuckets;
use std::{error::Error, time::Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let buckets = LeakyBuckets::new();

    let rate_limiter = buckets
        .rate_limiter()
        .max(100)
        .refill_interval(Duration::from_secs(10))
        .refill_amount(100)
        .build()?;

    let coordinator = buckets.coordinate().boxed();

    // spawn the coordinate thread to refill the rate limiter.
    tokio::spawn(async move { coordinator.await.expect("coordinate thread errored") });

    println!("Waiting for permit...");
    // should take about ten seconds to get a permit.
    rate_limiter.acquire(100).await?;
    println!("I made it!");

    Ok(())
}
```