use tokio::sync::OnceCell;

use aws_sdk_s3::Client;

static CLIENT: OnceCell<Client> = OnceCell::const_new();

/// Returns a shared S3 client initialised from environment variables.
///
/// The client is created once (on first call) and reused for the lifetime of
/// the process. For tests or situations where you need a custom endpoint, build
/// a `Client` yourself and pass it to the `util::s3` functions directly.
pub async fn get_s3_client() -> &'static Client {
    CLIENT
        .get_or_init(|| async {
            let config = aws_config::load_from_env().await;
            Client::new(&config)
        })
        .await
}
