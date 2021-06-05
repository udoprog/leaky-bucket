use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    helpers::init_logging();

    for iteration in 0..5 {
        let limiter = Arc::new(
            leaky_bucket::RateLimiter::builder()
                .initial(100)
                .refill(100)
                .interval(Duration::from_millis(200))
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
                    println!("tick: {}:{}:{}", iteration, n, i);
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
    }

    Ok(())
}
