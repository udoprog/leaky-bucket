use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::Context;

use leaky_bucket::RateLimiter;
use tokio::time::{self, Duration};

const INTERVAL: Duration = Duration::from_millis(100);

struct Waker;

impl std::task::Wake for Waker {
    fn wake(self: Arc<Self>) {}
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn test_try_acquire() {
    let limiter = RateLimiter::builder().refill(1).initial(1).build();

    assert!(limiter.try_acquire(1));
    assert!(!limiter.try_acquire(1));

    time::sleep(Duration::from_millis(200)).await;

    assert!(limiter.try_acquire(1));
    assert!(limiter.try_acquire(1));
    assert!(!limiter.try_acquire(1));
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn test_try_acquire_contended() {
    let limiter = RateLimiter::builder()
        .interval(INTERVAL)
        .refill(2)
        .initial(1)
        .build();

    let waker = Arc::new(Waker).into();
    let mut cx = Context::from_waker(&waker);

    {
        let mut waiting = pin!(limiter.acquire(1));
        // Task is not linked.
        assert!(limiter.try_acquire(1));
        assert!(waiting.as_mut().poll(&mut cx).is_pending());
        // Task is now linked, so we cannot acquire.
        assert!(!limiter.try_acquire(1));
    }

    time::sleep(INTERVAL).await;

    assert!(limiter.try_acquire(2));
    assert!(!limiter.try_acquire(1));
}
