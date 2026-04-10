use utoipa::{Modify, OpenApi};

/// Public OpenAPI document for the orchestrator control plane.
#[derive(OpenApi)]
#[openapi(
    info(title = "Orchestrator Control Plane API", version = "0.1.0"),
    modifiers(&BearerSecurity)
)]
pub struct ApiV1ApiDoc;

struct BearerSecurity;

impl Modify for BearerSecurity {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::{
            schema::Components,
            security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
        };

        let mut components = openapi.components.take().unwrap_or_else(Components::new);
        components.add_security_scheme(
            "bearer_auth",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("Token")
                    .build(),
            ),
        );
        openapi.components = Some(components);
    }
}
