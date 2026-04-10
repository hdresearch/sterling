//! Test logging configuration.

use std::sync::Once;
use tracing_subscriber::EnvFilter;

static INIT: Once = Once::new();

/// Initialize test logging (only once).
pub fn init_test_logging() {
    INIT.call_once(|| {
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("warn,chelsea_test=info"));

        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_test_writer()
            .try_init()
            .ok();
    });
}
