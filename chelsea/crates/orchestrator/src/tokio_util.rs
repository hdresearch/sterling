use std::{pin::Pin, time::Duration};

use tokio::{sync::oneshot::Sender, task::JoinHandle, time};
use vers_config::VersConfig;

pub struct TokioTaskGracefulShutdown {
    pub sender: Sender<()>,
    pub task: JoinHandle<()>,
    pub label: Option<&'static str>,
}

impl IntoFuture for TokioTaskGracefulShutdown {
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output>>>;
    type Output = ();
    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            // Channel is dropped which means that task is finished.
            if self.sender.send(()).is_err() {
                return;
            }
            if let Err(err) = time::timeout(
                Duration::from_secs(VersConfig::orchestrator().task_shutdown_timeout_secs),
                self.task,
            )
            .await
            {
                tracing::error!(?err, "exceeded timout of task graceful shutdown");
            };

            if let Some(label) = self.label {
                tracing::info!(label, "shutting down");
            }
        })
    }
}

pub struct GracefulShutdown(pub Vec<TokioTaskGracefulShutdown>);

impl IntoFuture for GracefulShutdown {
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output>>>;
    type Output = ();
    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            for task in self.0 {
                task.await;
            }
        })
    }
}
