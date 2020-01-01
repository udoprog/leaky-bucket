#![recursion_limit = "256"]
#![deny(missing_docs)]
//! # tokio-based leaky-bucket rate limiter
//!
//! This implements a leaky bucket from which you can acquire tokens.
//!
//! If the tokens are already available, the acquisition will be instant (fast path) and the
//! acquired number of tokens will be added to the bucket.
//!
//! If the bucket overflows (i.e. goes over max), the task that tried to acquire the tokens will
//! be suspended until the required number of tokens has been added.
//!
//! ## Example
//!
//! If the project is built with the `static` feature (default), you can use
//! `LeakyBucket` directly as long as you are inside a tokio runtime, like so:
//!
//! ```no_run
//! use leaky_bucket::LeakyBucket;
//! use std::{error::Error, time::Duration};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn Error>> {
//!     let rate_limiter = LeakyBucket::builder()
//!         .max(5)
//!         .tokens(5)
//!         .build()?;
//!
//!     println!("Waiting for permit...");
//!     // should take about 5 seconds to acquire.
//!     rate_limiter.acquire(10).await?;
//!     println!("I made it!");
//!     Ok(())
//! }
//! ```
//!
//! ## Example using explicit coordinator
//!
//! ```no_run
//! use leaky_bucket::LeakyBuckets;
//! use std::{error::Error, time::Duration};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn Error>> {
//!     let mut buckets = LeakyBuckets::new();
//!     let coordinator = buckets.coordinate()?;
//!     // spawn the coordinate thread to refill the rate limiter.
//!     tokio::spawn(async move { coordinator.await.expect("coordinate thread errored") });
//!
//!     let rate_limiter = buckets
//!         .rate_limiter()
//!         .max(5)
//!         .tokens(5)
//!         .build()?;
//!
//!     println!("Waiting for permit...");
//!     // should take about 5 seconds to acquire.
//!     rate_limiter.acquire(10).await?;
//!     println!("I made it!");
//!     Ok(())
//! }
//! ```

use futures_channel::mpsc;
use futures_util::{
    ready, select,
    stream::{FuturesUnordered, StreamExt as _},
};
use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll, Waker},
    time::Duration,
};
use thiserror::Error;

#[cfg(feature = "static")]
lazy_static::lazy_static! {
    static ref LEAKY_BUCKETS: LeakyBuckets = {
        let mut buckets = LeakyBuckets::new();
        let coordinator = buckets.coordinate().expect("no other running coordinator");
        tokio::spawn(async move { coordinator.await.expect("coordinate thread errored") });
        buckets
    };
}

/// Error type for the rate limiter.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// The bucket has already been started.
    #[error("Coordinator already started")]
    AlreadyStarted,
    /// There was an issue enqueueing a task.
    #[error("Failed to send task to coordinator: {0}")]
    TaskSendError(#[source] mpsc::SendError),
    /// Failed to queue up new task.
    #[error("Failed to queue up new task coordinator")]
    NewTaskError,
    /// Tried to acquire more tokens than what is possible.
    #[error("Acquiring tokens would cause an overflow")]
    TokenOverflow,
}

struct NewTask {
    inner: Arc<Inner>,
    task_rx: mpsc::Receiver<Task>,
}

struct LeakyBucketsInner {
    new_task_tx: mpsc::UnboundedSender<NewTask>,
}

/// Coordinator for rate limiters. Is used to create new rate limiters as needed.
pub struct LeakyBuckets {
    inner: Arc<LeakyBucketsInner>,
    new_task_rx: Option<mpsc::UnboundedReceiver<NewTask>>,
}

impl Default for LeakyBuckets {
    fn default() -> Self {
        Self::new()
    }
}

impl LeakyBuckets {
    /// Construct a new coordinator for rate limiters.
    pub fn new() -> Self {
        let (new_task_tx, new_task_rx) = mpsc::unbounded();

        let inner = Arc::new(LeakyBucketsInner { new_task_tx });

        LeakyBuckets {
            inner,
            new_task_rx: Some(new_task_rx),
        }
    }

