use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use utoipa::{OpenApi, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::{
    action::{CreateDomain, DeleteDomain, DomainResponse, GetDomain, ListDomains},
    inbound::{InboundState, OperationId, extractors::AuthApiKey},
};

/// Request body for POST /api/v1/domains
#[derive(Serialize, Deserialize, ToSchema, Debug)]
pub struct CreateDomainRequest {
    pub vm_id: Uuid,
    pub domain: String,
}

/// Response body for DELETE /api/v1/domains/{domain_id}
#[derive(Serialize, Deserialize, ToSchema, Debug)]
pub struct DeleteDomainResponse {
    pub domain_id: Uuid,
}

/// Query parameters for GET /api/v1/domains
#[derive(Deserialize, Debug)]
pub struct ListDomainsQuery {
    pub vm_id: Option<Uuid>,
}

#[utoipa::path(
    post,
    path = "",
    request_body = CreateDomainRequest,
    responses(
        (status = 201, description = "Domain created", body = DomainResponse),
        (status = 400, description = "Invalid request", body = dto_lib::ErrorResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = dto_lib::ErrorResponse),
        (status = 404, description = "VM not found", body = dto_lib::ErrorResponse),
        (status = 409, description = "Domain already exists", body = dto_lib::ErrorResponse),
        (status = 500, description = "Internal server error", body = dto_lib::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "domains"
)]
pub async fn create_domain(
    Extension(state): Extension<InboundState>,
    _operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Json(req): Json<CreateDomainRequest>,
) -> impl IntoResponse {
    match CreateDomain::new(req.vm_id, req.domain, key)
        .call(&state.db)
        .await
    {
        Ok(response) => (StatusCode::CREATED, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    get,
    path = "",
    params(("vm_id" = Option<Uuid>, Query, description = "Filter by VM ID")),
    responses(
        (status = 200, description = "List of domains", body = Vec<DomainResponse>),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = dto_lib::ErrorResponse),
        (status = 500, description = "Internal server error", body = dto_lib::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "domains"
)]
pub async fn list_domains(
    Extension(state): Extension<InboundState>,
    _operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Query(query): Query<ListDomainsQuery>,
) -> impl IntoResponse {
    match ListDomains::new(query.vm_id, key).call(&state.db).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/{domain_id}",
    params(("domain_id" = Uuid, Path, description = "Domain ID")),
    responses(
        (status = 200, description = "Domain details", body = DomainResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = dto_lib::ErrorResponse),
        (status = 404, description = "Not found", body = dto_lib::ErrorResponse),
        (status = 500, description = "Internal server error", body = dto_lib::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "domains"
)]
pub async fn get_domain(
    Extension(state): Extension<InboundState>,
    _operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Path(domain_id): Path<Uuid>,
) -> impl IntoResponse {
    match GetDomain::new(domain_id, key).call(&state.db).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    delete,
    path = "/{domain_id}",
    params(("domain_id" = Uuid, Path, description = "Domain ID")),
    responses(
        (status = 200, description = "Domain deleted", body = DeleteDomainResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = dto_lib::ErrorResponse),
        (status = 404, description = "Not found", body = dto_lib::ErrorResponse),
        (status = 500, description = "Internal server error", body = dto_lib::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "domains"
)]
pub async fn delete_domain(
    Extension(state): Extension<InboundState>,
    _operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Path(domain_id): Path<Uuid>,
) -> impl IntoResponse {
    match DeleteDomain::new(domain_id, key).call(&state.db).await {
        Ok(deleted_id) => (
            StatusCode::OK,
            Json(DeleteDomainResponse {
                domain_id: deleted_id,
            }),
        )
            .into_response(),
        Err(e) => e.into_response(),
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(create_domain, list_domains, get_domain, delete_domain),
    components(schemas(
        CreateDomainRequest,
        DeleteDomainResponse,
        DomainResponse,
        dto_lib::ErrorResponse,
    ))
)]
pub struct DomainsControlApiDoc;

pub fn domains_routes() -> OpenApiRouter {
    OpenApiRouter::with_openapi(DomainsControlApiDoc::openapi())
        .routes(routes!(create_domain, list_domains))
        .routes(routes!(get_domain, delete_domain))
}
