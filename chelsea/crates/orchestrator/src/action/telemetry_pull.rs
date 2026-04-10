use dto_lib::chelsea_server2::system::SystemTelemetryResponse;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    action::{Action, ActionContext},
    db::{ChelseaNodeRepository, DBError},
    outbound::node_proto::HttpError,
};

pub struct TelemetryPull {
    vec: Vec<Uuid>,
}

impl TelemetryPull {
    pub fn all() -> Self {
        Self { vec: Vec::new() }
    }
}

#[derive(Debug, Error)]
pub enum TelemetryPullError {
    #[error("http-error: {0:?}")]
    Http(#[from] HttpError),

    #[error("db error: {0:?}")]
    DB(#[from] DBError),
    #[error("node_id provided non existant: {0:?}")]
    NodeNonExistant(Uuid),
}

// TODO: smart telemetry pulling only in the correct scenario.
impl Action for TelemetryPull {
    type Response = Vec<SystemTelemetryResponse>;
    type Error = TelemetryPullError;

    const ACTION_ID: &'static str = "telemetry_pull";
    async fn call(self, ctx: &ActionContext) -> Result<Self::Response, Self::Error> {
        let nodes = if self.vec.is_empty() {
            let nodes = ctx.db.node().all_under_orchestrator(ctx.orch.id()).await?;

            nodes.into_iter().map(|node| *node.id()).collect()
        } else {
            self.vec
        };

        let mut results = Vec::new();

        for node_id in nodes {
            let node = match ctx.db.node().get_by_id(&node_id).await? {
                Some(node) => node,
                None => {
                    tracing::trace!(node_id = ?&node_id, "tried pulling telemetry for non-existing node");
                    return Err(TelemetryPullError::NodeNonExistant(node_id));
                }
            };

            // Note: No request_id since this is a background telemetry pull
            let data = ctx.proto().system_telemetry(&node, None).await?;

            // TODO: Log in some ways.

            // node.telemetry_push(data);

            results.push(data);
        }

        Ok(results)
    }
}
