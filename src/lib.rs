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
//! The core type is [`RateLimiter`], which allows for limiting the throughput
//! of a section using its [`acquire`], [`try_acquire`], and [`acquire_one`]
//! methods.
//!
//! The following is a simple example where we wrap requests through a HTTP
//! `Client`, to ensure that we don't exceed a given limit:
//!
//! ```
//! use leaky_bucket::RateLimiter;
//! # struct Client;
//! # impl Client { async fn request<T>(&self, path: &str) -> Result<T> { todo!() } }
//! # trait DeserializeOwned {}
//! # impl DeserializeOwned for Vec<Post> {}
//! # type Result<T> = core::result::Result<T, ()>;
//!
//! /// A blog client.
//! pub struct BlogClient {
//!     limiter: RateLimiter,
//!     client: Client,
//! }
//!
//! struct Post {
//!     // ..
//! }
//!
//! impl BlogClient {
//!     /// Get all posts from the service.
//!     pub async fn get_posts(&self) -> Result<Vec<Post>> {
//!         self.request("posts").await
//!     }
//!
//!     /// Perform a request against the service, limiting requests to abide by a rate limit.
//!     async fn request<T>(&self, path: &str) -> Result<T>
//!     where
//!         T: DeserializeOwned
//!     {
//!         // Before we start sending a request, we block on acquiring one token.
//!         self.limiter.acquire(1).await;
//!         self.client.request::<T>(path).await
//!     }
//! }
//! ```
//!
//! <br>
//!
//! ## Implementation details
//!
//! Each rate limiter has two acquisition modes. A fast path and a slow path.
//! The fast path is used if the desired number of tokens are readily available,
//! and simply involves decrementing the number of tokens available in the
//! shared pool.
//!
//! If the required number of tokens is not available, the task will be forced
//! to be suspended until the next refill interval. Here one of the acquiring
//! tasks will switch over to work as a *core*. This is known as *core
//! switching*.
//!
//! ```
//! use leaky_bucket::RateLimiter;
//! use tokio::time::Duration;
//!
//! # #[tokio::main(flavor="current_thread", start_paused=true)] async fn main() {
//! let limiter = RateLimiter::builder()
//!     .initial(10)
//!     .interval(Duration::from_millis(100))
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
//! > cargo run --example block_forever
//! > ```
//!
//! ```no_run
//! use std::future::Future;
//! use std::sync::Arc;
//! use std::task::Context;
//!
//! use leaky_bucket::RateLimiter;
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
//! tokens. This might cause more core switching, increasing the total work
//! needed. An unfair scheduler is expected to do a bit less work under
//! contention. But without fair scheduling some tasks might end up taking
//! longer to acquire than expected.
//!
//! Unfair rate limiters also have access to a fast path for acquiring tokens,
//! which might further improve throughput.
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
//! cargo run --example unfair_scheduling
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
//! [`acquire_one`]: https://docs.rs/leaky-bucket/1/leaky_bucket/struct.RateLimiter.html#method.acquire_one
//! [`acquire`]: https://docs.rs/leaky-bucket/1/leaky_bucket/struct.RateLimiter.html#method.acquire
//! [`Builder::fair`]: https://docs.rs/leaky-bucket/1/leaky_bucket/struct.Builder.html#method.fair
//! [`Mutex`]: https://docs.rs/tokio/1/tokio/sync/struct.Mutex.html
//! [`RateLimiter`]: https://docs.rs/leaky-bucket/1/leaky_bucket/struct.RateLimiter.html
//! [`time` feature]: https://docs.rs/tokio/1/tokio/#feature-flags
//! [`try_acquire`]: https://docs.rs/leaky-bucket/1/leaky_bucket/struct.RateLimiter.html#method.try_acquire
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
use core::mem::{self, ManuallyDrop};
use core::ops::{Deref, DerefMut};
use core::pin::Pin;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use core::task::{Context, Poll, Waker};

use alloc::sync::Arc;

use parking_lot::{Mutex, MutexGuard};
use pin_project_lite::pin_project;
use tokio::time::{self, Duration, Instant};

#[cfg(feature = "tracing")]
macro_rules! trace {
    ($($arg:tt)*) => {
        tracing::trace!($($arg)*)
    };
}

