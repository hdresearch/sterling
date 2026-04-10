use std::net::SocketAddr;
use std::sync::Arc;

use crate::ChelseaServerCore;
use crate::middleware::operation_id_middleware;
use crate::routes::{
    admin::create_admin_router, images::create_images_router, system::create_system_router,
    vm::create_vm_router,
};

use axum::{Router, middleware};
use tracing::info;

pub async fn run_server(core: Arc<dyn ChelseaServerCore>, addr: SocketAddr) -> anyhow::Result<()> {
    // Create the router and OpenAPI spec
    let (vm_router, _vm_openapi) = create_vm_router(core.clone());
    let (system_router, _system_openapi) = create_system_router(core.clone());
    let (admin_router, _admin_openapi) = create_admin_router(core.clone());
    let (images_router, _images_openapi) = create_images_router(core);

    // Mount the API router at /api/vm
    let app = Router::new()
        .merge(vm_router)
        .merge(system_router)
        .merge(admin_router)
        .merge(images_router)
        .layer(middleware::from_fn(operation_id_middleware));

    // Bind a listener on addr
    let listener = tokio::net::TcpListener::bind(addr).await?;

    // Start the server
    info!("ChelseaServer2 listening on http://{}", addr);
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}
