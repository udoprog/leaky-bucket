#![recursion_limit = "256"]
#![feature(async_await)]
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
//! ```no_run
//! #![feature(async_await)]
//!
//! use futures::prelude::*;
//! use leaky_bucket::LeakyBucket;
//! use std::{error::Error, time::Duration};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn Error>> {
//!     let rate_limiter = LeakyBucket::builder()
//!         .max(100)
//!         .refill_interval(Duration::from_secs(10))
//!         .refill_amount(100)
//!         .build();
//!
//!     let coordinator = rate_limiter.coordinate().boxed();
//!
//!     // spawn the coordinate thread to refill the rate limiter.
//!     tokio::spawn(async move { coordinator.await.expect("coordinate thread errored") });

//!     println!("Waiting for permit...");
//!     // should take about ten seconds to get a permit.
//!     rate_limiter.acquire(100).await;
//!     println!("I made it!");
//!
//!     Ok(())
//! }
//! ```

use futures::{
    channel::mpsc::{self, Receiver, Sender},
    stream::StreamExt as _,
    task::AtomicWaker,
};
use parking_lot::Mutex;
use std::{
    collections::VecDeque,
    error, fmt,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio_timer::Interval;

/// Error type for the rate limiter.
#[derive(Debug)]
pub enum Error {
    /// The bucket has already been started.
    AlreadyStarted,
}

impl fmt::Display for Error {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        use self::Error::*;

        match self {
            AlreadyStarted => write!(fmt, "already started"),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            _ => None,
        }
    }
}

/// Builder for a leaky bucket.
#[derive(Debug, Default, Clone)]
pub struct Builder {
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
    pub fn build(self) -> LeakyBucket {
        const DEFAULT_MAX: usize = 120;
        const DEFAULT_TOKENS: usize = 0;
        const DEFAULT_REFILL_INTERVAL: Duration = Duration::from_secs(1);
        const DEFAULT_REFILL_AMOUNT: usize = 1;

        let max = self.max.unwrap_or(DEFAULT_MAX);
        let tokens = max.saturating_sub(self.tokens.unwrap_or(DEFAULT_TOKENS));
        let refill_interval = self
            .refill_interval
            .unwrap_or(DEFAULT_REFILL_INTERVAL.clone());
        let refill_amount = self.refill_amount.unwrap_or(DEFAULT_REFILL_AMOUNT);

        let tokens = AtomicUsize::new(tokens);

        let (tx, rx) = mpsc::channel(1);

        LeakyBucket {
            inner: Arc::new(Inner {
                tokens,
                max,
                refill_interval,
                refill_amount,
                tx,
                rx: Mutex::new(Some(rx)),
            }),
        }
    }
}

struct Waker {
    /// Amount required to wake up the given task.
    required: usize,
    /// Waker to call.
    waker: AtomicWaker,
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
    tx: Sender<Waker>,
    /// Receive tasks to wake up.
    rx: Mutex<Option<Receiver<Waker>>>,
}

impl Inner {
    /// Coordinate tasks.
    async fn coordinate(self: Arc<Inner>) -> Result<(), Error> {
        let mut rx = match self.rx.lock().take() {
            Some(rx) => rx,
            None => return Err(Error::AlreadyStarted),
        };

        let mut wakers = VecDeque::new();
        let mut interval = Interval::new_interval(self.refill_interval.clone()).fuse();
        // amount that has been refilled.
        let mut amount = 0;

        loop {
            futures::select! {
                waker = rx.select_next_some() => {
                    wakers.push_back(waker);
                },
                _ = interval.select_next_some() => {
                    amount += self.refill_amount;

                    let waker = match wakers.front() {
                        Some(waker) => waker,
                        None => {
                            // NB: nothing to wake up, subtract the number of
                            // tokens immediately allowing future acquire's to
                            // enter the fast path.
                            self.balance_tokens(amount);
                            amount = 0;
                            continue;
                        },
                    };

                    while amount > 0 {
                        let waker = match wakers.front() {
                            Some(waker) => waker,
                            None => break,
                        };

                        if amount < waker.required {
                            break;
                        }

                        // NB: available = amount already added to the token bucket.
                        amount -= waker.required;
                        waker.complete.store(true, Ordering::Release);
                        waker.waker.wake();
                        wakers.pop_front();
                    }

                    if wakers.is_empty() {
                        self.balance_tokens(amount);
                    }
                },
            }
        }
    }

    /// Subtracts the given amount of tokens.
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
    /// Construct a new builder for a leaky bucket.
    pub fn builder() -> Builder {
        Builder::default()
    }

    /// Run the leaky bucket.
    pub fn coordinate(&self) -> impl Future<Output = Result<(), Error>> {
        self.inner.clone().coordinate()
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
    tx: Sender<Waker>,
    queued: Option<Arc<AtomicBool>>,
}

impl Future for Acquire<'_> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // if it has been woken up by the coordinator.
        if let Some(complete) = &self.queued {
            return match complete.load(Ordering::Acquire) {
                true => Poll::Ready(()),
                false => Poll::Pending,
            };
        }

        let mut required = self.amount;
        let current = self.tokens.fetch_add(required, Ordering::AcqRel);

        // fast path.
        while current + required < self.max {
            return Poll::Ready(());
        }

        // subtract the number of tokens already consumed.
        if current < self.max {
            required -= self.max - current;
        }

        // queue up thread to be released once more tokens are available.
        match self.tx.poll_ready(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(())) => (),
            Poll::Ready(Err(e)) => panic!("failed to poll sender: {}", e),
        }

        let waker = AtomicWaker::new();
        waker.register(cx.waker());

        let complete = Arc::new(AtomicBool::new(false));

        self.queued = Some(complete.clone());

        if let Err(e) = self.tx.start_send(Waker {
            required,
            waker,
            complete,
        }) {
            panic!("failed to queue sender: {}", e);
        }

        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::LeakyBucket;
    use futures::prelude::*;
    use std::time::{Duration, Instant};

    #[test]
    fn test_leaky_bucket() {
        use tokio::runtime::current_thread::Runtime;
        let mut rt = Runtime::new().expect("working runtime");

        rt.block_on(async move {
            let leaky = LeakyBucket::builder()
                .tokens(0)
                .max(10)
                .refill_amount(10)
                .refill_interval(interval)
                .build();

            let mut wakeups = 0;
            let mut duration = None;

            let test = async {
                let start = Instant::now();
                leaky.acquire(10).await;
                wakeups += 1;
                leaky.acquire(10).await;
                wakeups += 1;
                leaky.acquire(10).await;
                wakeups += 1;
                duration = Some(Instant::now().duration_since(start));
            };

            futures::future::select(test.boxed(), leaky.coordinate().boxed()).await;

            assert_eq!(3, wakeups);
            assert!(duration.expect("expected measured duration") > interval * 2);
        });
    }
}
