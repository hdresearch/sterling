use std::net::{IpAddr, Ipv6Addr};

use axum::{Json, extract::Path, middleware, response::IntoResponse, routing::post};
use reqwest::StatusCode;
use serde::Deserialize;
use utoipa_axum::router::OpenApiRouter;
use uuid::Uuid;

use crate::{
    action::{self, NodesAdd, NodesRemove},
    inbound::middleware::check_admin_key,
};

#[derive(Deserialize)]
pub struct AddNodeBody {
    pub node_ipv6: Ipv6Addr,
    pub node_id: Uuid,
    pub node_wg_private_key: String,
    pub node_wg_public_key: String,
    pub node_pub_ip: IpAddr,
}

async fn add_node(Json(body): Json<AddNodeBody>) -> impl IntoResponse {
    match action::call(NodesAdd::new(body)).await {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn remove_node(Path(node_id): Path<Uuid>) -> impl IntoResponse {
    match action::call(NodesRemove::new(node_id)).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub fn nodes_routes() -> OpenApiRouter {
    OpenApiRouter::new()
        .route("/add", post(add_node))
        .route("/{node_id}/remove", post(remove_node))
        .layer(middleware::from_fn(check_admin_key))
}