    /// Run the coordinator.
    pub fn coordinate(
        &mut self,
    ) -> Result<impl Future<Output = Result<(), Error>> + 'static, Error> {
        let mut new_task_rx = match self.new_task_rx.take() {
            Some(new_task_rx) => new_task_rx,
            None => return Err(Error::AlreadyStarted),
        };

        Ok(async move {
            let mut futures = FuturesUnordered::new();

            loop {
                while futures.is_empty() {
                    select! {
                        NewTask { inner, task_rx } = new_task_rx.select_next_some() => {
                            futures.push(inner.coordinate(task_rx));
                        }
                    }
                }

                select! {
                    _ = futures.next() => {
                        panic!("coordinator task exited unexpectedly");
                    }
                    NewTask { inner, task_rx } = new_task_rx.select_next_some() => {
                        futures.push(inner.coordinate(task_rx));
                    }
                }
            }
        })
    }

    /// Construct a new rate limiter.
    pub fn rate_limiter(&self) -> Builder<'_> {
        Builder {
            new_task_tx: &self.inner.new_task_tx,
            tokens: None,
            max: None,
            refill_interval: None,
            refill_amount: None,
        }
    }
}

/// Builder for a leaky bucket.
pub struct Builder<'a> {
    new_task_tx: &'a mpsc::UnboundedSender<NewTask>,
    tokens: Option<usize>,
    max: Option<usize>,
    refill_interval: Option<Duration>,
    refill_amount: Option<usize>,
}

impl Builder<'_> {
    /// Set the max value for the builder.
    #[inline(always)]
    pub fn max(mut self, max: usize) -> Self {
        self.max = Some(max);
        self
    }

    /// The number of tokens that the bucket should start with.
    ///
    /// If set to larger than `max` at build time, will only saturate to max.
    #[inline(always)]
    pub fn tokens(mut self, tokens: usize) -> Self {
        self.tokens = Some(tokens);
        self
    }

    /// Set the max value for the builder.
    #[inline(always)]
    pub fn refill_interval(mut self, refill_interval: Duration) -> Self {
        self.refill_interval = Some(refill_interval);
        self
    }

    /// Set the refill amount to use.
    #[inline(always)]
    pub fn refill_amount(mut self, refill_amount: usize) -> Self {
        self.refill_amount = Some(refill_amount);
        self
    }

    /// Construct a new leaky bucket.
    pub fn build(self) -> Result<LeakyBucket, Error> {
        const DEFAULT_MAX: usize = 120;
        const DEFAULT_TOKENS: usize = 0;
        const DEFAULT_REFILL_INTERVAL: Duration = Duration::from_secs(1);
        const DEFAULT_REFILL_AMOUNT: usize = 1;

        let max = self.max.unwrap_or(DEFAULT_MAX);
        let tokens = max.saturating_sub(self.tokens.unwrap_or(DEFAULT_TOKENS));
        let refill_interval = self.refill_interval.unwrap_or(DEFAULT_REFILL_INTERVAL);
        let refill_amount = self.refill_amount.unwrap_or(DEFAULT_REFILL_AMOUNT);

        let tokens = AtomicUsize::new(tokens);

        let (task_tx, task_rx) = mpsc::channel(1);

        let inner = Arc::new(Inner {
            tokens,
            max,
            refill_interval,
            refill_amount,
            task_tx,
        });

        self.new_task_tx
            .clone()
            .unbounded_send(NewTask {
                inner: inner.clone(),
                task_rx,
            })
            .map_err(|_| Error::NewTaskError)?;

        Ok(LeakyBucket { inner })
    }
}

/// A single queued task waiting to be woken up.
struct Task {
    /// Amount required to wake up the given task.
    required: usize,
    /// Waker to call.
    waker: Waker,
    /// Indicates if the task is completed.
    complete: Arc<AtomicBool>,
}

