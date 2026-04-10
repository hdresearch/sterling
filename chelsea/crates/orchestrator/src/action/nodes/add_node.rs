use crate::{
    action::{self, ActionContext, ActionError, ChooseNode, RechooseNodeError},
    db::{DBError, NodeResources},
    inbound::routes::controlplane::chelsea::AddNodeBody,
};
use thiserror::Error;

use crate::{action::Action, db::ChelseaNodeRepository};

pub struct NodesAdd(AddNodeBody);

impl NodesAdd {
    pub fn new(body: AddNodeBody) -> Self {
        NodesAdd(body)
    }
}

#[derive(Error, Debug)]
pub enum NodesAddError {
    #[error("db error: {0:#?}")]
    DBError(#[from] DBError),
    #[error("node choose action error: {0:#?}")]
    NodeChooseError(#[from] RechooseNodeError),
    #[error("internal server error")]
    InternalServerError,
}

impl Action for NodesAdd {
    type Error = NodesAddError;
    type Response = ();
    const ACTION_ID: &'static str = "nodes.add";
    async fn call(self, ctx: &'static ActionContext) -> Result<Self::Response, Self::Error> {
        let _node = ctx
            .db
            .node()
            .insert(
                self.0.node_id,
                ctx.orch.id(),
                &NodeResources::new(0, 0, 0, 0),
                self.0.node_wg_private_key.as_str(),
                self.0.node_wg_public_key.as_str(),
                Some(self.0.node_ipv6),
                Some(self.0.node_pub_ip),
            )
            .await?;

        // Verify at least one healthy node exists (candidates drop immediately, no resources reserved)
        let _ = match action::call(ChooseNode::new()).await {
            Ok(candidates) => candidates,
            Err(err) => match err {
                ActionError::Shutdown | ActionError::Timeout | ActionError::Panic => {
                    return Err(NodesAddError::InternalServerError);
                }
                ActionError::Error(err) => Err(err)?,
            },
        };

        tracing::info!("done");

        Ok(())
    }
}
