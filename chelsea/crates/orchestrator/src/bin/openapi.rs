use std::{fs, path::Path};

use orchestrator::{
    inbound::routes::controlplane::{
        commit_tags::CommitTagsApiDoc,
        commits::CommitsApiDoc,
        domains::DomainsControlApiDoc,
        env_vars::EnvVarsApiDoc,
        public_repositories::PublicRepositoriesApiDoc,
        repositories::RepositoriesApiDoc,
        vm::{VmControlApiDoc, VmsControlApiDoc},
    },
    openapi::ApiV1ApiDoc,
};
use utoipa::OpenApi;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Source environment variables (e.g., REGION) when generating locally.
    let _ = dotenv::dotenv();

    let openapi = ApiV1ApiDoc::openapi()
        .nest("/api/v1/vm", VmControlApiDoc::openapi())
        .nest("/api/v1/vms", VmsControlApiDoc::openapi())
        .nest("/api/v1/commits", CommitsApiDoc::openapi())
        .nest("/api/v1/commit_tags", CommitTagsApiDoc::openapi())
        .nest("/api/v1/domains", DomainsControlApiDoc::openapi())
        .nest("/api/v1/env_vars", EnvVarsApiDoc::openapi())
        .nest(
            "/api/v1/public/repositories",
            PublicRepositoriesApiDoc::openapi(),
        )
        .nest("/api/v1/repositories", RepositoriesApiDoc::openapi());

    let out_dir = Path::new("openapi");
    fs::create_dir_all(out_dir)?;

    let yaml = openapi.to_yaml()?;
    fs::write(out_dir.join("orchestrator.openapi.yaml"), yaml)?;

    let json = serde_json::to_string_pretty(&openapi)?;
    fs::write(out_dir.join("orchestrator.openapi.json"), json)?;

    println!("Wrote orchestrator OpenAPI specs to ./openapi");

    Ok(())
}
