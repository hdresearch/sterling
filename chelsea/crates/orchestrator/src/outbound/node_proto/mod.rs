use std::net::SocketAddr;

use orch_wg::{WG, WgPeer};
use reqwest::StatusCode;
use thiserror::Error;
use vers_config::VersConfig;

use crate::db::NodeEntity;

pub mod chelsea_lifecycle;
pub mod reporting;

/// Header name for request ID propagation.
///
/// This header is used to correlate requests across services:
/// - Proxy generates/forwards X-Request-ID to Orchestrator
/// - Orchestrator extracts it and adds to tracing span
/// - Orchestrator forwards it to Chelsea for downstream correlation
/// - All services return X-Request-ID in response headers for client correlation
///
/// Request IDs are validated to be <= 256 chars and contain only printable ASCII.
pub const REQUEST_ID_HEADER: &str = "x-request-id";

/// Maximum allowed length for request IDs to prevent log flooding
const MAX_REQUEST_ID_LENGTH: usize = 256;

/// Extension trait for reqwest::RequestBuilder to conditionally add request ID header
pub(crate) trait RequestBuilderExt {
    /// Add X-Request-ID header only if request_id is Some and valid
    fn maybe_request_id(self, request_id: Option<&str>) -> Self;
}

impl RequestBuilderExt for reqwest::RequestBuilder {
    fn maybe_request_id(self, request_id: Option<&str>) -> Self {
        match request_id {
            Some(id) if is_valid_request_id(id) => self.header(REQUEST_ID_HEADER, id),
            _ => self,
        }
    }
}

/// Validate request ID: must be printable ASCII and within length limit
fn is_valid_request_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_REQUEST_ID_LENGTH && id.bytes().all(|b| b >= 0x20 && b < 0x7F)
}

#[cfg(test)]
mod request_id_tests {
    use super::*;

    #[test]
    fn accepts_valid_uuids() {
        assert!(is_valid_request_id("550e8400-e29b-41d4-a716-446655440000"));
    }

    #[test]
    fn accepts_alphanumeric_ids() {
        assert!(is_valid_request_id("my-request-123"));
        assert!(is_valid_request_id("abc123"));
        assert!(is_valid_request_id("req_id.v1"));
    }

    #[test]
    fn rejects_empty() {
        assert!(!is_valid_request_id(""));
    }

    #[test]
    fn rejects_oversized() {
        assert!(!is_valid_request_id(&"x".repeat(257)));
        // Exactly at limit should pass
        assert!(is_valid_request_id(&"x".repeat(256)));
    }

    #[test]
    fn rejects_control_characters() {
        assert!(!is_valid_request_id("test\ninjection"));
        assert!(!is_valid_request_id("test\rinjection"));
        assert!(!is_valid_request_id("test\tinjection"));
        assert!(!is_valid_request_id("test\0injection"));
        assert!(!is_valid_request_id("test\x7Finjection"));
    }
}

/// Test mock support for intercepting ChelseaProto calls
#[cfg(any(test, feature = "integration-tests"))]
pub mod mock {
    use super::*;
    use dto_lib::chelsea_server2::vm::VmStatusResponse;
    use std::sync::Mutex;
    use uuid::Uuid;

    type VmStatusMockFn = Box<dyn Fn(Uuid) -> Result<VmStatusResponse, HttpError> + Send + Sync>;

    static VM_STATUS_MOCK: Mutex<Option<VmStatusMockFn>> = Mutex::new(None);

    /// Set a mock function for vm_status calls. The mock receives the vm_id
    /// and should return the desired Result.
    pub fn set_vm_status_mock<F>(mock_fn: F)
    where
        F: Fn(Uuid) -> Result<VmStatusResponse, HttpError> + Send + Sync + 'static,
    {
        *VM_STATUS_MOCK.lock().unwrap() = Some(Box::new(mock_fn));
    }

    /// Clear the vm_status mock
    pub fn clear_vm_status_mock() {
        *VM_STATUS_MOCK.lock().unwrap() = None;
    }

    /// Try to get a mocked response for vm_status. Returns None if no mock is set.
    pub(super) fn try_mock_vm_status(vm_id: Uuid) -> Option<Result<VmStatusResponse, HttpError>> {
        let guard = VM_STATUS_MOCK.lock().unwrap();
        guard.as_ref().map(|f| f(vm_id))
    }
}

#[derive(Debug, Error)]
pub enum HttpError {
    #[error("network error: {0:?}")]
    Other(reqwest::Error),
    #[error("conn refused")]
    ConnectionRefused,
    #[error("network timeout")]
    Timeout,
    #[error("{0}: {1}")]
    NonSuccessStatusCode(u16, String),
    #[error("body unparsable")]
    BodyUnparsable,
    #[error("not implemented")]
    NotImplemented,

    #[error("failed to make node peer")]
    FailedToMakeNodePeer,
}

impl HttpError {
    pub async fn from_response(value: reqwest::Response) -> Self {
        Self::from_status_and_body_result(&value.status(), value.text().await)
    }

    pub fn from_status_and_body(status: &StatusCode, body: String) -> Self {
        Self::NonSuccessStatusCode(status.as_u16(), body)
    }

    pub fn from_status_and_body_result(status: &StatusCode, body: reqwest::Result<String>) -> Self {
        Self::from_status_and_body(status, body.unwrap_or_else(|e| e.to_string()))
    }
}

pub struct ChelseaProto {
    http: reqwest::Client,
    port: u16,
    wg: WG,
}

impl ChelseaProto {
    const USER_AGENT: &'static str = "Chelsea-LB-DevMode/1.0";

    pub fn new(wg: WG) -> Self {
        Self {
            http: reqwest::Client::new(),
            port: VersConfig::chelsea().server_port,
            wg,
        }
    }

    /// Ensures that the node specified is a WG peer.
    /// # Returns
    /// Endpoint, with port
    fn ensure_node_wg(&self, node: &NodeEntity) -> Result<SocketAddr, HttpError> {
        self.wg
            .peer_ensure(WgPeer {
                endpoint_ip: node.ip_pub(),
                remote_ipv6: node.ip_priv(),
                port: VersConfig::chelsea().wg_port,
                pub_key: node.wg_pub_key().to_string(),
            })
            .map_err(|_| HttpError::FailedToMakeNodePeer)?;

        let sock_addr = SocketAddr::new(node.ip_pub().into(), VersConfig::chelsea().wg_port);
        Ok(sock_addr)
    }

    /// Get the configured Chelsea server port
    pub fn port(&self) -> u16 {
        self.port
    }
}