#[cfg(not(feature = "tracing"))]
macro_rules! trace {
    ($($arg:tt)*) => {};
}

mod linked_list;
use self::linked_list::{LinkedList, Node};

/// Default factor for how to calculate max refill value.
const DEFAULT_REFILL_MAX_FACTOR: usize = 10;

/// Interval to bump the shared mutex guard to allow other parts of the system
/// to make process. Processes which loop should use this number to determine
/// how many times it should loop before calling [Guard::bump].
///
/// If we do not respect this limit we might inadvertently end up starving other
/// tasks from making progress so that they can unblock.
const BUMP_LIMIT: usize = 16;

/// The maximum supported balance.
const MAX_BALANCE: usize = isize::MAX as usize;

/// Marker trait which indicates that a type represents a unique held critical section.
trait IsCritical {}
impl IsCritical for Critical {}
impl IsCritical for Guard<'_> {}

/// Linked task state.
struct Task {
    /// Remaining tokens that need to be satisfied.
    remaining: usize,
    /// If this node has been released or not. We make this an atomic to permit
    /// access to it without synchronization.
    complete: AtomicBool,
    /// The waker associated with the node.
    waker: Option<Waker>,
}

impl Task {
    /// Construct a new task state with the given permits remaining.
    const fn new() -> Self {
        Self {
            remaining: 0,
            complete: AtomicBool::new(false),
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

impl Deref for BorrowedRateLimiter<'_> {
    type Target = RateLimiter;

    #[inline]
    fn deref(&self) -> &RateLimiter {
        self.0
    }
}

struct Critical {
    /// Waiter list.
    waiters: LinkedList<Task>,
    /// The deadline for when more tokens can be be added.
    deadline: Instant,
}

#[repr(transparent)]
struct Guard<'a> {
    critical: MutexGuard<'a, Critical>,
}

impl Guard<'_> {
    #[inline]
    fn bump(this: &mut Guard<'_>) {
        MutexGuard::bump(&mut this.critical)
    }
}

impl Deref for Guard<'_> {
    type Target = Critical;

    #[inline]
    fn deref(&self) -> &Critical {
        &self.critical
    }
}

impl DerefMut for Guard<'_> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Critical {
        &mut self.critical
    }
}

impl Critical {
    #[inline]
    fn push_task_front(&mut self, task: &mut Node<Task>) {
        // SAFETY: We both have mutable access to the node being pushed, and
        // mutable access to the critical section through `self`. So we know we
        // have exclusive tampering rights to the waiter queue.
        unsafe {
            self.waiters.push_front(task.into());
        }
    }

    #[inline]
    fn push_task(&mut self, task: &mut Node<Task>) {
        // SAFETY: We both have mutable access to the node being pushed, and
        // mutable access to the critical section through `self`. So we know we
        // have exclusive tampering rights to the waiter queue.
        unsafe {
            self.waiters.push_back(task.into());
        }
    }

    #[inline]
    fn remove_task(&mut self, task: &mut Node<Task>) {
        // SAFETY: We both have mutable access to the node being pushed, and
        // mutable access to the critical section through `self`. So we know we
        // have exclusive tampering rights to the waiter queue.
        unsafe {
            self.waiters.remove(task.into());
        }
    }

    /// Release the current core. Beyond this point the current task may no
    /// longer interact exclusively with the core.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self), level = "trace"))]
    fn release(&mut self, state: &mut State<'_>) {
        trace!("releasing core");
        state.available = true;

        // Find another task that might take over as core. Once it has acquired
        // core status it will have to make sure it is no longer linked into the
        // wait queue.
        unsafe {
            if let Some(node) = self.waiters.front() {
                trace!(node = ?node, "waking next core");

                if let Some(ref waker) = node.as_ref().waker {
                    waker.wake_by_ref();
                }
            }
        }
    }
}

#[derive(Debug)]
struct State<'a> {
    /// Original state.
    state: usize,
    /// If the core is available or not.
    available: bool,
    /// The balance.
    balance: usize,
    /// The rate limiter the state is associated with.
    lim: &'a RateLimiter,
}

