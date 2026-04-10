use dto_lib::orchestrator::env_var::EnvVarsResponse;
use thiserror::Error;

use crate::{
    action::Action,
    db::{ApiKeyEntity, DBError, EnvVarsRepository},
};

#[derive(Debug, Clone)]
pub struct ListEnvVars {
    pub api_key: ApiKeyEntity,
}

impl ListEnvVars {
    pub fn new(api_key: ApiKeyEntity) -> Self {
        Self { api_key }
    }
}

#[derive(Debug, Error)]
pub enum ListEnvVarsError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
}

impl Action for ListEnvVars {
    type Response = EnvVarsResponse;
    type Error = ListEnvVarsError;
    const ACTION_ID: &'static str = "env_vars.list";

    async fn call(self, ctx: &crate::action::ActionContext) -> Result<Self::Response, Self::Error> {
        let vars = ctx
            .db
            .env_vars()
            .get_by_user_id(self.api_key.user_id())
            .await?;
        Ok(EnvVarsResponse { vars })
    }
}
