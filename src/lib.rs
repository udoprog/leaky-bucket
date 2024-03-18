//! [<img alt="github" src="https://img.shields.io/badge/github-udoprog/leaky--bucket-8da0cb?style=for-the-badge&logo=github" height="20">](https://github.com/udoprog/leaky-bucket)
//! [<img alt="crates.io" src="https://img.shields.io/crates/v/leaky-bucket.svg?style=for-the-badge&color=fc8d62&logo=rust" height="20">](https://crates.io/crates/leaky-bucket)
//! [<img alt="docs.rs" src="https://img.shields.io/badge/docs.rs-leaky--bucket-66c2a5?style=for-the-badge&logoColor=white&logo=data:image/svg+xml;base64,PHN2ZyByb2xlPSJpbWciIHhtbG5zPSJodHRwOi8vd3d3LnczLm9yZy8yMDAwL3N2ZyIgdmlld0JveD0iMCAwIDUxMiA1MTIiPjxwYXRoIGZpbGw9IiNmNWY1ZjUiIGQ9Ik00ODguNiAyNTAuMkwzOTIgMjE0VjEwNS41YzAtMTUtOS4zLTI4LjQtMjMuNC0zMy43bC0xMDAtMzcuNWMtOC4xLTMuMS0xNy4xLTMuMS0yNS4zIDBsLTEwMCAzNy41Yy0xNC4xIDUuMy0yMy40IDE4LjctMjMuNCAzMy43VjIxNGwtOTYuNiAzNi4yQzkuMyAyNTUuNSAwIDI2OC45IDAgMjgzLjlWMzk0YzAgMTMuNiA3LjcgMjYuMSAxOS45IDMyLjJsMTAwIDUwYzEwLjEgNS4xIDIyLjEgNS4xIDMyLjIgMGwxMDMuOS01MiAxMDMuOSA1MmMxMC4xIDUuMSAyMi4xIDUuMSAzMi4yIDBsMTAwLTUwYzEyLjItNi4xIDE5LjktMTguNiAxOS45LTMyLjJWMjgzLjljMC0xNS05LjMtMjguNC0yMy40LTMzLjd6TTM1OCAyMTQuOGwtODUgMzEuOXYtNjguMmw4NS0zN3Y3My4zek0xNTQgMTA0LjFsMTAyLTM4LjIgMTAyIDM4LjJ2LjZsLTEwMiA0MS40LTEwMi00MS40di0uNnptODQgMjkxLjFsLTg1IDQyLjV2LTc5LjFsODUtMzguOHY3NS40em0wLTExMmwtMTAyIDQxLjQtMTAyLTQxLjR2LS42bDEwMi0zOC4yIDEwMiAzOC4ydi42em0yNDAgMTEybC04NSA0Mi41di03OS4xbDg1LTM4Ljh2NzUuNHptMC0xMTJsLTEwMiA0MS40LTEwMi00MS40di0uNmwxMDItMzguMiAxMDIgMzguMnYuNnoiPjwvcGF0aD48L3N2Zz4K" height="20">](https://docs.rs/leaky-bucket)
//!
//! A token-based rate limiter based on the [leaky bucket] algorithm.
//!
//! If the bucket overflows and goes over its max configured capacity, the task
//! that tried to acquire the tokens will be suspended until the required number
//! of tokens has been drained from the bucket.
//!
//! Since this crate uses timing facilities from tokio it has to be used within
//! a Tokio runtime with the [`time` feature] enabled.
//!
//! This library has some neat features, which includes:
//!
//! **Not requiring a background task**. This is usually needed by token bucket
//! rate limiters to drive progress. Instead, one of the waiting tasks
//! temporarily assumes the role as coordinator (called the *core*). This
//! reduces the amount of tasks needing to sleep, which can be a source of
//! jitter for imprecise sleeping implementations and tight limiters. See below
//! for more details.
//!
//! **Dropped tasks** release any resources they've reserved. So that
//! constructing and cancellaing asynchronous tasks to not end up taking up wait
//! slots it never uses which would be the case for cell-based rate limiters.
//!
//! <br>
//!
//! ## Usage
//!
//! The core type is the [`RateLimiter`] type, which allows for limiting the
//! throughput of a section using its [`acquire`] and [`acquire_one`] methods.
//!
//! ```
//! use leaky_bucket::RateLimiter;
//! use tokio::time;
//!
//! # #[tokio::main(flavor="current_thread", start_paused=true)] async fn main() {
//! let limiter = RateLimiter::builder()
//!     .max(10)
//!     .initial(0)
//!     .refill(5)
//!     .build();
//!
//! let start = time::Instant::now();
//!
//! println!("Waiting for permit...");
//!
//! // Should take ~400 ms to acquire in total.
//! let a = limiter.acquire(7);
//! let b = limiter.acquire(3);
//! let c = limiter.acquire(10);
//!
//! let ((), (), ()) = tokio::join!(a, b, c);
//!
//! println!(
//!     "I made it in {:?}!",
//!     time::Instant::now().duration_since(start)
//! );
//! # }
//! ```
//!
//! <br>
//!
//! ## Implementation details
//!
//! Each rate limiter has two acquisition modes. A fast path and a slow path.
//! The fast path is used if the desired number of tokens are readily available,
//! and involves incrementing an atomic counter indicating that the acquired
//! number of tokens have been added to the bucket.
//!
//! If this counter goes over its configured maximum capacity, it overflows into
//! a slow path. Here one of the acquiring tasks will switch over to work as a
//! *core*. This is known as *core switching*.
//!
//! ```
//! use leaky_bucket::RateLimiter;
//! use std::time;
//!
//! # #[tokio::main(flavor="current_thread", start_paused=true)] async fn main() {
//! let limiter = RateLimiter::builder()
//!     .initial(10)
//!     .interval(time::Duration::from_millis(100))
//!     .build();
//!
//! // This is instantaneous since the rate limiter starts with 10 tokens to
//! // spare.
//! limiter.acquire(10).await;
//!
//! // This however needs to core switch and wait for a while until the desired
//! // number of tokens is available.
//! limiter.acquire(3).await;
//! # }
//! ```
//!
//! The core is responsible for sleeping for the configured interval so that
//! more tokens can be added. After which it ensures that any tasks that are
//! waiting to acquire including itself are appropriately unsuspended.
//!
//! On-demand core switching is what allows this rate limiter implementation to
//! work without a coordinating background thread. But we need to ensure that
//! any asynchronous tasks that uses [`RateLimiter`] must either run an
//! [`acquire`] call to completion, or be *cancelled* by being dropped.
//!
//! If none of these hold, the core might leak and be locked indefinitely
//! preventing any future use of the rate limiter from making progress. This is
//! similar to if you would lock an asynchronous [`Mutex`] but never drop its
//! guard.
//!
//! > You can run this example with:
//! >
//! > ```sh
//! > cargo run --example block-forever
//! > ```
//!
//! ```no_run
//! use leaky_bucket::RateLimiter;
//! use std::future::Future;
//! use std::sync::Arc;
//! use std::task::Context;
//!
//! struct Waker;
//! # impl std::task::Wake for Waker { fn wake(self: Arc<Self>) { } }
//!
//! # #[tokio::main(flavor="current_thread", start_paused=true)] async fn main() {
//! let limiter = Arc::new(RateLimiter::builder().build());
//!
//! let waker = Arc::new(Waker).into();
//! let mut cx = Context::from_waker(&waker);
//!
//! let mut a0 = Box::pin(limiter.acquire(1));
//! // Poll once to ensure that the core task is assigned.
//! assert!(a0.as_mut().poll(&mut cx).is_pending());
//! assert!(a0.is_core());
//!
//! // We leak the core task, preventing the rate limiter from making progress
//! // by assigning new core tasks.
//! std::mem::forget(a0);
//!
//! // Awaiting acquire here would block forever.
//! // limiter.acquire(1).await;
//! # }
//! ```
//!
//! <br>
//!
//! ## Fairness
//!
//! By default [`RateLimiter`] uses a *fair* scheduler. This ensures that the
//! core task makes progress even if there are many tasks waiting to acquire
//! tokens. As a result it causes more frequent core switching, increasing the
//! total work needed. An unfair scheduler is expected to do a bit less work
//! under contention. But without fair scheduling some tasks might end up taking
//! longer to acquire than expected.
//!
//! This behavior can be tweaked with the [`Builder::fair`] option.
//!
//! ```
//! use leaky_bucket::RateLimiter;
//!
//! let limiter = RateLimiter::builder()
//!     .fair(false)
//!     .build();
//! ```
//!
//! The `unfair-scheduling` example can showcase this phenomenon.
//!
//! ```sh
//! cargh run --example unfair-scheduling
//! ```
//!
//! ```text
//! # fair
//! Max: 1011ms, Total: 1012ms
//! Timings:
//!  0: 101ms
//!  1: 101ms
//!  2: 101ms
//!  3: 101ms
//!  4: 101ms
//!  ...
//! # unfair
//! Max: 1014ms, Total: 1014ms
//! Timings:
//!  0: 1014ms
//!  1: 101ms
//!  2: 101ms
//!  3: 101ms
//!  4: 101ms
//!  ...
//! ```
//!
//! As can be seen above the first task in the *unfair* scheduler takes longer
//! to run because it prioritises releasing other tasks waiting to acquire over
//! itself.
//!
//! [`acquire_one`]: https://docs.rs/leaky-bucket/0/leaky_bucket/struct.RateLimiter.html#method.acquire_one
//! [`acquire`]: https://docs.rs/leaky-bucket/0/leaky_bucket/struct.RateLimiter.html#method.acquire
//! [`Builder::fair`]: https://docs.rs/leaky-bucket/0/leaky_bucket/struct.Builder.html#method.fair
//! [`Mutex`]: https://docs.rs/tokio/1/tokio/sync/struct.Mutex.html
//! [`RateLimiter`]: https://docs.rs/leaky-bucket/0/leaky_bucket/struct.RateLimiter.html
//! [`time` feature]: https://docs.rs/tokio/1/tokio/#feature-flags
//! [leaky bucket]: https://en.wikipedia.org/wiki/Leaky_bucket