impl<'a> State<'a> {
    fn try_fast_path(mut self, permits: usize) -> bool {
        let mut attempts = 0;

        // Fast path where we just try to nab any available permit without
        // locking.
        //
        // We do have to race against anyone else grabbing permits here when
        // storing the state back.
        while self.balance >= permits {
            // Abandon fast path if we've tried too many times.
            if attempts == BUMP_LIMIT {
                break;
            }

            self.balance -= permits;

            if let Err(new_state) = self.try_save() {
                self = new_state;
                attempts += 1;
                continue;
            }

            return true;
        }

        false
    }

    /// Add tokens and release any pending tasks.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(skip(self, critical, f), level = "trace")
    )]
    #[inline]
    fn add_tokens<F, O>(&mut self, critical: &mut Guard<'_>, tokens: usize, f: F) -> O
    where
        F: FnOnce(&mut Guard<'_>, &mut State) -> O,
    {
        if tokens > 0 {
            debug_assert!(
                tokens <= MAX_BALANCE,
                "Additional tokens {} must be less than {}",
                tokens,
                MAX_BALANCE
            );

            self.balance = (self.balance + tokens).min(self.lim.max);
            drain_wait_queue(critical, self);
            let output = f(critical, self);
            return output;
        }

        f(critical, self)
    }

    #[inline]
    fn decode(state: usize, lim: &'a RateLimiter) -> Self {
        State {
            state,
            available: state & 1 == 1,
            balance: state >> 1,
            lim,
        }
    }

    #[inline]
    fn encode(&self) -> usize {
        (self.balance << 1) | usize::from(self.available)
    }

    /// Try to save the state, but only succeed if it hasn't been modified.
    #[inline]
    fn try_save(self) -> Result<(), Self> {
        let this = ManuallyDrop::new(self);

        match this.lim.state.compare_exchange(
            this.state,
            this.encode(),
            Ordering::Release,
            Ordering::Relaxed,
        ) {
            Ok(_) => Ok(()),
            Err(state) => Err(State::decode(state, this.lim)),
        }
    }
}

impl Drop for State<'_> {
    #[inline]
    fn drop(&mut self) {
        self.lim.state.store(self.encode(), Ordering::Release);
    }
}

