use leaky_bucket::RateLimiter;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test(start_paused = true)]
async fn test_threaded() -> anyhow::Result<()> {
    let limiter = Arc::new(
        RateLimiter::builder()
            .initial(100)
            .refill(100)
            .interval(Duration::from_millis(50))
            .max(100)
            .build(),
    );

    let mut tasks = Vec::new();
    let mut expected = Vec::new();

    for n in 0..10 {
        let limiter = limiter.clone();

        let task = tokio::spawn(async move {
            let mut locals = Vec::new();

            for i in 0..10 {
                limiter.acquire(10).await;
                locals.push((n, i));
            }

            locals
        });

        for i in 0..10 {
            expected.push((n, i));
        }

        tasks.push(task);
    }

    let mut globals = Vec::new();

    for t in tasks {
        globals.extend(t.await?);
    }

    globals.sort();

    assert_eq!(expected, globals);
    Ok(())
}
