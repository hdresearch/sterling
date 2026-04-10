use std::future::Future;

use dto_lib::chelsea_server2::error::ChelseaServerError;
use tokio::sync::oneshot;
use tracing::{debug, error};

/// Run a long-lived handler in a background task so it keeps running even if the
/// HTTP request future is dropped (e.g., client disconnect).
pub async fn spawn_detached<F, T, E>(fut: F) -> Result<T, ChelseaServerError>
where
    F: Future<Output = Result<T, E>> + Send + 'static,
    E: Into<ChelseaServerError> + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = oneshot::channel();

    tokio::spawn(async move {
        let result = fut.await;
        if tx.send(result).is_err() {
            debug!("Detached handler result receiver dropped (client likely disconnected)");
        }
    });

    match rx.await {
        Ok(result) => result.map_err(Into::into),
        Err(err) => {
            error!(
                ?err,
                "Detached handler task ended before sending a response"
            );
            Err(ChelseaServerError::internal(
                "Operation completed before response could be returned",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spawn_detached_completes_successfully() {
        let result = spawn_detached(async { Ok::<_, ChelseaServerError>(123) })
            .await
            .expect("expected success");
        assert_eq!(result, 123);
    }

    #[tokio::test]
    async fn spawn_detached_reports_internal_error_when_task_panics() {
        let err = spawn_detached(async {
            panic!("intentional test panic to simulate task abort");
            #[allow(unreachable_code)]
            Ok::<_, ChelseaServerError>(())
        })
        .await
        .expect_err("expected helper to surface internal error");
        assert_eq!(
            err.error,
            "Operation completed before response could be returned"
        );
    }
}