/// A token-bucket rate limiter.
pub struct RateLimiter {
    /// Tokens to add every `per` duration.
    refill: usize,
    /// Interval in milliseconds to add tokens.
    interval: Duration,
    /// Max number of tokens associated with the rate limiter.
    max: usize,
    /// If the rate limiter is fair or not.
    fair: bool,
    /// The state of the rate limiter.
    state: AtomicUsize,
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
    /// use tokio::time::Duration;
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
    /// use leaky_bucket::RateLimiter;
    /// use tokio::time::Duration;
    ///
    /// let limiter = RateLimiter::builder()
    ///     .interval(Duration::from_millis(1000))
    ///     .build();
    ///
    /// assert_eq!(limiter.interval(), Duration::from_millis(1000));
    /// ```
    pub fn interval(&self) -> Duration {
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
        self.state.load(Ordering::Acquire) >> 1
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

    /// Forces a refill check without consuming any tokens.
    ///
    /// This method calculates and adds any tokens that should be added based on the elapsed time
    /// since the last refill, without consuming any tokens. This is useful for ensuring the token
    /// bucket is up-to-date before checking the balance or performing other operations.
    ///
    /// # Examples
    ///
    /// ```
    /// use leaky_bucket::RateLimiter;
    /// use tokio::time::Duration;
    ///
    /// # #[tokio::main(flavor="current_thread", start_paused=true)] async fn main() {
    /// let limiter = RateLimiter::builder()
    ///     .initial(10)
    ///     .refill(10)
    ///     .interval(Duration::from_millis(100))
    ///     .build();
    ///
    /// // Force a refill check without consuming tokens
    /// limiter.probe_refill();
    ///
    /// assert_eq!(limiter.balance(), 10);
    /// # }
    /// ```
    pub fn probe_refill(&self) {
        let mut critical = self.lock();
        let mut state = self.take();
        let now = Instant::now();

        if let Some((tokens, deadline)) = self.calculate_drain(critical.deadline, now) {
            state.balance = (state.balance + tokens).min(self.max);
            critical.deadline = deadline;
        }
    }

    /// Try to acquire the given number of permits, returning `true` if the
    /// given number of permits were successfully acquired.
    ///
    /// If the scheduler is fair, and there are pending tasks waiting to acquire
    /// tokens this method will return `false`.
    ///
    /// If zero permits are specified, this method returns `true`.
    ///
    /// # Examples
    ///
    /// ```
    /// use leaky_bucket::RateLimiter;
    /// use tokio::time;
    ///
    /// # #[tokio::main(flavor="current_thread", start_paused=true)] async fn main() {
    /// let limiter = RateLimiter::builder().refill(1).initial(1).build();
    ///
    /// assert!(limiter.try_acquire(1));
    /// assert!(!limiter.try_acquire(1));
    /// assert!(limiter.try_acquire(0));
    ///
    /// time::sleep(limiter.interval() * 2).await;
    ///
    /// assert!(limiter.try_acquire(1));
    /// assert!(limiter.try_acquire(1));
    /// assert!(!limiter.try_acquire(1));
    /// # }
    /// ```
    pub fn try_acquire(&self, permits: usize) -> bool {
        if self.try_fast_path(permits) {
            return true;
        }

        let mut critical = self.lock();

        // Reload the state while we are under the critical lock, this
        // ensures that the `available` flag is up-to-date since it is only
        // ever modified while holding the critical lock.
        let mut state = self.take();

        // The core is *not* available, which also implies that there are tasks
        // ahead which are busy.
        if !state.available {
            return false;
        }

        let now = Instant::now();

        // Here we try to assume core duty temporarily to see if we can
        // release a sufficient number of tokens to allow the current task
        // to proceed.
        if let Some((tokens, deadline)) = self.calculate_drain(critical.deadline, now) {
            state.balance = (state.balance + tokens).min(self.max);
            critical.deadline = deadline;
        }

        if state.balance >= permits {
            state.balance -= permits;
            return true;
        }

        false
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
    /// use std::future::Future;
    /// use std::pin::Pin;
    /// use std::sync::Arc;
    /// use std::task::{Context, Poll};
    ///
    /// use leaky_bucket::{AcquireOwned, RateLimiter};
    /// use pin_project::pin_project;
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

    /// Lock the critical section of the rate limiter and return the associated guard.
    fn lock(&self) -> Guard<'_> {
        Guard {
            critical: self.critical.lock(),
        }
    }

    /// Load the current state.
    fn load(&self) -> State<'_> {
        State::decode(self.state.load(Ordering::Acquire), self)
    }

    /// Take the current state, leaving the core state intact.
    fn take(&self) -> State<'_> {
        State::decode(self.state.swap(0, Ordering::Acquire), self)
    }

    /// Try to use fast path.
    fn try_fast_path(&self, permits: usize) -> bool {
        if permits == 0 {
            return true;
        }

        if self.fair {
            return false;
        }

        self.load().try_fast_path(permits)
    }

    /// Calculate refill amount. Returning a tuple of how much to fill and remaining
    /// duration to sleep until the next refill time if appropriate.
    ///
    /// The maximum number of additional tokens this method will ever return is
    /// limited to [`MAX_BALANCE`] to ensure that addition with an existing
    /// balance will never overflow.
    fn calculate_drain(&self, deadline: Instant, now: Instant) -> Option<(usize, Instant)> {
        if now < deadline {
            return None;
        }

        // Time elapsed in milliseconds since the last deadline.
        let millis = self.interval.as_millis();
        let since = now.saturating_duration_since(deadline).as_millis();

        let periods = usize::try_from(since / millis + 1).unwrap_or(usize::MAX);

        let tokens = periods
            .checked_mul(self.refill)
            .unwrap_or(MAX_BALANCE)
            .min(MAX_BALANCE);

        let rem = u64::try_from(since % millis).unwrap_or(u64::MAX);

        // Calculated time remaining until the next deadline.
        let deadline = now
            + self
                .interval
                .saturating_sub(time::Duration::from_millis(rem));

        Some((tokens, deadline))
    }
}

