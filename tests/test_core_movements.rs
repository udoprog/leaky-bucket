use leaky_bucket::RateLimiter;
use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Wake};
use std::time;

struct Waker;

impl Wake for Waker {
    fn wake(self: Arc<Self>) {}
}

#[tokio::test(start_paused = true)]
async fn test_drop_core() {
    let limiter = RateLimiter::builder()
        .interval(time::Duration::from_millis(50))
        .build();

    let waker = Arc::new(Waker).into();
    let mut cx = Context::from_waker(&waker);

    // Test that dropping a core task restores the ability to acquire new cores.
    let a1 = limiter.acquire(1);
    let b1 = limiter.acquire(1);
    let a2 = limiter.acquire(1);
    tokio::pin!(a1, b1, a2);

    assert!(!a1.is_core() && !b1.is_core());
    assert!(a1.as_mut().poll(&mut cx).is_pending());
    assert!(b1.as_mut().poll(&mut cx).is_pending());
    assert!(a1.is_core() && !b1.is_core());

    a1.set(limiter.acquire(0));
    assert!(!a1.is_core());
    assert!(a1.poll(&mut cx).is_ready());

    assert!(!a2.is_core());
    assert!(a2.as_mut().poll(&mut cx).is_pending());
    assert!(a2.is_core());
    a2.as_mut().await;
    assert!(!a2.is_core());

    b1.await;
}

#[tokio::test(start_paused = true)]
async fn test_core_move() {
    let limiter = RateLimiter::builder()
        .interval(time::Duration::from_millis(50))
        .build();

    let waker = Arc::new(Waker).into();
    let mut cx = Context::from_waker(&waker);

    // Test that dropping a core task restores the ability to acquire new cores.
    let a1 = limiter.acquire(1);
    let a2 = limiter.acquire(1);
    tokio::pin!(a1, a2);

    assert!(!a1.is_core());
    assert!(a1.as_mut().poll(&mut cx).is_pending());
    assert!(a1.is_core());

    // a2 is not a core because a1 is already a core.
    assert!(!a2.is_core());
    assert!(a2.as_mut().poll(&mut cx).is_pending());
    assert!(!a2.is_core());

    // drop the previous core and poll a2 again to become the new core.
    a1.set(limiter.acquire(1));

    assert!(a2.as_mut().poll(&mut cx).is_pending());
    assert!(a2.is_core());

    let ((), ()) = tokio::join!(a1, a2);
}
