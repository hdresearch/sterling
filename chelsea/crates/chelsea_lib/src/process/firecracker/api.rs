use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use macaddr::MacAddr6;
use reqwest::Client;
use serde_json::json;
use uuid::Uuid;
use vers_config::VersConfig;

use crate::process::firecracker::{
    config::{FirecrackerProcessLoggerLogLevel, PathBufJailer},
    error::FirecrackerApiError,
    types::{FirecrackerInstanceInfo, MachineConfiguration},
};

const SOCKET_PATH: &str = "/run/vm.sock";

/// A struct representing a Firecracker API client.
#[derive(Debug)]
pub struct FirecrackerApi {
    /// The path to the socket; created by exec'ing jailer/firecracker, do not create manually. Intended as the --api-sock value for the exec call
    pub socket_path: PathBufJailer,
    /// A reqwest client holding a connection pool to the socket
    client: Client,
}

impl FirecrackerApi {
    pub fn new(vm_id: Uuid) -> Result<Self, reqwest::Error> {
        let socket_path = PathBufJailer::new(vm_id, PathBuf::from(SOCKET_PATH));
        let firecracker_api_timeout =
            Duration::from_secs(VersConfig::chelsea().firecracker_api_timeout_secs);
        let client = reqwest::Client::builder()
            .unix_socket(socket_path.with_jail_root())
            .timeout(firecracker_api_timeout)
            .build()?;
        Ok(Self {
            socket_path,
            client,
        })
    }

    async fn handle_response(response: reqwest::Response) -> Result<String, FirecrackerApiError> {
        if !response.status().is_success() {
            let status_code = response.status();
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            Err(FirecrackerApiError::ResponseNotOk {
                status_code,
                error_body,
            })
        } else {
            Ok(response.text().await?)
        }
    }

    /// Get the current Firecracker machine configuration (GET /machine-config).
    pub async fn get_machine_configuration(
        &self,
    ) -> Result<MachineConfiguration, FirecrackerApiError> {
        let response = self
            .client
            .get("http://localhost/machine-config")
            .send()
            .await?;
        let text = Self::handle_response(response).await?;
        let config: MachineConfiguration = serde_json::from_str(&text)?;
        Ok(config)
    }

    /// Firecracker API: PUT /machine-config
    pub async fn configure_machine(
        &self,
        machine_configuration: &MachineConfiguration,
    ) -> Result<(), FirecrackerApiError> {
        let body = machine_configuration;

        let response = self
            .client
            .put("http://localhost/machine-config")
            .json(&body)
            .send()
            .await?;

        Self::handle_response(response).await?;
        Ok(())
    }

    /// Firecracker API: PUT /boot-source
    pub async fn configure_boot_source(
        &self,
        kernel_path: &PathBufJailer,
        boot_args: &str,
    ) -> Result<(), FirecrackerApiError> {
        let kernel_path = kernel_path.without_jail_root();
        let body = json!({
            "kernel_image_path": kernel_path,
            "boot_args": boot_args
        });

        let response = self
            .client
            .put("http://localhost/boot-source")
            .json(&body)
            .send()
            .await?;

        Self::handle_response(response).await?;
        Ok(())
    }

    /// Firecracker API: PUT /drives/:drive_id NOTE: path_on_host should be jail-relative, and is quite likely VersConfig::vm_root_drive_path
    pub async fn configure_drive(
        &self,
        drive_id: &str,
        path_on_host: impl AsRef<Path>,
        is_root_device: bool,
        is_read_only: bool,
    ) -> Result<(), FirecrackerApiError> {
        let body = json!({
            "drive_id": drive_id,
            "path_on_host": path_on_host.as_ref(),
            "is_root_device": is_root_device,
            "is_read_only": is_read_only
        });

        let response = self
            .client
            .put(&format!("http://localhost/drives/{}", drive_id))
            .json(&body)
            .send()
            .await?;

        Self::handle_response(response).await?;
        Ok(())
    }

    /// Firecracker API: PUT /network-interfaces/:iface_id
    pub async fn configure_network(
        &self,
        iface_id: &str,
        host_dev_name: &str,
        guest_mac: &MacAddr6,
    ) -> Result<(), FirecrackerApiError> {
        let guest_mac = guest_mac.to_string();
        let body = json!({
            "iface_id": iface_id,
            "host_dev_name": host_dev_name,
            "guest_mac": guest_mac
        });

        let response = self
            .client
            .put(&format!("http://localhost/network-interfaces/{}", iface_id))
            .json(&body)
            .send()
            .await?;

        Self::handle_response(response).await?;
        Ok(())
    }