impl fmt::Debug for RateLimiter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RateLimiter")
            .field("refill", &self.refill)
            .field("interval", &self.interval)
            .field("max", &self.max)
            .field("fair", &self.fair)
            .finish_non_exhaustive()
    }
}

/// Refill the wait queue with the given number of tokens.
#[cfg_attr(feature = "tracing", tracing::instrument(skip_all, level = "trace"))]
fn drain_wait_queue(critical: &mut Guard<'_>, state: &mut State<'_>) {
    trace!(?state, "releasing waiters");

    let mut bump = 0;

    // SAFETY: we're holding the lock guard to all the waiters so we can be
    // sure that we have exclusive access to the wait queue.
    unsafe {
        while state.balance > 0 {
            let mut node = match critical.waiters.pop_back() {
                Some(node) => node,
                None => break,
            };

            let n = node.as_mut();
            n.fill(&mut state.balance);

            trace! {
                ?state,
                remaining = n.remaining,
                "filled node",
            };

            if !n.is_completed() {
                critical.waiters.push_back(node);
                break;
            }

            n.complete.store(true, Ordering::Release);

            if let Some(waker) = n.waker.take() {
                waker.wake();
            }

            bump += 1;

            if bump == BUMP_LIMIT {
                Guard::bump(critical);
                bump = 0;
            }
        }
    }
}

// SAFETY: All the internals of acquire is thread safe and correctly
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
    interval: Duration,
    /// If the rate limiter is fair or not.
    fair: bool,
}

impl Builder {
    /// Configure the max number of tokens to use.
    ///
    /// If unspecified, this will default to be 10 times the [`refill`] or the
    /// [`initial`] value, whichever is largest.
    ///
    /// The maximum supported balance is limited to [`isize::MAX`].
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
    /// This is 100ms by default.
    ///
    /// # Panics
    ///
    /// This panics if the provided interval does not fit within the millisecond
    /// bounds of a [usize] or is zero.
    ///
    /// ```should_panic
    /// use leaky_bucket::RateLimiter;
    /// use tokio::time::Duration;
    ///
    /// let limiter = RateLimiter::builder()
    ///     .interval(Duration::from_secs(u64::MAX))
    ///     .build();
    /// ```
    ///
    /// ```should_panic
    /// use leaky_bucket::RateLimiter;
    /// use tokio::time::Duration;
    ///
    /// let limiter = RateLimiter::builder()
    ///     .interval(Duration::from_millis(0))
    ///     .build();
    /// ```
    ///
    /// # Examples
    ///
    /// ```
    /// use leaky_bucket::RateLimiter;
    /// use tokio::time::Duration;
    ///
    /// let limiter = RateLimiter::builder()
    ///     .interval(Duration::from_millis(100))
    ///     .build();
    /// ```
    ///
    /// [`refill`]: Builder::refill
    pub fn interval(&mut self, interval: Duration) -> &mut Self {
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

    /// Configure the rate limiter to be fair.
    ///
    /// Fairness is enabled by deafult.
    ///
    /// Fairness ensures that tasks make progress in the order that they acquire
    /// even when the rate limiter is under contention. An unfair scheduler
    /// might have a higher total throughput.
    ///
    /// Fair scheduling also affects the behavior of
    /// [`RateLimiter::try_acquire`] which will return `false` if there are any
    /// pending tasks since they should be given priority.
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
    /// use tokio::time::Duration;
    ///
    /// let limiter = RateLimiter::builder()
    ///     .refill(100)
    ///     .interval(Duration::from_millis(200))
    ///     .max(10_000)
    ///     .build();
    /// ```
    pub fn build(&self) -> RateLimiter {
        let deadline = Instant::now() + self.interval;

        let initial = self.initial.min(MAX_BALANCE);
        let refill = self.refill.min(MAX_BALANCE);

        let max = match self.max {
            Some(max) => max.min(MAX_BALANCE),
            None => refill
                .max(initial)
                .saturating_mul(DEFAULT_REFILL_MAX_FACTOR)
                .min(MAX_BALANCE),
        };

        let initial = initial.min(max);

        RateLimiter {
            refill,
            interval: self.interval,
            max,
            fair: self.fair,
            state: AtomicUsize::new(initial << 1 | 1),
            critical: Mutex::new(Critical {
                waiters: LinkedList::new(),
                deadline,
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
            interval: Duration::from_millis(100),
            fair: true,
        }
    }
}

/// The state of an acquire operation.
#[derive(Debug, Clone, Copy)]
enum AcquireFutState {
    /// Initial unconfigured state.
    Initial,
    /// The acquire is waiting to be released by the core.
    Waiting,
    /// The operation is completed.
    Complete,
    /// The task is currently the core.
    Core,
}

/// Inner state and methods of the acquire.
#[repr(transparent)]
struct AcquireFutInner {
    /// Aliased task state.
    node: UnsafeCell<Node<Task>>,
}

impl AcquireFutInner {
    const fn new() -> AcquireFutInner {
        AcquireFutInner {
            node: UnsafeCell::new(Node::new(Task::new())),
        }
    }

    /// Access the completion flag.
    pub fn complete(&self) -> &AtomicBool {
        // SAFETY: This is always safe to access since it's atomic.
        unsafe { &*ptr::addr_of!((*self.node.get()).complete) }
    }

    /// Get the underlying task mutably.
    ///
    /// We prove that the caller does indeed have mutable access to the node by
    /// passing in a mutable reference to the critical section.
    #[inline]
    pub fn get_task<'crit, C>(
        self: Pin<&'crit mut Self>,
        critical: &'crit mut C,
    ) -> (&'crit mut C, &'crit mut Node<Task>)
    where
        C: IsCritical,
    {
        // SAFETY: Caller has exclusive access to the critical section, since
        // it's passed in as a mutable argument. We can also ensure that none of
        // the borrows outlive the provided closure.
        unsafe { (critical, &mut *self.node.get()) }
    }

    /// Update the waiting state for this acquisition task. This might require
    /// that we update the associated waker.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(skip(self, critical, waker), level = "trace")
    )]
    fn update(self: Pin<&mut Self>, critical: &mut Guard<'_>, waker: &Waker) {
        let (critical, task) = self.get_task(critical);

        if !task.is_linked() {
            critical.push_task_front(task);
        }

        let new_waker = match task.waker {
            None => true,
            Some(ref w) => !w.will_wake(waker),
        };

        if new_waker {
            trace!("updating waker");
            task.waker = Some(waker.clone());
        }
    }