struct Inner {
    /// Current number of tokens that have been acquired.
    ///
    /// This has an inverted relationship to `max`.
    /// The current number of active tokens can be found be subtracting the
    /// the number of acquire tokens (this), from max.
    ///
    /// It is structure like this since adding to an atomic growing number to
    /// acquire tokens is much easier than trying to subtract and truncate it
    /// towards zero.
    ///
    /// This means that there is a risk the value will overflow.
    tokens: AtomicUsize,
    /// Max number of tokens.
    max: usize,
    /// Period to use when refilling.
    refill_interval: Duration,
    /// Amount to add when refilling.
    refill_amount: usize,
    /// Sender for emitting queued tasks.
    task_tx: mpsc::Sender<Task>,
}

impl Inner {
    /// Coordinate tasks.
    async fn coordinate(self: Arc<Inner>, mut task_rx: mpsc::Receiver<Task>) -> Result<(), Error> {
        // The queue of tasks to process.
        let mut tasks = VecDeque::new();
        let first = std::time::Instant::now()
            .checked_add(self.refill_interval)
            .unwrap_or_else(std::time::Instant::now);
        // The interval at which we refill tokens.
        let mut interval = tokio::time::interval_at(first.into(), self.refill_interval).fuse();
        // The current number of tokens accumulated locally.
        // This will increase until we have enough to satisfy the next waking task.
        let mut amount = 0;
        let mut current = None;

        'outer: loop {
            select! {
                waker = task_rx.select_next_some() => {
                    tasks.push_back(waker);
                },
                _ = interval.select_next_some() => {
                    amount += self.refill_amount;

                    let mut task = match current.take().or_else(|| tasks.pop_front()) {
                        Some(task) => task,
                        None => {
                            // Nothing to wake up, subtract the number of
                            // tokens immediately allowing future acquires to
                            // enter the fast path.
                            self.balance_tokens(amount);
                            amount = 0;
                            continue;
                        }
                    };

                    while amount >= task.required {
                        // We have enough tokens to wake up the next task.
                        // Subtract it from the current amount and notify the task to wake up.
                        amount -= task.required;
                        task.complete.store(true, Ordering::Release);
                        task.waker.wake();

                        task = match tasks.pop_front() {
                            Some(task) => task,
                            None => {
                                self.balance_tokens(amount);
                                amount = 0;
                                continue 'outer;
                            },
                        };
                    }

                    current = Some(task);
                },
            }
        }
    }

    /// Subtract the given amount of tokens, allowing them to be used by the fast path acquire.
    fn balance_tokens(&self, amount: usize) {
        if amount == 0 {
            return;
        }

        let mut current = self.tokens.load(Ordering::Acquire);

        while current > 0 {
            let new = current.saturating_sub(amount);

            match self.tokens.compare_exchange_weak(
                current,
                new,
                Ordering::SeqCst,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(x) => current = x,
            }
        }
    }
}

/// The leaky bucket.
#[derive(Clone)]
pub struct LeakyBucket {
    inner: Arc<Inner>,
}

