use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::Context;

use leaky_bucket::RateLimiter;
use tokio::time;

struct Waker;

impl std::task::Wake for Waker {
    fn wake(self: Arc<Self>) {}
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn test_try_acquire() {
    let limiter = RateLimiter::builder().refill(1).initial(1).build();

    assert!(limiter.try_acquire(1));
    assert!(!limiter.try_acquire(1));

    time::sleep(limiter.interval() * 2).await;

    assert!(limiter.try_acquire(1));
    assert!(limiter.try_acquire(1));
    assert!(!limiter.try_acquire(1));
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn test_try_acquire_contended() {
    let limiter = RateLimiter::builder().refill(2).initial(1).build();

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

    time::sleep(limiter.interval()).await;

    assert!(limiter.try_acquire(2));
    assert!(!limiter.try_acquire(1));
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn test_try_acquire_max() {
    let limiter = RateLimiter::builder().refill(100).initial(1).max(1).build();
    time::sleep(limiter.interval()).await;
    assert!(!limiter.try_acquire(2));
}
