use axum::{extract::FromRequestParts, http::StatusCode, http::request::Parts};
use std::future::Future;

use dto_lib::ErrorResponse;

use crate::action::ValidateApiKey;
use crate::db::ApiKeyEntity;
use crate::inbound::InboundState;

#[derive(Clone, Debug)]
pub struct AuthApiKey(pub ApiKeyEntity);

impl<S> FromRequestParts<S> for AuthApiKey
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, ErrorResponse);

    #[tracing::instrument(skip_all)]
    fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        let token = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(|s| s.to_string());

        let state = parts.extensions.get::<InboundState>().cloned();

        async move {
            let token = match token {
                Some(t) if !t.is_empty() => t,
                _ => {
                    return Err((
                        StatusCode::UNAUTHORIZED,
                        ErrorResponse::new("Missing or invalid Authorization header".into()),
                    ));
                }
            };

            let state = state.ok_or_else(|| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorResponse::new("Internal Server Error".into()),
                )
            })?;

            match ValidateApiKey::new(token).call(&state.db).await {
                Ok(entity) => Ok(AuthApiKey(entity)),
                Err(_) => Err((
                    StatusCode::UNAUTHORIZED,
                    ErrorResponse::new("Invalid bearer token".into()),
                )),
            }
        }
    }
}
