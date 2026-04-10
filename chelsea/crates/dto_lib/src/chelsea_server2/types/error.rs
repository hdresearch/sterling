use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(ToSchema, Serialize, Deserialize, Debug)]
pub struct ChelseaServerError {
    pub error: String,
    #[serde(skip_serializing, skip_deserializing)]
    status_code: StatusCode,
}

impl ChelseaServerError {
    pub fn new(error: impl Into<String>, status_code: StatusCode) -> Self {
        Self {
            error: error.into(),
            status_code,
        }
    }

    pub fn internal(error: impl Into<String>) -> Self {
        Self::new(error, StatusCode::INTERNAL_SERVER_ERROR)
    }

    pub fn status_code(&self) -> StatusCode {
        self.status_code
    }
}

impl IntoResponse for ChelseaServerError {
    fn into_response(self) -> axum::response::Response {
        let status = self.status_code;
        (status, Json(self)).into_response()
    }
}

impl From<String> for ChelseaServerError {
    fn from(value: String) -> Self {
        Self::internal(value)
    }
}

impl From<&str> for ChelseaServerError {
    fn from(value: &str) -> Self {
        Self::internal(value.to_owned())
    }
}