    /// Ensure that the current core task is correctly linked up if needed.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(skip(self, critical, lim), level = "trace")
    )]
    fn link_core(self: Pin<&mut Self>, critical: &mut Critical, lim: &RateLimiter) {
        let (critical, task) = self.get_task(critical);

        match (lim.fair, task.is_linked()) {
            (true, false) => {
                // Fair scheduling needs to ensure that the core is part of the wait
                // queue, and will be woken up in-order with other tasks.
                critical.push_task(task);
            }
            (false, true) => {
                // Unfair scheduling will not wake the core in order, so
                // don't bother having it linked.
                critical.remove_task(task);
            }
            _ => {}
        }
    }

    /// Release any remaining tokens which are associated with this particular task.
    fn release_remaining(
        self: Pin<&mut Self>,
        critical: &mut Guard<'_>,
        state: &mut State<'_>,
        permits: usize,
    ) {
        let (critical, task) = self.get_task(critical);

        if task.is_linked() {
            critical.remove_task(task);
        }

        // Hand back permits which we've acquired so far.
        let release = permits.saturating_sub(task.remaining);
        state.add_tokens(critical, release, |_, _| ());
    }

    /// Drain the given number of tokens through the core. Returns `true` if the
    /// core has been completed.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(skip(self, critical), level = "trace")
    )]
    fn drain_core(
        self: Pin<&mut Self>,
        critical: &mut Guard<'_>,
        state: &mut State<'_>,
        tokens: usize,
    ) -> bool {
        let completed = state.add_tokens(critical, tokens, |critical, state| {
            let (_, task) = self.get_task(critical);

            // If the limiter is not fair, we need to in addition to draining
            // remaining tokens from linked nodes, drain it from ourselves. We
            // fill the current holder of the core last (self). To ensure that
            // it stays around for as long as possible.
            if !state.lim.fair {
                task.fill(&mut state.balance);
            }

            task.is_completed()
        });

        if completed {
            // Everything was drained, including the current core (if
            // appropriate). So we can release it now.
            critical.release(state);
        }

        completed
    }

    /// Assume the current core and calculate how long we must sleep for in
    /// order to do it.
    ///
    /// # Safety
    ///
    /// This might link the current task into the task queue, so the caller must
    /// ensure that it is pinned.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(skip(self, critical), level = "trace")
    )]
    fn assume_core(
        mut self: Pin<&mut Self>,
        critical: &mut Guard<'_>,
        state: &mut State<'_>,
        now: Instant,
    ) -> bool {
        self.as_mut().link_core(critical, state.lim);

        let (tokens, deadline) = match state.lim.calculate_drain(critical.deadline, now) {
            Some(tokens) => tokens,
            None => return true,
        };

        // It is appropriate to update the deadline.
        critical.deadline = deadline;
        !self.drain_core(critical, state, tokens)
    }
}

