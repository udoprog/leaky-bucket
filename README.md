# leaky-bucket

[![Documentation](https://docs.rs/leaky-bucket/badge.svg)](https://docs.rs/leaky-bucket)
[![Crates](https://img.shields.io/crates/v/leaky-bucket.svg)](https://crates.io/crates/leaky-bucket)
[![Actions Status](https://github.com/udoprog/leaky-bucket/workflows/Rust/badge.svg)](https://github.com/udoprog/leaky-bucket/actions)

A token-based rate limiter based on the [leaky bucket] algorithm.

If the bucket overflows and goes over its max configured capacity, the task
that tried to acquire the tokens will be suspended until the required number
of tokens has been drained from the bucket.

Since this crate uses timing facilities from tokio it has to be used within
a Tokio runtime with the [`time` feature] enabled.

### Usage

Add the following to your `Cargo.toml`:

```toml
leaky-bucket = "0.11.0"
```

### Examples

The core type is the [`RateLimiter`] type, which allows for limiting the
throughput of a section using its [`acquire`] and [`acquire_one`] methods.

```rust
use leaky_bucket::RateLimiter;
use std::time;

#[tokio::main]
async fn main() {
    let limiter = RateLimiter::builder()
        .max(10)
        .initial(0)
        .refill(5)
        .build();

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
```

### Implementation details

Each rate limiter has two acquisition modes. A fast path and a slow path.
The fast path is used if the desired number of tokens are readily available,
and involves incrementing an atomic counter indicating that the acquired
number of tokens have been added to the bucket.

If this counter goes over its configured maximum capacity, it overflows into
a slow path. Here one of the acquiring tasks will switch over to work as a
*core*. This is known as *core switching*.

```rust
use leaky_bucket::RateLimiter;
use std::time;

let limiter = RateLimiter::builder()
    .initial(10)
    .interval(time::Duration::from_millis(100))
    .build();

// This is instantaneous since the rate limiter starts with 10 tokens to
// spare.
limiter.acquire(10).await;

// This however needs to core switch and wait for a while until the desired
// number of tokens is available.
limiter.acquire(3).await;
```

The core is responsible for sleeping for the configured interval so that
more tokens can be added. After which it ensures that any tasks that are
waiting to acquire including itself are appropriately unsuspended.

On-demand core switching is what allows this rate limiter implementation to
work without a coordinating background thread. But we need to ensure that
any asynchronous tasks that uses [`RateLimiter`] must either run an
[`acquire`] call to completion, or be *cancelled* by being dropped.

If none of these hold, the core might leak and be locked indefinitely
preventing any future use of the rate limiter from making progress. This is
similar to if you would lock an asynchronous [`Mutex`] but never drop its
guard.

> You can run this example with:
>
> ```sh
> cargo run --example block-forever
> ```

```rust
use leaky_bucket::RateLimiter;
use std::future::Future;
use std::sync::Arc;
use std::task::Context;

struct Waker;

let limiter = Arc::new(RateLimiter::builder().build());

let waker = Arc::new(Waker).into();
let mut cx = Context::from_waker(&waker);

let mut a0 = Box::pin(limiter.acquire(1));
// Poll once to ensure that the core task is assigned.
assert!(a0.as_mut().poll(&mut cx).is_pending());
assert!(a0.is_core());

// We leak the core task, preventing the rate limiter from making progress
// by assigning new core tasks.
std::mem::forget(a0);

// Awaiting acquire here would block forever.
// limiter.acquire(1).await;
```

### Fairness

By default [`RateLimiter`] uses a *fair* scheduler. This ensures that the
core task makes progress even if there are many tasks waiting to acquire
tokens. As a result it causes more frequent core switching, increasing the
total work needed. An unfair scheduler is expected to do a bit less work
under contention. But without fair scheduling some tasks might end up taking
longer to acquire than expected.

This behavior can be changed tweaked the [`Builder::fair`] option.

```rust
use leaky_bucket::RateLimiter;

let limiter = RateLimiter::builder()
    .fair(false)
    .build();
```

The `unfair-scheduling` example can showcase this phenomenon.

```sh
cargh run --example unfair-scheduling
```

```
# fair
Max: 1011ms, Total: 1012ms
Timings:
 0: 101ms
 1: 101ms
 2: 101ms
 3: 101ms
 4: 101ms
 ...
# unfair
Max: 1014ms, Total: 1014ms
Timings:
 0: 1014ms
 1: 101ms
 2: 101ms
 3: 101ms
 4: 101ms
 ...
```

As can be seen above the first task in the *unfair* scheduler takes longer
to run because it prioritises releasing other tasks waiting to acquire over
itself.

[`acquire_one`]: https://docs.rs/leaky-bucket/0/leaky_bucket/struct.RateLimiter.html#method.acquire_one
[`acquire`]: https://docs.rs/leaky-bucket/0/leaky_bucket/struct.RateLimiter.html#method.acquire
[`Builder::fair`]: https://docs.rs/leaky-bucket/0/leaky_bucket/struct.Builder.html#method.fair
[`Mutex`]: https://docs.rs/tokio/1/tokio/sync/struct.Mutex.html
[`RateLimiter`]: https://docs.rs/leaky-bucket/0/leaky_bucket/struct.RateLimiter.html
[`time` feature]: https://docs.rs/tokio/1/tokio/#feature-flags
[leaky bucket]: https://en.wikipedia.org/wiki/Leaky_bucket

License: MIT/Apache-2.0