impl LeakyBucket {
    /// Construct a new leaky bucket through a builder.
    #[cfg(feature = "static")]
    pub fn builder() -> Builder<'static> {
        LEAKY_BUCKETS.rate_limiter()
    }

    /// Query how many tokens are available.
    ///
    /// This is just a best-effort estimate, calling this to ensure that there
    /// are enough tokens available to avoid blocking does not guarantee that
    /// the acquire operation won't block.
    ///
    /// Tokens is always reported as less than or equal to `max`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use leaky_bucket::LeakyBucket;
    /// use std::{error::Error, time::Duration};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn Error>> {
    ///     let rate_limiter = LeakyBucket::builder()
    ///         .max(5)
    ///         .tokens(0)
    ///         .build()?;
    ///
    ///     assert_eq!(0, rate_limiter.tokens());
    ///
    ///     println!("Waiting for permit...");
    ///     // should take about 5 seconds to acquire.
    ///     rate_limiter.acquire_one().await?;
    ///     println!("I made it!");
    ///
    ///     assert!(rate_limiter.tokens() >= 0 && rate_limiter.tokens() <= rate_limiter.max());
    ///     Ok(())
    /// }
    /// ```
    pub fn tokens(&self) -> usize {
        let tokens = self.inner.tokens.load(Ordering::Acquire);
        self.inner.max.saturating_sub(tokens)
    }

    /// Get the max number of tokens this rate limiter is configured for.
    #[inline]
    pub fn max(&self) -> usize {
        self.inner.max
    }

    /// Acquire a single token.
    ///
    /// This is identical to [`acquire`] with an argument of `1`.
    ///
    /// [`acquire`]: LeakyBucket::acquire
    ///
    /// # Example
    ///
    /// ```rust
    /// use leaky_bucket::LeakyBucket;
    /// use std::{error::Error, time::Duration};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn Error>> {
    ///     let rate_limiter = LeakyBucket::builder()
    ///         .max(5)
    ///         .tokens(0)
    ///         .build()?;
    ///
    ///     assert_eq!(0, rate_limiter.tokens());
    ///
    ///     println!("Waiting for permit...");
    ///     // should take about 5 seconds to acquire.
    ///     rate_limiter.acquire_one().await?;
    ///     println!("I made it!");
    ///
    ///     assert_eq!(0, rate_limiter.tokens());
    ///     Ok(())
    /// }
    /// ```
    #[inline]
    pub fn acquire_one(&self) -> Acquire<'_> {
        self.acquire(1)
    }

    /// Acquire the given `amount` of tokens.
    ///
    /// Note that you _are_ allowed to acquire more tokens than the current
    /// `max`, but the acquire will have to suspend the task until enough
    /// tokens has built up to satisfy the acquire request.
    ///
    /// # Example
    ///
    /// ```rust
    /// use leaky_bucket::LeakyBucket;
    /// use std::{error::Error, time::Duration};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn Error>> {
    ///     let rate_limiter = LeakyBucket::builder()
    ///         .max(5)
    ///         .tokens(5)
    ///         .build()?;
    ///
    ///     assert_eq!(5, rate_limiter.tokens());
    ///
    ///     println!("Waiting for permit...");
    ///     // should take about 5 seconds to acquire.
    ///     rate_limiter.acquire(10).await?;
    ///     println!("I made it!");
    ///
    ///     assert_eq!(0, rate_limiter.tokens());
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// The returned future will fail with an [`Error::TokenOverflow`] if the
    /// tracked number of tokens attempts to overflow the internal token
    /// counter, which is an `usize`.
    ///
    /// For this reason you should prefer to use smaller values in the amount
    /// acquired.
    ///
    /// [`Error::TokenOverflow`]: self::Error::TokenOverflow
    pub fn acquire(&self, amount: usize) -> Acquire<'_> {
        Acquire {
            inner: &self.inner,
            amount,
            in_progress: None,
            queued: None,
        }
    }
}

/// Future associated with acquiring a single token.
pub struct Acquire<'a> {
    /// Reference to leaky bucket internals.
    inner: &'a Inner,
    /// The amount of tokens to acquire.
    amount: usize,
    // An owned Sender, used for sending the `Task` struct over.
    // Only created lazily when necessary.
    in_progress: Option<(mpsc::Sender<Task>, usize)>,
    // State of a queued task, or if it was woken up.
    // If defined, this needs to be checked when the task is woken to test if
    // the current acquire is awake or not.
    queued: Option<Arc<AtomicBool>>,
}