#![no_std]
#![deny(missing_docs)]

extern crate alloc;

#[macro_use]
extern crate std;

use core::cell::UnsafeCell;
use core::convert::TryFrom as _;
use core::fmt;
use core::future::Future;
use core::marker;
use core::mem;
use core::pin::Pin;
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::{Context, Poll, Waker};

use alloc::sync::Arc;

use parking_lot::{Mutex, MutexGuard};
use pin_project_lite::pin_project;
use tokio::time;
use tracing::trace;

#[doc(hidden)]
pub mod linked_list;
use self::linked_list::{LinkedList, Node};

/// Default factor for how to calculate max refill value.
const DEFAULT_REFILL_MAX_FACTOR: usize = 10;

/// Interval to bump the shared mutex guard to allow other parts of the system
/// to make process. Processes which loop should use this number to determine
/// how many times it should loop before calling [MutexGuard::bump].
///
/// If we do not respect this limit we might inadvertently end up starving other
/// tasks from making progress so that they can unblock.
const BUMP_LIMIT: usize = 16;

/// Linked task state.
struct Task {
    /// Remaining tokens that need to be satisfied.
    remaining: usize,
    /// Link to [Linking::complete].
    complete: Option<ptr::NonNull<AtomicBool>>,
    /// The waker associated with the node.
    waker: Option<Waker>,
}

