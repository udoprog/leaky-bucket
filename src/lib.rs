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
//!         .max(100)
//!         .refill_interval(Duration::from_secs(10))
//!         .refill_amount(100)
//!         .build()?;
//!
//!     println!("Waiting for permit...");
//!     // should take about ten seconds to get a permit.
//!     rate_limiter.acquire(100).await?;
//!     println!("I made it!");
//!
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
//!         .max(100)
//!         .refill_interval(Duration::from_secs(10))
//!         .refill_amount(100)
//!         .build()?;

//!     println!("Waiting for permit...");
//!     // should take about ten seconds to get a permit.
//!     rate_limiter.acquire(100).await?;
//!     println!("I made it!");
//!
//!     Ok(())
//! }
//! ```

use futures_channel::mpsc::{self, Receiver, Sender, UnboundedReceiver, UnboundedSender};
use futures_util::{
    ready, select,
    stream::{FuturesUnordered, StreamExt as _},
};
use std::{
    collections::VecDeque,
    error, fmt,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll, Waker},
    time::Duration,
};

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
#[derive(Debug)]
pub enum Error {
    /// The bucket has already been started.
    AlreadyStarted,
    /// There was an issue enqueueing a task.
    TaskSendError(mpsc::SendError),
    /// Failed to queue up new task.
    NewTaskError,
}

impl fmt::Display for Error {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        use self::Error::*;

        match self {
            AlreadyStarted => write!(fmt, "already started"),
            TaskSendError(e) => write!(fmt, "failed to send task to coordinator: {}", e),
            NewTaskError => write!(fmt, "failed to queue up new task coordinator"),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        use self::Error::*;

        match self {
            TaskSendError(e) => Some(e),
            _ => None,
        }
    }
}

struct NewTask {
    inner: Arc<Inner>,
    rx: Receiver<Task>,
}

struct LeakyBucketsInner {
    tx: UnboundedSender<NewTask>,
    rx: Mutex<Option<UnboundedReceiver<NewTask>>>,
}

/// Coordinator for rate limiters. Is used to create new rate limiters as needed.
#[derive(Clone)]
pub struct LeakyBuckets {
    inner: Arc<LeakyBucketsInner>,
}

impl Default for LeakyBuckets {
    fn default() -> Self {
        Self::new()
    }
}

impl LeakyBuckets {
    /// Construct a new coordinator for rate limiters.
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded();

        let inner = Arc::new(LeakyBucketsInner {
            tx,
            rx: Mutex::new(Some(rx)),
        });

        LeakyBuckets { inner }
    }

    /// Run the coordinator.
    pub fn coordinate(
        &mut self,
    ) -> Result<impl Future<Output = Result<(), Error>> + 'static, Error> {
        let mut rx = match self.inner.rx.lock().expect("ok mutex").take() {
            Some(rx) => rx,
            None => return Err(Error::AlreadyStarted),
        };

        Ok(async move {
            let mut futures = FuturesUnordered::new();

            loop {
                while futures.is_empty() {
                    select! {
                        NewTask { inner, rx } = rx.select_next_some() => {
                            futures.push(inner.coordinate(rx));
                        }
                    }
                }

                select! {
                    _ = futures.next() => {
                        panic!("coordinator task exited unexpectedly");
                    }
                    NewTask { inner, rx } = rx.select_next_some() => {
                        futures.push(inner.coordinate(rx));
                    }
                }
            }
        })
    }

    /// Construct a new rate limiter.
    pub fn rate_limiter(&self) -> Builder {
        Builder {
            new_task_tx: self.inner.tx.clone(),
            tokens: None,
            max: None,
            refill_interval: None,
            refill_amount: None,
        }
    }
}

/// Builder for a leaky bucket.
pub struct Builder {
    new_task_tx: UnboundedSender<NewTask>,
    tokens: Option<usize>,
    max: Option<usize>,
    refill_interval: Option<Duration>,
    refill_amount: Option<usize>,
}

impl Builder {
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

        let (tx, rx) = mpsc::channel(1);

