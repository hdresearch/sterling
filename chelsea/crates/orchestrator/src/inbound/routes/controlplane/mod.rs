pub mod admin;
pub mod billing;
pub mod chelsea;
pub mod commit_tags;
pub mod commits;
pub mod deploy;
pub mod domains;
pub mod env_vars;
pub mod images;
pub mod internal;
pub mod keys;
pub mod public_repositories;
pub mod repositories;
pub mod system;
pub mod vm;

use utoipa_axum::router::OpenApiRouter;

use crate::inbound::InboundState;

pub fn controlplane_router(state: &InboundState) -> OpenApiRouter {
    OpenApiRouter::new()
        .nest("/vm", vm::vm_routes())
        .nest("/vms", vm::vms_routes())
        .nest("/commits", commits::commits_routes())
        .nest("/nodes", chelsea::nodes_routes())
        .nest("/keys", keys::keys_routes())
        .nest("/images", images::images_routes())
        .nest("/system", system::system_router())
        .nest("/admin", admin::admin_routes())
        .nest("/commit_tags", commit_tags::commit_tags_routes())
        .nest("/repositories", repositories::repositories_routes())
        .nest(
            "/public/repositories",
            public_repositories::public_repositories_routes(),
        )
        .nest("/env_vars", env_vars::env_vars_routes())
        .nest("/internal", internal::internal_routes())
        .nest("/domains", domains::domains_routes())
        .nest("/deploy", deploy::deploy_routes())
        .nest("/billing", billing::billing_routes(state))
}