impl Task {
    /// Construct a new task state with the given permits remaining.
    const fn new() -> Self {
        Self {
            remaining: 0,
            complete: None,
            waker: None,
        }
    }

    /// Test if the current node is completed.
    fn is_completed(&self) -> bool {
        self.remaining == 0
    }

    /// Fill the current node from the given pool of tokens and modify it.
    fn fill(&mut self, current: &mut usize) {
        let removed = usize::min(self.remaining, *current);
        self.remaining -= removed;
        *current -= removed;
    }
}

/// A borrowed rate limiter.
struct BorrowedRateLimiter<'a>(&'a RateLimiter);

impl AsRef<RateLimiter> for BorrowedRateLimiter<'_> {
    fn as_ref(&self) -> &RateLimiter {
        self.0
    }
}

struct Critical {
    /// Current balance of tokens. A value of 0 means that it is empty. Goes up
    /// to [`RateLimiter::max`].
    balance: usize,
    /// Waiter list.
    waiters: LinkedList<Task>,
    /// The deadline for when more tokens can be be added.
    deadline: time::Instant,
    /// If the core is available.
    available: bool,
}

impl Critical {
    /// Release the current core. Beyond this point the current task may no
    /// longer interact exclusively with the core.
    #[tracing::instrument(skip(self), level = "trace")]
    fn release(&mut self) {
        trace!("releasing core");
        self.available = true;

        // Find another task that might take over as core. Once it has acquired
        // core status it will have to make sure it is no longer linked into the
        // wait queue.
        //
        // We have to do this, because another task might miss that the core is
        // available since it's hidden behind an atomic, so we wake any task up
        // to ensure that it will always be picked up.
        //
        // Safety: We're holding the lock guard to all the waiters so we can be
        // certain that we have exclusive access.
        unsafe {
            if let Some(mut node) = self.waiters.front_mut() {
                trace!(node = ?node, "waking next core");

                if let Some(waker) = node.as_mut().waker.take() {
                    waker.wake();
                }
            }
        }
    }
}

/// A token-bucket rate limiter.
pub struct RateLimiter {
    /// Tokens to add every `per` duration.
    refill: usize,
    /// Interval in milliseconds to add tokens.
    interval: time::Duration,
    /// Max number of tokens associated with the rate limiter.
    max: usize,
    /// If the rate limiter is fair or not.
    fair: bool,
    /// Critical state of the rate limiter.
    critical: Mutex<Critical>,
}

impl RateLimiter {
    /// Construct a new [`Builder`] for a [`RateLimiter`].
    ///
    /// # Examples
    ///
    /// ```
    /// use leaky_bucket::RateLimiter;
    /// use std::time::Duration;
    ///
    /// let limiter = RateLimiter::builder()
    ///     .initial(100)
    ///     .refill(100)
    ///     .max(1000)
    ///     .interval(Duration::from_millis(250))
    ///     .fair(false)
    ///     .build();
    /// ```
    pub fn builder() -> Builder {
        Builder::default()
    }

    /// Get the refill amount  of this rate limiter as set through
    /// [`Builder::refill`].
    ///
    /// # Examples
    ///
    /// ```
    /// use leaky_bucket::RateLimiter;
    ///
    /// let limiter = RateLimiter::builder()
    ///     .refill(1024)
    ///     .build();
    ///
    /// assert_eq!(limiter.refill(), 1024);
    /// ```
    pub fn refill(&self) -> usize {
        self.refill
    }

    /// Get the refill interval of this rate limiter as set through
    /// [`Builder::interval`].
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use leaky_bucket::RateLimiter;
    ///
    /// let limiter = RateLimiter::builder()
    ///     .interval(Duration::from_millis(1000))
    ///     .build();
    ///
    /// assert_eq!(limiter.interval(), Duration::from_millis(1000));
    /// ```
    pub fn interval(&self) -> time::Duration {
        self.interval
    }

    /// Get the max value of this rate limiter as set through [`Builder::max`].
    ///
    /// # Examples
    ///
    /// ```
    /// use leaky_bucket::RateLimiter;
    ///
    /// let limiter = RateLimiter::builder()
    ///     .max(1024)
    ///     .build();
    ///
    /// assert_eq!(limiter.max(), 1024);
    /// ```
    pub fn max(&self) -> usize {
        self.max
    }

    /// Test if the current rate limiter is fair as specified through
    /// [`Builder::fair`].
    ///
    /// # Examples
    ///
    /// ```
    /// use leaky_bucket::RateLimiter;
    ///
    /// let limiter = RateLimiter::builder()
    ///     .fair(true)
    ///     .build();
    ///
    /// assert_eq!(limiter.is_fair(), true);
    /// ```
    pub fn is_fair(&self) -> bool {
        self.fair
    }

    /// Get the current token balance.
    ///
    /// This indicates how many tokens can be requested without blocking.
    ///
    /// # Examples
    ///
    /// ```
    /// use leaky_bucket::RateLimiter;
    ///
    /// # #[tokio::main(flavor="current_thread", start_paused=true)] async fn main() {
    /// let limiter = RateLimiter::builder()
    ///     .initial(100)
    ///     .build();
    ///
    /// assert_eq!(limiter.balance(), 100);
    /// limiter.acquire(10).await;
    /// assert_eq!(limiter.balance(), 90);
    /// # }
    /// ```
    pub fn balance(&self) -> usize {
        self.critical.lock().balance
    }

