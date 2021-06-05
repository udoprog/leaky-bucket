/// Initialize logging for a given example.
pub fn init_logging() {
    use tracing_subscriber::prelude::*;

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_level(true)
                .compact(),
        )
        .init();
}
