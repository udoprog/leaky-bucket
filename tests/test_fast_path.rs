use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::Context;
use std::thread;

use leaky_bucket::RateLimiter;

struct Waker;

impl std::task::Wake for Waker {
    fn wake(self: Arc<Self>) {}
}

#[test]
fn test_fast_paths() {
    let limiter = Arc::new(
        RateLimiter::builder()
            .max(10000)
            .initial(10000)
            .fair(false)
            .build(),
    );

    let mut threads = Vec::new();

    for _ in 0..100 {
        let limiter = limiter.clone();

        threads.push(thread::spawn(move || {
            let waker = Arc::new(Waker).into();
            let mut cx = Context::from_waker(&waker);

            for _ in 0..100 {
                let acquire = pin!(limiter.acquire(1));
                assert!(acquire.poll(&mut cx).is_ready());
            }
        }));
    }

    for thread in threads {
        thread.join().unwrap();
    }
}