    /// Acquire a single permit.
    ///
    /// # Examples
    ///
    /// ```
    /// use leaky_bucket::RateLimiter;
    ///
    /// # #[tokio::main(flavor="current_thread", start_paused=true)] async fn main() {
    /// let limiter = RateLimiter::builder()
    ///     .initial(10)
    ///     .build();
    ///
    /// limiter.acquire_one().await;
    /// # }
    /// ```
    pub fn acquire_one(&self) -> Acquire<'_> {
        self.acquire(1)
    }

    /// Acquire the given number of permits, suspending the current task until
    /// they are available.
    ///
    /// If zero permits are specified, this function never suspends the current
    /// task.
    ///
    /// # Examples
    ///
    /// ```
    /// use leaky_bucket::RateLimiter;
    ///
    /// # #[tokio::main(flavor="current_thread", start_paused=true)] async fn main() {
    /// let limiter = RateLimiter::builder()
    ///     .initial(10)
    ///     .build();
    ///
    /// limiter.acquire(10).await;
    /// # }
    /// ```
    pub fn acquire(&self, permits: usize) -> Acquire<'_> {
        Acquire {
            inner: AcquireFut::new(BorrowedRateLimiter(self), permits),
        }
    }

    /// Acquire a permit using an owned future.
    ///
    /// If zero permits are specified, this function never suspends the current
    /// task.
    ///
    /// This required the [`RateLimiter`] to be wrapped inside of an
    /// [`std::sync::Arc`] but will in contrast permit the acquire operation to
    /// be owned by another struct making it more suitable for embedding.
    ///
    /// # Examples
    ///
    /// ```
    /// use leaky_bucket::RateLimiter;
    /// use std::sync::Arc;
    ///
    /// # #[tokio::main(flavor="current_thread", start_paused=true)] async fn main() {
    /// let limiter = Arc::new(RateLimiter::builder().initial(10).build());
    ///
    /// limiter.acquire_owned(10).await;
    /// # }
    /// ```
    ///
    /// Example when embedded into another future. This wouldn't be possible
    /// with [`RateLimiter::acquire`] since it would otherwise hold a reference
    /// to the corresponding [`RateLimiter`] instance.
    ///
    /// ```
    /// use leaky_bucket::{AcquireOwned, RateLimiter};
    /// use pin_project::pin_project;
    /// use std::future::Future;
    /// use std::pin::Pin;
    /// use std::sync::Arc;
    /// use std::task::{Context, Poll};
    /// use std::time::Duration;
    ///
    /// #[pin_project]
    /// struct MyFuture {
    ///     limiter: Arc<RateLimiter>,
    ///     #[pin]
    ///     acquire: Option<AcquireOwned>,
    /// }
    ///
    /// impl Future for MyFuture {
    ///     type Output = ();
    ///
    ///     fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    ///         let mut this = self.project();
    ///
    ///         loop {
    ///             if let Some(acquire) = this.acquire.as_mut().as_pin_mut() {
    ///                 futures::ready!(acquire.poll(cx));
    ///                 return Poll::Ready(());
    ///             }
    ///
    ///             this.acquire.set(Some(this.limiter.clone().acquire_owned(100)));
    ///         }
    ///     }
    /// }
    ///
    /// # #[tokio::main(flavor="current_thread", start_paused=true)] async fn main() {
    /// let limiter = Arc::new(RateLimiter::builder().initial(100).build());
    ///
    /// let future = MyFuture { limiter, acquire: None };
    /// future.await;
    /// # }
    /// ```
    pub fn acquire_owned(self: Arc<Self>, permits: usize) -> AcquireOwned {
        AcquireOwned {
            inner: AcquireFut::new(self, permits),
        }
    }
}

// Safety: All the internals of acquire is thread safe and correctly
// synchronized. The embedded waiter queue doesn't have anything inherently
// unsafe in it.
unsafe impl Send for RateLimiter {}
unsafe impl Sync for RateLimiter {}

/// A builder for a [`RateLimiter`].
pub struct Builder {
    /// The max number of tokens.
    max: Option<usize>,
    /// The initial count of tokens.
    initial: usize,
    /// Tokens to add every `per` duration.
    refill: usize,
    /// Interval to add tokens in milliseconds.
    interval: time::Duration,
    /// If the rate limiter is fair or not.
    fair: bool,
}

impl Builder {
    /// Configure the max number of tokens to use.
    ///
    /// If unspecified, this will default to be 2 times the [`refill`] or the
    /// [`initial`] value, whichever is largest.
    ///
    /// # Examples
    ///
    /// ```
    /// use leaky_bucket::RateLimiter;
    ///
    /// let limiter = RateLimiter::builder()
    ///     .max(10_000)
    ///     .build();
    /// ```
    ///
    /// [`refill`]: Builder::refill
    /// [`initial`]: Builder::initial
    pub fn max(&mut self, max: usize) -> &mut Self {
        self.max = Some(max);
        self
    }

    /// Configure the initial number of tokens to configure. The default value
    /// is `0`.
    ///
    /// # Examples
    ///
    /// ```
    /// use leaky_bucket::RateLimiter;
    ///
    /// let limiter = RateLimiter::builder()
    ///     .initial(10)
    ///     .build();
    /// ```
    pub fn initial(&mut self, initial: usize) -> &mut Self {
        self.initial = initial;
        self
    }

    /// Configure the time duration between which we add [`refill`] number to
    /// the bucket rate limiter.
    ///
    /// # Panics
    ///
    /// This panics if the provided interval does not fit within the millisecond
    /// bounds of a [usize] or is zero.
    ///
    /// ```should_panic
    /// use leaky_bucket::RateLimiter;
    /// use std::time;
    ///
    /// let limiter = RateLimiter::builder()
    ///     .interval(time::Duration::from_secs(u64::MAX))
    ///     .build();
    /// ```
    ///
    /// ```should_panic
    /// use leaky_bucket::RateLimiter;
    /// use std::time;
    ///
    /// let limiter = RateLimiter::builder()
    ///     .interval(time::Duration::from_millis(0))
    ///     .build();
    /// ```
    ///
    /// # Examples
    ///
    /// ```
    /// use leaky_bucket::RateLimiter;
    /// use std::time;
    ///
    /// let limiter = RateLimiter::builder()
    ///     .interval(time::Duration::from_millis(100))
    ///     .build();
    /// ```
    ///
    /// [`refill`]: Builder::refill
    pub fn interval(&mut self, interval: time::Duration) -> &mut Self {
        assert! {
            interval.as_millis() != 0,
            "interval must be non-zero",
        };
        assert! {
            u64::try_from(interval.as_millis()).is_ok(),
            "interval must fit within a 64-bit integer"
        };
        self.interval = interval;
        self
    }

