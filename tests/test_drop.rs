use std::pin::pin;
use std::sync::Arc;
use tokio::time::{Duration, Instant};

use leaky_bucket::RateLimiter;
use tokio::task::JoinSet;
use tokio::time::sleep;

#[tokio::test(start_paused = true)]
async fn test_drop() -> anyhow::Result<()> {
    let limiter = Arc::new(
        RateLimiter::builder()
            .initial(0)
            .refill(10)
            .interval(Duration::from_millis(50))
            .max(100)
            .build(),
    );

    let limiter = limiter.clone();

    let mut task = pin!(Some(limiter.acquire(10000)));

    let mut join_set = JoinSet::new();

    for _ in 0..10 {
        let limiter = limiter.clone();

        join_set.spawn(async move {
            limiter.acquire(10).await;
        });
    }

    tokio::select! {
        _ = sleep(Duration::from_millis(1000)) => {
            // Drop the task
            task.set(None);
        }
        _ = Option::as_pin_mut(task.as_mut()).unwrap() => {
        }
        // Should never complete, because we have a giant task waiting.
        _ = join_set.join_next() => {
        }
    }

    let mut released = 0;

    let start = Instant::now();

    while join_set.join_next().await.is_some() {
        released += 1;
    }

    assert!(Instant::now().duration_since(start).as_millis() <= 10);
    assert_eq!(released, 10);
    Ok(())
}
