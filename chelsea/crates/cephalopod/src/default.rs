//! Singleton default client, initialized from VersConfig.

use std::sync::OnceLock;

use crate::client::Client;
use crate::error::CephalopodError;

static DEFAULT_CLIENT: OnceLock<Result<Client, CephalopodError>> = OnceLock::new();

/// Get a reference to the default cephalopod client.
///
/// Connects on first call using the `chelsea` config values from VersConfig.
/// Subsequent calls return the cached client (or error).
pub fn default_client() -> Result<&'static Client, CephalopodError> {
    DEFAULT_CLIENT
        .get_or_init(|| Client::connect("chelsea", "rbd"))
        .as_ref()
        .map_err(|e| e.clone())
}