    /// The number of tokens to add at each [`interval`] interval. The default
    /// value is `1`.
    ///
    /// # Panics
    ///
    /// Panics if a refill amount of `0` is specified.
    ///
    /// # Examples
    ///
    /// ```
    /// use leaky_bucket::RateLimiter;
    /// use std::time;
    ///
    /// let limiter = RateLimiter::builder()
    ///     .refill(100)
    ///     .build();
    /// ```
    ///
    /// [`interval`]: Builder::interval
    pub fn refill(&mut self, refill: usize) -> &mut Self {
        assert!(refill > 0, "refill amount cannot be zero");
        self.refill = refill;
        self
    }

    /// Configure the rate limiter to be fair. By default the rate limiter is
    /// *fair* which ensures that all tasks make steady progress even under
    /// contention. But an unfair scheduler might have a higher total
    /// throughput.
    ///
    /// # Examples
    ///
    /// ```
    /// use leaky_bucket::RateLimiter;
    ///
    /// let limiter = RateLimiter::builder()
    ///     .refill(100)
    ///     .fair(false)
    ///     .build();
    /// ```
    pub fn fair(&mut self, fair: bool) -> &mut Self {
        self.fair = fair;
        self
    }

    /// Construct a new [`RateLimiter`].
    ///
    /// # Examples
    ///
    /// ```
    /// use leaky_bucket::RateLimiter;
    /// use std::time;
    ///
    /// let limiter = RateLimiter::builder()
    ///     .refill(100)
    ///     .interval(time::Duration::from_millis(200))
    ///     .max(10_000)
    ///     .build();
    /// ```
    pub fn build(&self) -> RateLimiter {
        let deadline = time::Instant::now() + self.interval;

        let max = match self.max {
            Some(max) => max,
            None => usize::max(self.refill, self.initial).saturating_mul(DEFAULT_REFILL_MAX_FACTOR),
        };

        let initial = usize::min(self.initial, max);

        RateLimiter {
            refill: self.refill,
            interval: self.interval,
            max,
            fair: self.fair,
            critical: Mutex::new(Critical {
                balance: initial,
                waiters: LinkedList::new(),
                deadline,
                available: true,
            }),
        }
    }
}

/// Construct a new builder with default options.
///
/// # Examples
///
/// ```
/// use leaky_bucket::Builder;
///
/// let limiter = Builder::default().build();
/// ```
impl Default for Builder {
    fn default() -> Self {
        Self {
            max: None,
            initial: 0,
            refill: 1,
            interval: time::Duration::from_millis(100),
            fair: true,
        }
    }
}

pin_project! {
    /// The state of an acquire operation.
    #[project = StateProj]
    #[allow(clippy::large_enum_variant)]
    enum State {
        // Initial unconfigured state.
        Initial,
        // The acquire is waiting to be released by the core.
        Waiting,
        // This operation is currently the core.
        //
        // We need to take care to ensure that we don't move the configured sleep.
        // Since it needs to be pinned to be polled.
        Core {
            #[pin]
            // The current sleep of the core.
            sleep: time::Sleep,
        },
        // The operation is completed.
        Complete,
    }
}

/// Internal state of the acquire. This is separated because it can be computed
/// in constant time.
struct AcquireState {
    /// If we are linked or not.
    linked: bool,
    /// Inner state of the acquire.
    linking: UnsafeCell<Linking>,
}

impl AcquireState {
    #[allow(clippy::declare_interior_mutable_const)]
    const INITIAL: AcquireState = AcquireState {
        linked: false,
        linking: UnsafeCell::new(Linking {
            task: Node::new(Task::new()),
            complete: AtomicBool::new(false),
            _pin: marker::PhantomPinned,
        }),
    };

    /// Access the completion flag.
    pub fn complete(&self) -> &AtomicBool {
        // Safety: This is always safe to access since it's atomic.
        unsafe {
            let ptr = self.linking.get() as *const _ as *const Node<Task>;
            let ptr = ptr.add(1) as *const AtomicBool;
            &*ptr
        }
    }

    /// Get the underlying task.
    pub unsafe fn task(&self) -> &Node<Task> {
        let ptr = self.linking.get() as *mut Node<Task>;
        &*ptr
    }

    /// Get the underlying task mutably.
    pub unsafe fn task_mut(&mut self) -> &mut Node<Task> {
        let ptr = self.linking.get() as *mut Node<Task>;
        &mut *ptr
    }

    /// Get the underlying task mutably and completion flag as a pair.
    pub unsafe fn update_project(&mut self) -> (&mut Node<Task>, &AtomicBool, &mut bool) {
        let node = self.linking.get() as *mut Node<Task>;
        let complete = node.add(1) as *const _ as *const AtomicBool;
        let node = &mut *(node as *mut Node<Task>);
        let complete = &*complete;
        (node, complete, &mut self.linked)
    }

    /// Update the waiting state for this acquisition task. This might require
    /// that we update the associated waker.
    #[tracing::instrument(skip(self, critical, waker), level = "trace")]
    fn update(&mut self, critical: &mut MutexGuard<'_, Critical>, waker: &Waker) {
        // Safety: we're ensured to do this under the critical lock since we've
        // passed the relevant guard in through `waiters`.
        let (task, complete, linked) = unsafe { self.update_project() };

        if !*linked {
            trace!("linking self");
            *linked = true;

            unsafe {
                critical.waiters.push_front(task.into());
            }
        }

        let w = &mut task.waker;

        let new_waker = match w {
            None => true,
            Some(w) => !w.will_wake(waker),
        };

        if new_waker {
            trace!("updating waker");
            *w = Some(waker.clone());
        }

        if task.complete.is_none() {
            trace!("setting complete");
            task.complete = Some(complete.into());
        }
    }

