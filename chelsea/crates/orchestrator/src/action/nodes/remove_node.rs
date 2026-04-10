use crate::{
    action::{self, ActionContext, ActionError, ChooseNode, RechooseNodeError},
    db::DBError,
};
use thiserror::Error;
use uuid::Uuid;

use crate::{action::Action, db::ChelseaNodeRepository};

pub struct NodesRemove {
    node_id: Uuid,
}

impl NodesRemove {
    pub fn new(node_id: Uuid) -> Self {
        NodesRemove { node_id }
    }
}

#[derive(Error, Debug)]
pub enum NodesRemoveError {
    #[error("db error: {0:#?}")]
    DBError(#[from] DBError),
    #[error("node choose action error: {0:#?}")]
    NodeChooseError(#[from] RechooseNodeError),
    #[error("internal server error")]
    InternalServerError,
}

impl Action for NodesRemove {
    type Error = NodesRemoveError;
    type Response = ();
    const ACTION_ID: &'static str = "nodes.remove";

    async fn call(self, ctx: &'static ActionContext) -> Result<Self::Response, Self::Error> {
        let Some(node) = ctx.db.node().get_by_id(&self.node_id).await? else {
            return Err(NodesRemoveError::InternalServerError);
        };

        ctx.db.node().delete(&self.node_id).await?;

        if let Err(err) = ctx.wg().peer_remove(node.wg_pub_key()) {
            tracing::error!(?err, "error removing wg node");
            return Err(NodesRemoveError::InternalServerError);
        };

        if let Err(err) = action::call(ChooseNode::new()).await {
            match err {
                ActionError::Shutdown | ActionError::Timeout | ActionError::Panic => {
                    return Err(NodesRemoveError::InternalServerError);
                }
                ActionError::Error(err) => Err(err)?,
            }
        };

        tracing::info!("done");

        Ok(())
    }
}