    /// Firecracker API: PUT /actions {"action_type": "InstanceStart"}
    pub async fn start_instance(&self) -> Result<(), FirecrackerApiError> {
        let body = json!({
            "action_type": "InstanceStart"
        });

        let response = self
            .client
            .put("http://localhost/actions")
            .json(&body)
            .send()
            .await?;

        Self::handle_response(response).await?;
        Ok(())
    }

    /// Firecracker API: PATCH /drives/:drive_id — triggers re-read of block device size
    pub async fn patch_drive(
        &self,
        drive_id: &str,
        path_on_host: impl AsRef<Path>,
    ) -> Result<(), FirecrackerApiError> {
        let body = json!({
            "drive_id": drive_id,
            "path_on_host": path_on_host.as_ref(),
        });

        let response = self
            .client
            .patch(&format!("http://localhost/drives/{}", drive_id))
            .json(&body)
            .send()
            .await?;

        Self::handle_response(response).await?;
        Ok(())
    }

    /// Firecracker API: PATCH /vm {"state": "Paused"}
    pub async fn pause_instance(&self) -> Result<(), FirecrackerApiError> {
        let pause_config = json!({
            "state": "Paused"
        });

        let response = self
            .client
            .patch("http://localhost/vm")
            .json(&pause_config)
            .send()
            .await?;

        Self::handle_response(response).await?;
        Ok(())
    }

    /// Firecracker API: PATCH /vm {"state": "Resumed"}
    pub async fn resume_instance(&self) -> Result<(), FirecrackerApiError> {
        let resume_config = json!({
            "state": "Resumed"
        });

        let response = self
            .client
            .patch("http://localhost/vm")
            .json(&resume_config)
            .send()
            .await?;

        Self::handle_response(response).await?;
        Ok(())
    }

    /// Firecracker API: PUT /snapshot/create
    pub async fn create_snapshot(
        &self,
        mem_file_path: &PathBufJailer,
        state_file_path: &PathBufJailer,
    ) -> Result<(), FirecrackerApiError> {
        let request = json!({
            "snapshot_type": "Full",
            "snapshot_path": state_file_path.without_jail_root(),
            "mem_file_path": mem_file_path.without_jail_root(),
        });

        let response = self
            .client
            .put("http://localhost/snapshot/create")
            .json(&request)
            .send()
            .await?;

        Self::handle_response(response).await?;
        Ok(())
    }

    /// Firecracker API: PUT /snapshot/load
    ///
    /// NOTE: This does not resume the VM
    pub async fn load_snapshot(
        &self,
        state_file_path: &PathBufJailer,
        mem_file_path: &PathBufJailer,
        track_dirty_pages: bool,
    ) -> Result<(), FirecrackerApiError> {
        let request = json!({
            "snapshot_path": state_file_path.without_jail_root(),
            "mem_backend": {
                "backend_type": "File",
                "backend_path": mem_file_path.without_jail_root()
            },
            "track_dirty_pages": track_dirty_pages,
            "resume_vm": false,
        });

        let response = self
            .client
            .put("http://localhost/snapshot/load")
            .json(&request)
            .send()
            .await?;

        Self::handle_response(response).await?;
        Ok(())
    }

    /// Firecracker API: GET /
    pub async fn describe_instance(&self) -> Result<FirecrackerInstanceInfo, FirecrackerApiError> {
        let response = self.client.get("http://localhost/").send().await?;

        let response_text = Self::handle_response(response).await?;
        serde_json::from_str(&response_text).map_err(Into::into)
    }

    /// Firecracker API: PUT /logger
    pub async fn configure_logger(
        &self,
        log_path: &PathBufJailer,
        level: &FirecrackerProcessLoggerLogLevel,
    ) -> Result<(), FirecrackerApiError> {
        let log_path = log_path.without_jail_root();
        let request = json!({
            "log_path": log_path,
            "level": level.to_string(),
            "show_level": true,
            "show_log_origin": true
        });

        let response = self
            .client
            .put("http://localhost/logger")
            .json(&request)
            .send()
            .await?;

        Self::handle_response(response).await?;
        Ok(())
    }

    /// Firecracker API: PUT /vsock
    ///
    /// Configures a vsock device for host-to-guest communication.
    /// The vsock device allows the host to communicate with the guest
    /// via a Unix domain socket without network overhead.
    pub async fn configure_vsock(
        &self,
        vsock_id: &str,
        guest_cid: u64,
        uds_path: &PathBufJailer,
    ) -> Result<(), FirecrackerApiError> {
        let uds_path = uds_path.without_jail_root();
        let request = json!({
            "vsock_id": vsock_id,
            "guest_cid": guest_cid,
            "uds_path": uds_path
        });

        let response = self
            .client
            .put("http://localhost/vsock")
            .json(&request)
            .send()
            .await?;

        Self::handle_response(response).await?;
        Ok(())
    }
}