impl Future for Acquire<'_> {
    type Output = Result<(), Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Self {
            ref inner,
            ref mut in_progress,
            ref mut queued,
            amount,
            ..
        } = *self;

        // Test if it has been woken up by the coordinator.
        // If that is the case, complete should be `true`.
        //
        // Otherwise we are still pending.
        if let Some(complete) = queued {
            return if complete.load(Ordering::Acquire) {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            };
        }

        loop {
            // handle send-in-progress.
            if let Some((ref mut tx, required)) = *in_progress {
                // queue up thread to be released once more tokens are available.
                if let Err(e) = ready!(tx.poll_ready(cx)) {
                    return Poll::Ready(Err(Error::TaskSendError(e)));
                }

                let complete = Arc::new(AtomicBool::new(false));
                *queued = Some(complete.clone());

                let task = Task {
                    required,
                    waker: cx.waker().clone(),
                    complete,
                };

                if let Err(e) = tx.start_send(task) {
                    return Poll::Ready(Err(Error::TaskSendError(e)));
                }

                return Poll::Pending;
            }

            let mut required = amount;

            let mut current = inner.tokens.load(Ordering::Acquire);
            let mut new;

            loop {
                new = match current.checked_add(required) {
                    Some(new) => new,
                    None => {
                        return Poll::Ready(Err(Error::TokenOverflow));
                    }
                };

                match inner.tokens.compare_exchange_weak(
                    current,
                    new,
                    Ordering::SeqCst,
                    Ordering::Acquire,
                ) {
                    Ok(_) => break,
                    Err(x) => {
                        current = x;
                    }
                }
            }

            // fast path, we successfully acquired the number of tokens needed to proceed.
            if new < inner.max {
                return Poll::Ready(Ok(()));
            }

            // Slow path:
            //
            // Subtract the number of tokens already consumed from required and
            // enqueue a task to be awoken when the given number of tokens is
            // available.
            if current < inner.max {
                required -= inner.max - current;
            }

            // hand back the sender for the next time.
            *in_progress = Some((inner.task_tx.clone(), required));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, LeakyBuckets};
    use futures::prelude::*;
    use std::time::{Duration, Instant};
    use tokio::time;

    #[tokio::test]
    async fn test_leaky_bucket() {
        let interval = Duration::from_millis(20);

        let mut buckets = LeakyBuckets::new();

        let leaky = buckets
            .rate_limiter()
            .tokens(0)
            .max(10)
            .refill_amount(10)
            .refill_interval(interval)
            .build()
            .expect("build rate limiter");

        let mut wakeups = 0u32;
        let mut duration = None;

        let test = async {
            let start = Instant::now();
            leaky.acquire(10).await?;
            wakeups += 1;
            leaky.acquire(10).await?;
            wakeups += 1;
            leaky.acquire(10).await?;
            wakeups += 1;
            duration = Some(Instant::now().duration_since(start));

            Ok::<_, Error>(())
        };

        futures::future::select(test.boxed(), buckets.coordinate().unwrap().boxed()).await;

        assert_eq!(3, wakeups);
        assert!(duration.expect("expected measured duration") > interval * 2);
    }

    #[tokio::test]
    async fn test_concurrent_rate_limited() {
        let interval = Duration::from_millis(20);

        let mut buckets = LeakyBuckets::new();

        let leaky = buckets
            .rate_limiter()
            .tokens(0)
            .max(10)
            .refill_amount(1)
            .refill_interval(interval)
            .build()
            .expect("build rate limiter");

        let mut one_wakeups = 0;

        let one = async {
            loop {
                leaky.acquire(1).await?;
                one_wakeups += 1;
            }

            #[allow(unreachable_code)]
            Ok::<_, Error>(())
        };

        let mut two_wakeups = 0u32;

        let two = async {
            loop {
                leaky.acquire(1).await?;
                two_wakeups += 1;
            }

            #[allow(unreachable_code)]
            Ok::<_, Error>(())
        };

        let delay = time::delay_for(Duration::from_millis(200));

        let task = future::select(one.boxed(), two.boxed());
        let task = future::select(task, delay);

        future::select(task, buckets.coordinate().unwrap().boxed()).await;

        let total = one_wakeups + two_wakeups;

        assert!(total > 5 && total < 15);
    }
}
