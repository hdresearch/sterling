use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, ToSchema)]
pub struct GenerateApiKeyRequest {
    pub user_id: Uuid,
    pub org_id: Uuid,
    pub label: String,
}

#[derive(Serialize, ToSchema)]
pub struct GenerateApiKeyResponse {
    pub api_key: String,
}
