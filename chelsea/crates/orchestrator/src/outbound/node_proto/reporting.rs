use dto_lib::chelsea_server2::system::SystemTelemetryResponse;
use dto_lib::chelsea_server2::vm::VmListAllResponse;
use std::{fmt::Debug, time::Duration};
use thiserror::Error;
use tokio::time::timeout;
use tracing::{error, warn};
use vers_config::VersConfig;

use crate::{
    db::NodeEntity,
    outbound::node_proto::{ChelseaProto, HttpError, RequestBuilderExt},
};

#[derive(Error, Debug)]
pub enum TryNukingError {
    #[error("chelsea metal node not ready to be nuked")]
    NotReady,
    #[error("http-error {0:?}")]
    HttpError(HttpError),
}

#[derive(Error, Debug)]
pub enum SystemHealthError {
    #[error("non 2xx status code")]
    Non2xxStatusCode,
    #[error("node is down")]
    NodeDown,
    #[error("node didn't answer in time")]
    Timeout,
    #[error("http error {0:?}")]
    Http(#[from] HttpError),
}

impl ChelseaProto {
    pub async fn vm_list_all(
        &self,
        node: &NodeEntity,
        request_id: Option<&str>,
    ) -> Result<VmListAllResponse, HttpError> {
        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();
        let http_fut = self
            .http
            .get(&format!("http://{endpoint}/api/vm"))
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .send();

        match timeout(
            Duration::from_secs(VersConfig::orchestrator().node_proto_request_timeout_secs),
            http_fut,
        )
        .await
        {
            Ok(Ok(response)) if response.status().is_success() => {
                match response.json::<VmListAllResponse>().await {
                    Ok(list_response) => Ok(list_response),
                    Err(e) => {
                        error!(
                            node_ip = %endpoint,
                            error = %e,
                            "Failed to parse vm list response"
                        );
                        Err(HttpError::BodyUnparsable)
                    }
                }
            }
            Ok(Ok(response)) => {
                error!(
                    node_ip = %endpoint,
                    status = %response.status(),
                    "VM list API returned error status"
                );
                Err(HttpError::from_response(response).await)
            }
            Ok(Err(e)) => {
                error!(node_ip = %endpoint, "Non 2xx response from chelsea");
                Err(HttpError::Other(e))
            }
            Err(_) => {
                error!(node_ip = %endpoint, "Network timeout fetching VM list");
                Err(HttpError::Timeout)
            }
        }
    }

    pub async fn system_health(
        &self,
        node: &NodeEntity,
        request_id: Option<&str>,
    ) -> Result<(), SystemHealthError> {
        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();
        let http_fut = self
            .http
            .get(&format!("http://{endpoint}/api/system/health"))
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .send();

        match timeout(
            Duration::from_secs(VersConfig::orchestrator().health_check_timeout_secs),
            http_fut,
        )
        .await
        {
            Ok(Ok(response)) if response.status().is_success() => Ok(()),
            Ok(Ok(response)) => {
                warn!(
                    node_ip = %endpoint,
                    status = %response.status(),
                    "System health endpoint returned non-success status"
                );
                Err(SystemHealthError::Non2xxStatusCode)
            }
            Ok(Err(_)) => Err(SystemHealthError::NodeDown),
            Err(_) => Err(SystemHealthError::Timeout),
        }
    }

    pub async fn system_telemetry(
        &self,
        node: &NodeEntity,
        request_id: Option<&str>,
    ) -> Result<SystemTelemetryResponse, HttpError> {
        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();
        let http_fut = self
            .http
            .get(&format!("http://{endpoint}/api/system/telemetry"))
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .send();

        match timeout(
            Duration::from_secs(VersConfig::orchestrator().node_proto_request_timeout_secs),
            http_fut,
        )
        .await
        {
            Ok(Ok(response)) if response.status().is_success() => {
                let text = response.text().await.unwrap();
                match serde_json::from_str::<SystemTelemetryResponse>(&text) {
                    Ok(telemetry_response) => Ok(telemetry_response),
                    Err(e) => {
                        error!(
                            node_ip = %endpoint,
                            error = %e,
                            body = ?text,
                            "Failed to parse system telemetry response"
                        );
                        Err(HttpError::BodyUnparsable)
                    }
                }
            }
            Ok(Ok(response)) => {
                error!(
                    node_ip = %endpoint,
                    status = %response.status(),
                    "System telemetry endpoint returned error status"
                );
                Err(HttpError::from_response(response).await)
            }
            Ok(Err(e)) => {
                if e.is_connect() {
                    Err(HttpError::ConnectionRefused)
                } else {
                    Err(HttpError::Other(e))
                }
            }
            Err(_) => {
                error!(node_ip = %endpoint, "Network timeout fetching system telemetry");
                Err(HttpError::Timeout)
            }
        }
    }
}
