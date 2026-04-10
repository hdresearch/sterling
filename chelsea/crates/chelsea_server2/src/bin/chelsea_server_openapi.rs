use std::fs;
use std::path::Path;

use chelsea_server2::routes::{system::SystemApiDoc, vm::VmApiDoc};
use utoipa::OpenApi;

fn main() -> anyhow::Result<()> {
    let api = VmApiDoc::openapi().merge_from(SystemApiDoc::openapi());

    // Ensure output directory exists
    let out_dir = Path::new("openapi");
    fs::create_dir_all(out_dir)?;

    // Write YAML
    let yaml = api.to_yaml()?;
    fs::write(out_dir.join("openapi.yaml"), yaml)?;

    // Write JSON
    let json = serde_json::to_string_pretty(&api)?;
    fs::write(out_dir.join("openapi.json"), json)?;

    println!("Wrote OpenAPI specs to ./openapi/openapi.yaml and ./openapi/openapi.json");
    Ok(())
}