    /// Ensure that the current core task is correctly linked up if needed.
    #[tracing::instrument(skip(self, critical, lim), level = "trace")]
    unsafe fn link_core(&mut self, critical: &mut Critical, lim: &RateLimiter) {
        if lim.fair {
            // Fair scheduling needs to ensure that the core is part of the wait
            // queue, and will be woken up in-order with other tasks.
            if !mem::replace(&mut self.linked, true) {
                critical.waiters.push_front(self.task_mut().into());
            }
        } else {
            // Unfair scheduling the core task is not supposed to be in the wait
            // queue, so remove it from there if we've successfully stolen it.
            // Ensure that the current task is *not* linked since it is now to
            // become the coordinator for everyone else.
            if mem::take(&mut self.linked) {
                critical.waiters.remove(self.task_mut().into());
            }
        }
    }

    /// Release any remaining tokens which are associated with this particular task.
    unsafe fn release_remaining(
        &mut self,
        critical: &mut MutexGuard<'_, Critical>,
        permits: usize,
        lim: &RateLimiter,
    ) {
        if mem::take(&mut self.linked) {
            critical.waiters.remove(self.task_mut().into());
        }

        // Hand back permits which we've acquired so far.
        let release = permits.saturating_sub(self.linking.get_mut().task.remaining);

        // Temporarily assume the role of core and release the remaining
        // tokens to waiting tasks.
        if release > 0 {
            self.drain_wait_queue(critical, release, lim);
        }
    }

    /// Refill the wait queue with the given number of tokens.
    #[tracing::instrument(skip(self, critical, lim), level = "trace")]
    fn drain_wait_queue(
        &self,
        critical: &mut MutexGuard<'_, Critical>,
        tokens: usize,
        lim: &RateLimiter,
    ) {
        critical.balance = critical.balance.saturating_add(tokens);
        trace!(tokens = tokens, "draining tokens");

        let mut bump = 0;

        // Safety: we're holding the lock guard to all the waiters so we can be
        // sure that we have exclusive access to the wait queue.
        unsafe {
            while critical.balance > 0 {
                let mut node = match critical.waiters.pop_back() {
                    Some(node) => node,
                    None => break,
                };

                let n = node.as_mut();
                n.fill(&mut critical.balance);

                trace! {
                    balance = critical.balance,
                    remaining = n.remaining,
                    "filled node",
                };

                if !n.is_completed() {
                    critical.waiters.push_back(node);
                    break;
                }

                if let Some(complete) = n.complete.take() {
                    complete.as_ref().store(true, Ordering::Release);
                }

                if let Some(waker) = n.waker.take() {
                    waker.wake();
                }

                bump += 1;

                if bump == BUMP_LIMIT {
                    MutexGuard::bump(critical);
                    bump = 0;
                }
            }
        }

        if critical.balance > lim.max {
            critical.balance = lim.max;
        }
    }

    /// Drain the given number of tokens through the core. Returns `true` if the
    /// core has been completed.
    #[tracing::instrument(skip(self, critical, tokens, lim), level = "trace")]
    fn drain_core(
        &mut self,
        critical: &mut MutexGuard<'_, Critical>,
        tokens: usize,
        lim: &RateLimiter,
    ) -> bool {
        self.drain_wait_queue(critical, tokens, lim);

        if lim.fair {
            debug_assert! {
                self.linked,
                "core must be linked for fair scheduler",
            };

            // We only need to check the state since the current core holder is
            // linked up to the wait queue.
            //
            // Safety: we're doing this under the critical lock so we know we
            // have exclusive access to the node.
            if unsafe { self.task().is_completed() } {
                // Task was unlinked by the drain action.
                self.linked = false;
                return true;
            }

            false
        } else {
            debug_assert! {
                !self.linked,
                "core must not be linked for an unfair scheduler",
            };

            // If the limiter is not fair, we need to in addition to draining
            // remaining tokens from linked nodes, drain it from ourselves. We
            // fill the current holder of the core last (self). To ensure that
            // it stays around for as long as possible.
            //
            // Safety: we know that no one else holds the task at this point.
            // The in particular the task is not linked into the wait queue.
            let c = unsafe { &mut *self.task_mut() };
            c.fill(&mut critical.balance);
            c.is_completed()
        }
    }

    /// Assume the current core and calculate how long we must sleep for in
    /// order to do it.
    ///
    /// # Safety
    ///
    /// This might link the current task into the task queue, so the caller must
    /// ensure that it is pinned.
    #[tracing::instrument(skip(self, critical, lim), level = "trace")]
    unsafe fn assume_core(
        &mut self,
        critical: &mut MutexGuard<'_, Critical>,
        lim: &RateLimiter,
    ) -> bool {
        self.link_core(critical, lim);

        let (tokens, deadline) = match calculate_drain(critical.deadline, lim.interval) {
            Some(tokens) => tokens,
            None => return true,
        };

        // It is appropriate to update the deadline.
        critical.deadline = deadline;

        if self.drain_core(critical, tokens, lim) {
            // We synthetically "ran" at the current time minus the remaining time
            // we need to wait until the last update period.
            critical.release();
            return false;
        }

        true
    }
}

impl fmt::Debug for AcquireState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AcquireState").finish()
    }
}

pin_project! {
    /// The future associated with acquiring permits from a rate limiter using
    /// [`RateLimiter::acquire`].
    pub struct Acquire<'a>{
        #[pin]
        inner: AcquireFut<BorrowedRateLimiter<'a>>
    }
}