pin_project! {
    /// The future associated with acquiring permits from a rate limiter using
    /// [`RateLimiter::acquire`].
    #[project(!Unpin)]
    pub struct Acquire<'a> {
        #[pin]
        inner: AcquireFut<BorrowedRateLimiter<'a>>,
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

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().inner.poll(cx)
    }
}

pin_project! {
    /// The future associated with acquiring permits from a rate limiter using
    /// [`RateLimiter::acquire_owned`].
    #[project(!Unpin)]
    pub struct AcquireOwned {
        #[pin]
        inner: AcquireFut<Arc<RateLimiter>>,
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

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().inner.poll(cx)
    }
}

pin_project! {
    #[project(!Unpin)]
    #[project = AcquireFutProj]
    struct AcquireFut<T>
    where
        T: Deref<Target = RateLimiter>,
    {
        lim: T,
        permits: usize,
        state: AcquireFutState,
        #[pin]
        sleep: Option<time::Sleep>,
        #[pin]
        inner: AcquireFutInner,
    }

    impl<T> PinnedDrop for AcquireFut<T>
    where
        T: Deref<Target = RateLimiter>,
    {
        fn drop(this: Pin<&mut Self>) {
            let AcquireFutProj { lim, permits, state, inner, .. } = this.project();

            let is_core = match *state {
                AcquireFutState::Waiting => false,
                AcquireFutState::Core { .. } => true,
                _ => return,
            };

            let mut critical = lim.lock();
            let mut s = lim.take();
            inner.release_remaining(&mut critical, &mut s, *permits);

            if is_core {
                critical.release(&mut s);
            }

            *state = AcquireFutState::Complete;
        }
    }
}

impl<T> AcquireFut<T>
where
    T: Deref<Target = RateLimiter>,
{
    #[inline]
    const fn new(lim: T, permits: usize) -> Self {
        Self {
            lim,
            permits,
            state: AcquireFutState::Initial,
            sleep: None,
            inner: AcquireFutInner::new(),
        }
    }

    fn is_core(&self) -> bool {
        matches!(&self.state, AcquireFutState::Core { .. })
    }
}

// SAFETY: All the internals of acquire is thread safe and correctly
// synchronized. The embedded waiter queue doesn't have anything inherently
// unsafe in it.
unsafe impl<T> Send for AcquireFut<T> where T: Send + Deref<Target = RateLimiter> {}
unsafe impl<T> Sync for AcquireFut<T> where T: Sync + Deref<Target = RateLimiter> {}

