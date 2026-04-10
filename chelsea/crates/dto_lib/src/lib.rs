pub mod chelsea_server2;
pub mod orchestrator;
pub mod proxy;
pub mod domains {
    /// HTTP path served by the proxy to confirm a custom domain is routed to Chelsea.
    pub const READINESS_PROBE_PATH: &str = "/.well-known/chelsea-domain-check";
    /// Static body returned by the readiness probe endpoint.
    pub const READINESS_PROBE_RESPONSE: &str = "chelsea-domain-ok";
}

use std::borrow::Cow;

use axum::{Json, http::StatusCode, response::IntoResponse};

use utoipa::{
    PartialSchema, ToSchema,
    openapi::{Object, RefOr, Schema, Type, schema::SchemaType},
};

pub struct ErrorResponse {
    error: String,
    status: Option<StatusCode>,
}

impl ToSchema for ErrorResponse {
    fn name() -> Cow<'static, str> {
        Cow::Borrowed("ErrorResponse")
    }
}

impl PartialSchema for ErrorResponse {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        let mut error_obj = Object::with_type(SchemaType::new(Type::String));
        error_obj.description = Some("Reason of error".into());

        let mut success_obj = Object::with_type(SchemaType::new(Type::Boolean));
        success_obj.description = Some("Is always: false".into());

        RefOr::T(Schema::Object(
            Object::builder()
                .property("error", RefOr::T(Schema::Object(error_obj)))
                .property("success", RefOr::T(Schema::Object(success_obj)))
                .build(),
        ))
    }
}

impl ErrorResponse {
    pub fn new(reason: String) -> Self {
        Self {
            error: reason,
            status: None,
        }
    }

    pub fn not_found(reason: Option<String>) -> Self {
        Self {
            error: reason.unwrap_or(String::from("Not Found")),
            status: Some(StatusCode::NOT_FOUND),
        }
    }

    pub fn bad_request(reason: Option<String>) -> Self {
        Self {
            error: reason.unwrap_or(String::from("Bad Request")),
            status: Some(StatusCode::BAD_REQUEST),
        }
    }

    pub fn internal_server_error(reason: Option<String>) -> Self {
        Self {
            error: reason.unwrap_or(String::from("Internal Server Error")),
            status: Some(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }

    pub fn forbidden(reason: Option<String>) -> Self {
        Self {
            error: reason.unwrap_or(String::from("Forbidden")),
            status: Some(StatusCode::FORBIDDEN),
        }
    }

    pub fn conflict(reason: Option<String>) -> Self {
        Self {
            error: reason.unwrap_or(String::from("Conflict")),
            status: Some(StatusCode::CONFLICT),
        }
    }

    pub fn payload_too_large(reason: Option<String>) -> Self {
        Self {
            error: reason.unwrap_or(String::from("Payload Too Large")),
            status: Some(StatusCode::PAYLOAD_TOO_LARGE),
        }
    }
}

impl IntoResponse for ErrorResponse {
    fn into_response(self) -> axum::response::Response {
        let mut response = Json(serde_json::json!({
            "success": false,
            "error": self.error
        }))
        .into_response();

        if let Some(status) = self.status {
            *response.status_mut() = status;
        };

        response
    }
}

pub mod clusters {
    #[derive(Debug, Clone)]
    pub struct NewClusterParamsPartial {
        pub kernel_name: Option<String>,
        pub rootfs_name: Option<String>,
        pub vcpu_count: Option<i64>,
        pub mem_size_mib: Option<i64>,
        pub fs_size_cluster_mib: Option<i64>,
        pub fs_size_vm_mib: Option<i64>,
        pub cluster_alias: Option<String>,
        pub vm_alias: Option<String>,
    }

    #[derive(Debug, Clone)]
    pub struct ClusterFromCommitParamsPartial {
        pub commit_id: String,
        pub fs_size_cluster_mib: Option<i64>,
        pub cluster_alias: Option<String>,
        pub vm_alias: Option<String>,
    }

    pub enum ClusterCreateRequest {
        New {
            params: NewClusterParamsPartial,
        },
        FromCommit {
            params: ClusterFromCommitParamsPartial,
        },
    }
}

pub mod vms {
    use std::net::IpAddr;

    use serde::{Deserialize, Serialize};
    use utoipa::ToSchema;

    #[derive(Serialize, Deserialize, ToSchema)]
    #[serde(rename_all = "lowercase")]
    pub enum InstanceState {
        // FIXME: is there more?
        Running,
    }
    #[derive(Serialize, Deserialize)]
    pub struct VmNetworkInfoDTO {
        pub tap0_ip: IpAddr,
        pub tap0_name: String,
        pub guest_mac: String,
        pub guest_ip: IpAddr,
        pub vm_namespace: String,
        pub ssh_port: u16,
    }

    #[derive(Serialize, ToSchema)]
    pub struct VmDTO {
        pub id: String,
        pub alias: Option<Vec<String>>,
        pub state: InstanceState,
        pub parent_id: Option<String>,
        pub cluster_id: String,
        #[schema(no_recursion)]
        pub children: Vec<VmDTO>,
        #[serde(skip)]
        pub network_info: VmNetworkInfoDTO,
        #[serde(skip)]
        pub ip_address: IpAddr,
        pub vcpu_count: i32,
        pub mem_size_mib: i32,
        pub fs_size_mib: i32,
    }
}