impl Acquire<'_> {
    /// Test if this acquire task is currently coordinating the rate limiter.
    ///
    /// # Examples
    ///
    /// ```
    /// use leaky_bucket::RateLimiter;
    /// use std::future::Future;
    /// use std::sync::Arc;
    /// use std::task::Context;
    ///
    /// struct Waker;
    /// # impl std::task::Wake for Waker { fn wake(self: Arc<Self>) { } }
    ///
    /// # #[tokio::main(flavor="current_thread", start_paused=true)] async fn main() {
    /// let limiter = RateLimiter::builder().build();
    ///
    /// let waker = Arc::new(Waker).into();
    /// let mut cx = Context::from_waker(&waker);
    ///
    /// let a1 = limiter.acquire(1);
    /// tokio::pin!(a1);
    ///
    /// assert!(!a1.is_core());
    /// assert!(a1.as_mut().poll(&mut cx).is_pending());
    /// assert!(a1.is_core());
    ///
    /// a1.as_mut().await;
    ///
    /// // After completion this is no longer a core.
    /// assert!(!a1.is_core());
    /// # }
    /// ```
    pub fn is_core(&self) -> bool {
        self.inner.is_core()
    }
}

impl Future for Acquire<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().inner.poll(cx)
    }
}

pin_project! {
    /// The future associated with acquiring permits from a rate limiter using
    /// [`RateLimiter::acquire_owned`].
    pub struct AcquireOwned{
        #[pin]
        inner: AcquireFut<Arc<RateLimiter>>
    }
}

impl AcquireOwned {
    /// Test if this acquire task is currently coordinating the rate limiter.
    ///
    /// # Examples
    ///
    /// ```
    /// use leaky_bucket::RateLimiter;
    /// use std::future::Future;
    /// use std::sync::Arc;
    /// use std::task::Context;
    ///
    /// struct Waker;
    /// # impl std::task::Wake for Waker { fn wake(self: Arc<Self>) { } }
    ///
    /// # #[tokio::main(flavor="current_thread", start_paused=true)] async fn main() {
    /// let limiter = Arc::new(RateLimiter::builder().build());
    ///
    /// let waker = Arc::new(Waker).into();
    /// let mut cx = Context::from_waker(&waker);
    ///
    /// let a1 = limiter.acquire_owned(1);
    /// tokio::pin!(a1);
    ///
    /// assert!(!a1.is_core());
    /// assert!(a1.as_mut().poll(&mut cx).is_pending());
    /// assert!(a1.is_core());
    ///
    /// a1.as_mut().await;
    ///
    /// // After completion this is no longer a core.
    /// assert!(!a1.is_core());
    /// # }
    /// ```
    pub fn is_core(&self) -> bool {
        self.inner.is_core()
    }
}

impl Future for AcquireOwned {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().inner.poll(cx)
    }
}

pin_project! {
    struct AcquireFut<T>
    where
        T: AsRef<RateLimiter>,
    {
        // Inner shared state.
        lim: T,
        // The number of permits associated with this future.
        permits: usize,
        #[pin]
        // State of the acquisition.
        state: State,
        // The internal acquire state.
        internal: AcquireState,
    }

    impl<T> PinnedDrop for AcquireFut<T>
    where
        T: AsRef<RateLimiter>,
    {
        fn drop(this: Pin<&mut Self>) {
            let this = this.project();
            let lim = this.lim.as_ref();

            match this.state.project() {
                StateProj::Waiting => unsafe {
                    debug_assert! {
                        this.internal.linked,
                        "waiting nodes have to be linked",
                    };

                    // While the node is linked into the wait queue we have to
                    // ensure it's only accessed under a lock, but once it's been
                    // unlinked we can do what we want with it.
                    let mut critical = lim.critical.lock();
                    this.internal
                        .release_remaining(&mut critical, *this.permits, lim);
                },
                StateProj::Core { .. } => unsafe {
                    let mut critical = lim.critical.lock();
                    this.internal
                        .release_remaining(&mut critical, *this.permits, lim);
                    critical.release();
                },
                _ => (),
            }
        }
    }
}

impl<T> AcquireFut<T>
where
    T: AsRef<RateLimiter>,
{
    #[inline]
    fn new(lim: T, permits: usize) -> Self {
        Self {
            lim,
            permits,
            state: State::Initial,
            internal: AcquireState::INITIAL,
        }
    }

    fn is_core(&self) -> bool {
        matches!(&self.state, State::Core { .. })
    }
}

// Safety: All the internals of acquire is thread safe and correctly
// synchronized. The embedded waiter queue doesn't have anything inherently
// unsafe in it.
unsafe impl<T> Send for AcquireFut<T> where T: AsRef<RateLimiter> {}
unsafe impl<T> Sync for AcquireFut<T> where T: AsRef<RateLimiter> {}

