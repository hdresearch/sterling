use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::post,
};
use chelsea_server2::{
    ChelseaServerCore,
    types::{error::ChelseaServerError, vm::VmNotifyRequest},
};
use std::{net::SocketAddr, sync::Arc};
use tracing::{error, info};
use uuid::Uuid;

/// A locally-running server intended to be a point of contact for VMs, and serve an API accessible only to them.
pub struct ChelseaVmEventServer {
    core: Arc<dyn ChelseaServerCore>,
    address: SocketAddr,
}

impl ChelseaVmEventServer {
    pub fn new(core: Arc<dyn ChelseaServerCore>, address: SocketAddr) -> Self {
        Self { core, address }
    }

    /// Start the server
    pub async fn start(&self) -> std::io::Result<()> {
        let router = Router::new()
            .route("/api/vm/{vm_id}/notify", post(vm_notify_handler))
            .with_state(self.core.clone());

        // Bind a listener
        let listener = tokio::net::TcpListener::bind(&self.address).await?;

        // Start the server
        info!("EventServer listening on http://{}", self.address);
        axum::serve(listener, router.into_make_service()).await
    }

    /// Get the chelsea_notify_boot_url_template variable expected by booting VMs.
    pub fn chelsea_notify_boot_url_template(addr: &SocketAddr) -> String {
        format!(
            "http://{addr}{endpoint}",
            endpoint = "/api/vm/:vm_id/notify"
        )
    }
}

/// POST /api/vm/:vm_id/notify. Handles an event sent by a VM to the host.
async fn vm_notify_handler(
    State(core): State<Arc<dyn ChelseaServerCore>>,
    Path(vm_id): Path<Uuid>,
    Json(request): Json<VmNotifyRequest>,
) -> Result<StatusCode, ChelseaServerError> {
    match core.vm_notify(&vm_id, request).await {
        Ok(()) => Ok(StatusCode::OK),
        Err(error) => {
            error!(?error, "Error on /api/vm/{}/notify POST", vm_id);
            Err(error.into())
        }
    }
}
