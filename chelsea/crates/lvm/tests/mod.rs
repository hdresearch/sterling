pub mod core;
pub mod integration;

#[cfg(test)]
#[ctor::ctor]
fn init_test_logging() {
    use tracing::level_filters::LevelFilter;

    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::DEBUG)
        .init();
}