        let inner = Arc::new(Inner {
            tokens,
            max,
            refill_interval,
            refill_amount,
            tx,
        });

        self.new_task_tx
            .unbounded_send(NewTask {
                inner: inner.clone(),
                rx,
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
    /// Current number of tokens.
    tokens: AtomicUsize,
    /// Max number of tokens.
    max: usize,
    /// Period to use when refilling.
    refill_interval: Duration,
    /// Amount to add when refilling.
    refill_amount: usize,
    /// Sender for emitting queued tasks.
    tx: Sender<Task>,
}

impl Inner {
    /// Coordinate tasks.
    async fn coordinate(self: Arc<Inner>, mut rx: Receiver<Task>) -> Result<(), Error> {
        // The queue of tasks to process.
        let mut tasks = VecDeque::new();
        // The interval at which we refill tokens.
        let mut interval = tokio::time::interval(self.refill_interval).fuse();
        // The current number of tokens accumulated locally.
        // This will increase until we have enough to satisfy the next waking task.
        let mut amount = 0;
        let mut current = None;

        'outer: loop {
            select! {
                waker = rx.select_next_some() => {
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

                    while amount > 0 && amount >= task.required {
                        // We have enough tokens to wake up the next task.
                        // Subtract it from the current amount and notify the task to wake up.
                        amount -= task.required;
                        task.complete.store(true, Ordering::Release);
                        task.waker.wake();

                        task = match tasks.pop_front() {
                            Some(task) => task,
                            None => {
                                if amount > 0 {
                                    self.balance_tokens(amount);
                                    amount = 0;
                                }

                                continue 'outer;
                            },
                        };
                    }

                    current = Some(task);

                    // If there are no more queued tasks, make the remaining tokens available to
                    // the fast path.
                    if tasks.is_empty() {
                        self.balance_tokens(amount);
                    }
                },
            }
        }
    }

    /// Subtract the given amount of tokens, allowing them to be used by the fast path acquire.
    fn balance_tokens(&self, amount: usize) {
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
    pub fn builder() -> Builder {
        LEAKY_BUCKETS.rate_limiter()
    }

    /// Acquire a single token.
    pub fn acquire_one(&self) -> Acquire<'_> {
        self.acquire(1)
    }

    /// Acquire the given `amount` of tokens.
    pub fn acquire(&self, amount: usize) -> Acquire<'_> {
        Acquire {
            tokens: &self.inner.tokens,
            max: self.inner.max,
            amount,
            tx: self.inner.tx.clone(),
            queued: None,
        }
    }
}

/// Future associated with acquiring a single token.
pub struct Acquire<'a> {
    tokens: &'a AtomicUsize,
    max: usize,
    amount: usize,
    tx: Sender<Task>,
    queued: Option<Arc<AtomicBool>>,
}

impl Future for Acquire<'_> {
    type Output = Result<(), Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Test if it has been woken up by the coordinator.
        // If that is the case, complete should be `true`.
        //
        // Otherwise we are still pending.
        if let Some(complete) = &self.queued {
            return if complete.load(Ordering::Acquire) {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            };
        }

        let mut required = self.amount;
        let current = self.tokens.fetch_add(required, Ordering::AcqRel);

        // fast path, we successfully acquired the number of tokens needed to proceed.
        if current + required < self.max {
            return Poll::Ready(Ok(()));
        }

        // subtract the number of tokens already consumed from required.
        if current < self.max {
            required -= self.max - current;
        }

        // queue up thread to be released once more tokens are available.
        match ready!(self.tx.poll_ready(cx)) {
            Ok(()) => (),
            Err(e) => return Poll::Ready(Err(Error::TaskSendError(e))),
        }

        let waker = cx.waker().clone();

        let complete = Arc::new(AtomicBool::new(false));

        self.queued = Some(complete.clone());

        if let Err(e) = self.tx.start_send(Task {
            required,
            waker,
            complete,
        }) {
            return Poll::Ready(Err(Error::TaskSendError(e)));
        }

        Poll::Pending
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

        let mut two_wakeups = 0;

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
