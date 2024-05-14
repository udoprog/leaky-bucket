use std::future::Future;
use std::sync::Arc;
use std::task::Context;

use leaky_bucket::RateLimiter;

struct Waker;

impl std::task::Wake for Waker {
    fn wake(self: Arc<Self>) {}
}

#[tokio::main]
async fn main() {
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

    println!("Blocking forever...");
    limiter.acquire(1).await;
}
