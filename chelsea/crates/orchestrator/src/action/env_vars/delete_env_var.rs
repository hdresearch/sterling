use thiserror::Error;

use crate::{
    action::Action,
    db::{ApiKeyEntity, DBError, EnvVarsRepository},
};

#[derive(Debug, Clone)]
pub struct DeleteEnvVar {
    pub api_key: ApiKeyEntity,
    pub key: String,
}

impl DeleteEnvVar {
    pub fn new(api_key: ApiKeyEntity, key: String) -> Self {
        Self { api_key, key }
    }
}

#[derive(Debug, Error)]
pub enum DeleteEnvVarError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("environment variable not found")]
    NotFound,
}

impl Action for DeleteEnvVar {
    type Response = ();
    type Error = DeleteEnvVarError;
    const ACTION_ID: &'static str = "env_vars.delete";

    async fn call(self, ctx: &crate::action::ActionContext) -> Result<Self::Response, Self::Error> {
        let deleted = ctx
            .db
            .env_vars()
            .delete(self.api_key.user_id(), &self.key)
            .await?;

        if deleted {
            Ok(())
        } else {
            Err(DeleteEnvVarError::NotFound)
        }
    }
}