impl<T> Future for AcquireFut<T>
where
    T: Deref<Target = RateLimiter>,
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let AcquireFutProj {
            lim,
            permits,
            state,
            mut sleep,
            inner: mut internal,
            ..
        } = self.project();

        // Hold onto the critical lock for core operations, but only acquire it
        // when strictly necessary.
        let mut critical;

        // Shared state.
        //
        // Once we are holding onto the critical lock, we take the entire state
        // to ensure that any fast-past negotiators do not observe any available
        // permits while potential core work is ongoing.
        let mut s;

        // Hold onto any call to `Instant::now` which we might perform, so we
        // don't have to get the current time multiple times.
        let outer_now;

        match *state {
            AcquireFutState::Complete => {
                return Poll::Ready(());
            }
            AcquireFutState::Initial => {
                // If the rate limiter is not fair, try to oppurtunistically
                // just acquire a permit through the known atomic state.
                //
                // This is known as the fast path, but requires acquire to raise
                // against other tasks when storing the state back.
                if lim.try_fast_path(*permits) {
                    *state = AcquireFutState::Complete;
                    return Poll::Ready(());
                }

                critical = lim.lock();
                s = lim.take();

                let now = Instant::now();

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
                let tokens =
                    if let Some((tokens, deadline)) = lim.calculate_drain(critical.deadline, now) {
                        trace!(tokens, "inline drain");
                        // We pre-emptively update the deadline of the core
                        // since it might bump, and we don't want other
                        // processes to observe that the deadline has been
                        // reached.
                        critical.deadline = deadline;
                        tokens
                    } else {
                        0
                    };

                let completed = s.add_tokens(&mut critical, tokens, |critical, s| {
                    let (_, task) = internal.as_mut().get_task(critical);
                    task.remaining = *permits;
                    task.fill(&mut s.balance);
                    task.is_completed()
                });

                if completed {
                    *state = AcquireFutState::Complete;
                    return Poll::Ready(());
                }

                // Try to take over as core. If we're unsuccessful we just
                // ensure that we're linked into the wait queue.
                if !mem::take(&mut s.available) {
                    internal.as_mut().update(&mut critical, cx.waker());
                    *state = AcquireFutState::Waiting;
                    return Poll::Pending;
                }

                // SAFETY: This is done in a pinned section, so we know that
                // the linked section stays alive for the duration of this
                // future due to pinning guarantees.
                internal.as_mut().link_core(&mut critical, lim);
                Guard::bump(&mut critical);
                *state = AcquireFutState::Core;
                outer_now = Some(now);
            }
            AcquireFutState::Waiting => {
                // If we are complete, then return as ready.
                //
                // This field is atomic, so we can safely read it under shared
                // access and do not require a lock.
                if internal.complete().load(Ordering::Acquire) {
                    *state = AcquireFutState::Complete;
                    return Poll::Ready(());
                }

                // Note: we need to operate under this lock to ensure that
                // the core acquired here (or elsewhere) observes that the
                // current task has been linked up.
                critical = lim.lock();
                s = lim.take();

                // Try to take over as core. If we're unsuccessful we
                // just ensure that we're linked into the wait queue.
                if !mem::take(&mut s.available) {
                    internal.update(&mut critical, cx.waker());
                    return Poll::Pending;
                }

                let now = Instant::now();

                // This is done in a pinned section, so we know that the linked
                // section stays alive for the duration of this future due to
                // pinning guarantees.
                if !internal.as_mut().assume_core(&mut critical, &mut s, now) {
                    // Marks as completed.
                    *state = AcquireFutState::Complete;
                    return Poll::Ready(());
                }

                Guard::bump(&mut critical);
                *state = AcquireFutState::Core;
                outer_now = Some(now);
            }
            AcquireFutState::Core => {
                critical = lim.lock();
                s = lim.take();
                outer_now = None;
            }
        }

        trace!(until = ?critical.deadline, "taking over core and sleeping");

        let mut sleep = match sleep.as_mut().as_pin_mut() {
            Some(mut sleep) => {
                if sleep.deadline() != critical.deadline {
                    sleep.as_mut().reset(critical.deadline);
                }

                sleep
            }
            None => {
                sleep.set(Some(time::sleep_until(critical.deadline)));
                sleep.as_mut().as_pin_mut().unwrap()
            }
        };

        if sleep.as_mut().poll(cx).is_pending() {
            return Poll::Pending;
        }

        critical.deadline = outer_now.unwrap_or_else(Instant::now) + lim.interval;

        if internal.drain_core(&mut critical, &mut s, lim.refill) {
            *state = AcquireFutState::Complete;
            return Poll::Ready(());
        }

        cx.waker().wake_by_ref();
        Poll::Pending
    }
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
