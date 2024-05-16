# leaky-bucket

[<img alt="github" src="https://img.shields.io/badge/github-udoprog/leaky--bucket-8da0cb?style=for-the-badge&logo=github" height="20">](https://github.com/udoprog/leaky-bucket)
[<img alt="crates.io" src="https://img.shields.io/crates/v/leaky-bucket.svg?style=for-the-badge&color=fc8d62&logo=rust" height="20">](https://crates.io/crates/leaky-bucket)
[<img alt="docs.rs" src="https://img.shields.io/badge/docs.rs-leaky--bucket-66c2a5?style=for-the-badge&logoColor=white&logo=data:image/svg+xml;base64,PHN2ZyByb2xlPSJpbWciIHhtbG5zPSJodHRwOi8vd3d3LnczLm9yZy8yMDAwL3N2ZyIgdmlld0JveD0iMCAwIDUxMiA1MTIiPjxwYXRoIGZpbGw9IiNmNWY1ZjUiIGQ9Ik00ODguNiAyNTAuMkwzOTIgMjE0VjEwNS41YzAtMTUtOS4zLTI4LjQtMjMuNC0zMy43bC0xMDAtMzcuNWMtOC4xLTMuMS0xNy4xLTMuMS0yNS4zIDBsLTEwMCAzNy41Yy0xNC4xIDUuMy0yMy40IDE4LjctMjMuNCAzMy43VjIxNGwtOTYuNiAzNi4yQzkuMyAyNTUuNSAwIDI2OC45IDAgMjgzLjlWMzk0YzAgMTMuNiA3LjcgMjYuMSAxOS45IDMyLjJsMTAwIDUwYzEwLjEgNS4xIDIyLjEgNS4xIDMyLjIgMGwxMDMuOS01MiAxMDMuOSA1MmMxMC4xIDUuMSAyMi4xIDUuMSAzMi4yIDBsMTAwLTUwYzEyLjItNi4xIDE5LjktMTguNiAxOS45LTMyLjJWMjgzLjljMC0xNS05LjMtMjguNC0yMy40LTMzLjd6TTM1OCAyMTQuOGwtODUgMzEuOXYtNjguMmw4NS0zN3Y3My4zek0xNTQgMTA0LjFsMTAyLTM4LjIgMTAyIDM4LjJ2LjZsLTEwMiA0MS40LTEwMi00MS40di0uNnptODQgMjkxLjFsLTg1IDQyLjV2LTc5LjFsODUtMzguOHY3NS40em0wLTExMmwtMTAyIDQxLjQtMTAyLTQxLjR2LS42bDEwMi0zOC4yIDEwMiAzOC4ydi42em0yNDAgMTEybC04NSA0Mi41di03OS4xbDg1LTM4Ljh2NzUuNHptMC0xMTJsLTEwMiA0MS40LTEwMi00MS40di0uNmwxMDItMzguMiAxMDIgMzguMnYuNnoiPjwvcGF0aD48L3N2Zz4K" height="20">](https://docs.rs/leaky-bucket)
[<img alt="build status" src="https://img.shields.io/github/actions/workflow/status/udoprog/leaky-bucket/ci.yml?branch=main&style=for-the-badge" height="20">](https://github.com/udoprog/leaky-bucket/actions?query=branch%3Amain)

A token-based rate limiter based on the [leaky bucket] algorithm.

If the bucket overflows and goes over its max configured capacity, the task
that tried to acquire the tokens will be suspended until the required number
of tokens has been drained from the bucket.

Since this crate uses timing facilities from tokio it has to be used within
a Tokio runtime with the [`time` feature] enabled.

This library has some neat features, which includes:

**Not requiring a background task**. This is usually needed by token bucket
rate limiters to drive progress. Instead, one of the waiting tasks
temporarily assumes the role as coordinator (called the *core*). This
reduces the amount of tasks needing to sleep, which can be a source of
jitter for imprecise sleeping implementations and tight limiters. See below
for more details.

**Dropped tasks** release any resources they've reserved. So that
constructing and cancellaing asynchronous tasks to not end up taking up wait
slots it never uses which would be the case for cell-based rate limiters.

<br>

## Usage

The core type is [`RateLimiter`], which allows for limiting the throughput
of a section using its [`acquire`], [`try_acquire`], and [`acquire_one`]
methods.

The following is a simple example where we wrap requests through a HTTP
`Client`, to ensure that we don't exceed a given limit:

```rust
use leaky_bucket::RateLimiter;

/// A blog client.
pub struct BlogClient {
    limiter: RateLimiter,
    client: Client,
}

struct Post {
    // ..
}

impl BlogClient {
    /// Get all posts from the service.
    pub async fn get_posts(&self) -> Result<Vec<Post>> {
        self.request("posts").await
    }

    /// Perform a request against the service, limiting requests to abide by a rate limit.
    async fn request<T>(&self, path: &str) -> Result<T>
    where
        T: DeserializeOwned
    {
        // Before we start sending a request, we block on acquiring one token.
        self.limiter.acquire(1).await;
        self.client.request::<T>(path).await
    }
}
```

<br>

## Implementation details

Each rate limiter has two acquisition modes. A fast path and a slow path.
The fast path is used if the desired number of tokens are readily available,
and simply involves decrementing the number of tokens available in the
shared pool.

If the required number of tokens is not available, the task will be forced
to be suspended until the next refill interval. Here one of the acquiring
tasks will switch over to work as a *core*. This is known as *core
switching*.

```rust
use leaky_bucket::RateLimiter;
use tokio::time::Duration;

let limiter = RateLimiter::builder()
    .initial(10)
    .interval(Duration::from_millis(100))
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
> cargo run --example block_forever
> ```

```rust
use std::future::Future;
use std::sync::Arc;
use std::task::Context;

use leaky_bucket::RateLimiter;

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

<br>

## Fairness

By default [`RateLimiter`] uses a *fair* scheduler. This ensures that the
core task makes progress even if there are many tasks waiting to acquire
tokens. This might cause more core switching, increasing the total work
needed. An unfair scheduler is expected to do a bit less work under
contention. But without fair scheduling some tasks might end up taking
longer to acquire than expected.

Unfair rate limiters also have access to a fast path for acquiring tokens,
which might further improve throughput.

This behavior can be tweaked with the [`Builder::fair`] option.

```rust
use leaky_bucket::RateLimiter;

let limiter = RateLimiter::builder()
    .fair(false)
    .build();
```

The `unfair-scheduling` example can showcase this phenomenon.

```sh
cargh run --example unfair_scheduling
```

```text
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

[`acquire_one`]: https://docs.rs/leaky-bucket/1/leaky_bucket/struct.RateLimiter.html#method.acquire_one
[`acquire`]: https://docs.rs/leaky-bucket/1/leaky_bucket/struct.RateLimiter.html#method.acquire
[`Builder::fair`]: https://docs.rs/leaky-bucket/1/leaky_bucket/struct.Builder.html#method.fair
[`Mutex`]: https://docs.rs/tokio/1/tokio/sync/struct.Mutex.html
[`RateLimiter`]: https://docs.rs/leaky-bucket/1/leaky_bucket/struct.RateLimiter.html
[`time` feature]: https://docs.rs/tokio/1/tokio/#feature-flags
[`try_acquire`]: https://docs.rs/leaky-bucket/1/leaky_bucket/struct.RateLimiter.html#method.try_acquire
[leaky bucket]: https://en.wikipedia.org/wiki/Leaky_bucket