impl<T> Future for AcquireFut<T>
where
    T: AsRef<RateLimiter>,
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        let lim = this.lim.as_ref();

        loop {
            match this.state.as_mut().project() {
                StateProj::Initial => {
                    // Safety: The task is not linked up yet, so we can safely
                    // inspect the number of permits without having to
                    // synchronize.
                    if *this.permits == 0 {
                        this.state.set(State::Complete);
                        return Poll::Ready(());
                    }

                    let mut critical = lim.critical.lock();

                    // If we've hit a deadline, calculate the number of tokens
                    // to drain and perform it in line here. This is necessary
                    // because the core isn't aware of how long we sleep between
                    // each acquire, so we need to perform some of the drain
                    // work here in order to avoid acruing a debt that needs to
                    // be filled later in.
                    //
                    // If we didn't do this, and the process slept for a long
                    // time, the next time a core is acquired it would be very
                    // far removed from the expected deadline and has no idea
                    // when permits were acquired, so it would over-eagerly
                    // release a lot of acquires and accumulate permits.
                    //
                    // This is tested for in the `test_idle` suite of tests.
                    if let Some((tokens, deadline)) =
                        calculate_drain(critical.deadline, lim.interval)
                    {
                        trace!(tokens = tokens, "inline drain");
                        // We pre-emptively update the deadline of the core
                        // since it might bump, and we don't want other
                        // processes to observe that the deadline has been
                        // reached.
                        critical.deadline = deadline;
                        this.internal.drain_wait_queue(&mut critical, tokens, lim);
                    }

                    // Test the fast path first, where we simply subtract the
                    // permits available from the current balance.
                    if let Some(balance) = critical.balance.checked_sub(*this.permits) {
                        critical.balance = balance;
                        this.state.set(State::Complete);
                        return Poll::Ready(());
                    }

                    let balance = mem::take(&mut critical.balance);

                    // Safety: This is done in a pinned section, so we know that
                    // the linked section stays alive for the duration of this
                    // future due to pinning guarantees.
                    unsafe {
                        this.internal.task_mut().remaining = *this.permits - balance;
                    }

                    // Try to take over as core. If we're unsuccessful we just
                    // ensure that we're linked into the wait queue.
                    if !mem::take(&mut critical.available) {
                        this.internal.update(&mut critical, cx.waker());
                        this.state.set(State::Waiting);
                        return Poll::Pending;
                    }

                    // Safety: This is done in a pinned section, so we know that
                    // the linked section stays alive for the duration of this
                    // future due to pinning guarantees.
                    unsafe { this.internal.link_core(&mut critical, lim) };

                    trace!(until = ?critical.deadline, "taking over core and sleeping");
                    this.state.set(State::Core {
                        sleep: time::sleep_until(critical.deadline),
                    });

                    trace!("no immediate tokens available");
                }
                StateProj::Waiting => {
                    // If we are complete, then return as ready.
                    //
                    // This field is atomic, so we can safely read it under shared
                    // access and do not require a lock.
                    if this.internal.complete().load(Ordering::Acquire) {
                        this.state.set(State::Complete);
                        return Poll::Ready(());
                    }

                    // Note: we need to operate under this lock to ensure that
                    // the core acquired here (or elsewhere) observes that the
                    // current task has been linked up.
                    let mut critical = lim.critical.lock();

                    // Try to take over as core. If we're unsuccessful we
                    // just ensure that we're linked into the wait queue.
                    if !mem::take(&mut critical.available) {
                        this.internal.update(&mut critical, cx.waker());
                        return Poll::Pending;
                    }

                    // Safety: This is done in a pinned section, so we know that
                    // the linked section stays alive for the duration of this
                    // future due to pinning guarantees.
                    let assumed = unsafe { this.internal.assume_core(&mut critical, lim) };

                    if !assumed {
                        // Marks as completed.
                        this.state.set(State::Complete);
                        return Poll::Ready(());
                    }

                    trace!(until = ?critical.deadline, "taking over core and sleeping");
                    this.state.set(State::Core {
                        sleep: time::sleep_until(critical.deadline),
                    });
                }
                StateProj::Core { mut sleep } => {
                    if sleep.as_mut().poll(cx).is_pending() {
                        return Poll::Pending;
                    }

                    let now = time::Instant::now();
                    trace!(now = ?now, "sleep completed");
                    let mut critical = lim.critical.lock();
                    critical.deadline = now + lim.interval;

                    // Safety: we know that we're the only one with access to core
                    // because we ensured it as we acquire the `available` lock.
                    if this.internal.drain_core(&mut critical, lim.refill, lim) {
                        critical.release();
                        this.state.set(State::Complete);
                        return Poll::Ready(());
                    }

                    trace!(sleep = ?lim.interval, "keeping core and sleeping");
                    sleep.as_mut().reset(critical.deadline);
                }
                StateProj::Complete => {
                    panic!("polled after completion");
                }
            }
        }
    }
}

/// All of the state that is linked into the wait queue.
///
/// This is only ever accessed through raw pointer manipulation to avoid issues
/// with field aliasing.
#[repr(C)]
struct Linking {
    /// The node in the linked list.
    task: Node<Task>,
    /// If this node has been released or not. We make this an atomic to permit
    /// access to it without synchronization.
    complete: AtomicBool,
    /// Avoids noalias heuristics from kicking in on references to a `Linking`
    /// struct.
    _pin: marker::PhantomPinned,
}

/// Calculate refill amount. Returning a tuple of how much to fill and remaining
/// duration to sleep until the next refill time if appropriate.
fn calculate_drain(
    deadline: time::Instant,
    interval: time::Duration,
) -> Option<(usize, time::Instant)> {
    let now = time::Instant::now();

    if now < deadline {
        return None;
    }

    // Time elapsed in milliseconds since the last deadline.
    let millis = interval.as_millis();
    let since = now.saturating_duration_since(deadline).as_millis();

    let tokens = usize::try_from(since / millis + 1).unwrap_or(usize::MAX);
    let rem = u64::try_from(since % millis).unwrap_or(u64::MAX);

    // Calculated time remaining until the next deadline.
    let deadline = now + (interval - time::Duration::from_millis(rem));
    Some((tokens, deadline))
}

#[cfg(test)]
mod tests {
    use super::{Acquire, AcquireOwned, RateLimiter};

    fn is_send<T: Send>() {}
    fn is_sync<T: Sync>() {}

    #[test]
    fn assert_send_sync() {
        is_send::<AcquireOwned>();
        is_sync::<AcquireOwned>();

        is_send::<RateLimiter>();
        is_sync::<RateLimiter>();

        is_send::<Acquire<'_>>();
        is_sync::<Acquire<'_>>();
    }
}
