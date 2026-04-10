use std::time::Duration;

use reqwest::Client;
use uuid::Uuid;
use vers_config::VersConfig;

use super::config::PathBufJailer;
use super::error::CloudHypervisorApiError;
use super::types::{
    CloudHypervisorVmConfig, CloudHypervisorVmInfo, ConsoleConfig, CpusConfig, DiskConfig,
    KernelConfig, MemoryConfig, NetConfig, PayloadConfig, SerialConfig, VmResizeConfig,
    VmResizeDiskConfig, VmRestoreConfig, VmSnapshotConfig, VsockConfig,
};

/// HTTP client for communicating with the Cloud Hypervisor API
#[derive(Debug)]
pub struct CloudHypervisorApi {
    /// The path to the socket; created by exec'ing ch-jailer/cloud-hypervisor
    pub socket_path: PathBufJailer,
    /// A reqwest client holding a connection pool to the socket
    client: Client,
}

impl CloudHypervisorApi {
    /// Creates a new CloudHypervisorApi client for a Unix socket
    pub fn new(vm_id: Uuid) -> Result<Self, CloudHypervisorApiError> {
        // Socket is in the VM's working directory
        let socket_path = PathBufJailer::new(vm_id, std::path::PathBuf::from("run/ch.sock"));
        let api_timeout = Duration::from_secs(VersConfig::chelsea().firecracker_api_timeout_secs);
        let client = reqwest::Client::builder()
            .unix_socket(socket_path.with_jail_root())
            .timeout(api_timeout)
            .build()?;

        Ok(Self {
            socket_path,
            client,
        })
    }

    async fn handle_response(
        response: reqwest::Response,
    ) -> Result<String, CloudHypervisorApiError> {
        if !response.status().is_success() {
            let status_code = response.status();
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            Err(CloudHypervisorApiError::ResponseNotOk {
                status_code,
                error_body,
            })
        } else {
            Ok(response.text().await?)
        }
    }

    /// Cloud Hypervisor API: PUT /api/v1/vm.create
    /// Creates a VM with the given configuration
    pub async fn create_vm(
        &self,
        cpus: &CpusConfig,
        memory: &MemoryConfig,
        kernel: &KernelConfig,
        disks: &[DiskConfig],
        net: &[NetConfig],
        serial_log_path: Option<String>,
        vsock: Option<&VsockConfig>,
    ) -> Result<(), CloudHypervisorApiError> {
        let payload = PayloadConfig {
            kernel: Some(kernel.path.clone()),
            cmdline: kernel.cmdline.clone(),
        };

        // Configure serial console to output to TTY (cloud-hypervisor's stdout)
        let serial = Some(SerialConfig {
            file: serial_log_path,
            mode: Some("Tty".to_string()),
        });

        let console = Some(ConsoleConfig {
            mode: "Off".to_string(),
        });

        let config = CloudHypervisorVmConfig {
            cpus: Some(cpus.clone()),
            memory: Some(memory.clone()),
            payload: Some(payload),
            disks: Some(disks.to_vec()),
            net: Some(net.to_vec()),
            serial,
            console,
            vsock: vsock.cloned(),
        };

        // Debug: log the config being sent
        if let Ok(json) = serde_json::to_string_pretty(&config) {
            tracing::debug!("Sending VM config to cloud-hypervisor:\n{}", json);
        }

        let response = self
            .client
            .put("http://localhost/api/v1/vm.create")
            .json(&config)
            .send()
            .await?;

        Self::handle_response(response).await?;
        Ok(())
    }

    /// Cloud Hypervisor API: PUT /api/v1/vm.boot
    /// Boots the VM
    pub async fn boot_vm(&self) -> Result<(), CloudHypervisorApiError> {
        let response = self
            .client
            .put("http://localhost/api/v1/vm.boot")
            .send()
            .await?;

        Self::handle_response(response).await?;
        Ok(())
    }

    /// Cloud Hypervisor API: PUT /api/v1/vm.shutdown
    /// Shuts down the VM
    pub async fn shutdown_vm(&self) -> Result<(), CloudHypervisorApiError> {
        let response = self
            .client
            .put("http://localhost/api/v1/vm.shutdown")
            .send()
            .await?;

        Self::handle_response(response).await?;
        Ok(())
    }

    /// Cloud Hypervisor API: GET /api/v1/vm.info
    /// Gets VM information
    pub async fn vm_info(&self) -> Result<CloudHypervisorVmInfo, CloudHypervisorApiError> {
        let response = self
            .client
            .get("http://localhost/api/v1/vm.info")
            .send()
            .await?;

        let response_text = Self::handle_response(response).await?;
        serde_json::from_str(&response_text).map_err(Into::into)
    }

    /// Cloud Hypervisor API: PUT /api/v1/vm.pause
    /// Pauses the VM
    pub async fn pause_vm(&self) -> Result<(), CloudHypervisorApiError> {
        let response = self
            .client
            .put("http://localhost/api/v1/vm.pause")
            .send()
            .await?;

        Self::handle_response(response).await?;
        Ok(())
    }

    /// Cloud Hypervisor API: PUT /api/v1/vm.resume
    /// Resumes the VM
    pub async fn resume_vm(&self) -> Result<(), CloudHypervisorApiError> {
        let response = self
            .client
            .put("http://localhost/api/v1/vm.resume")
            .send()
            .await?;

        Self::handle_response(response).await?;
        Ok(())
    }

    /// Cloud Hypervisor API: PUT /api/v1/vm.snapshot
    /// Takes a snapshot of the VM to the specified destination directory
    pub async fn snapshot_vm(
        &self,
        config: &VmSnapshotConfig,
    ) -> Result<(), CloudHypervisorApiError> {
        let response = self
            .client
            .put("http://localhost/api/v1/vm.snapshot")
            .json(config)
            .send()
            .await?;

        Self::handle_response(response).await?;
        Ok(())
    }

    /// Cloud Hypervisor API: PUT /api/v1/vm.restore
    /// Restores a VM from a snapshot
    pub async fn restore_vm(
        &self,
        config: &VmRestoreConfig,
    ) -> Result<(), CloudHypervisorApiError> {
        let response = self
            .client
            .put("http://localhost/api/v1/vm.restore")
            .json(config)
            .send()
            .await?;

        Self::handle_response(response).await?;
        Ok(())
    }

    /// Cloud Hypervisor API: PUT /api/v1/vm.resize
    /// Resizes the VM (CPUs or memory hotplug)
    pub async fn resize_vm(&self, config: &VmResizeConfig) -> Result<(), CloudHypervisorApiError> {
        let response = self
            .client
            .put("http://localhost/api/v1/vm.resize")
            .json(config)
            .send()
            .await?;

        Self::handle_response(response).await?;
        Ok(())
    }

    /// Cloud Hypervisor API: PUT /api/v1/vm.resize-disk
    /// Notifies the guest that a block device has been resized.
    /// The guest kernel will see the new size and can resize the filesystem.
    pub async fn resize_disk(
        &self,
        config: &VmResizeDiskConfig,
    ) -> Result<(), CloudHypervisorApiError> {
        // Cloud hypervisor seems to support resizing guest block devices, only
        // if the backing space is a "raw file" on the host fs, for example qcow2.
        // Issue: https://github.com/cloud-hypervisor/cloud-hypervisor/issues/7923
        // PR: https://github.com/cloud-hypervisor/cloud-hypervisor/pull/7948
        //
        // Implementation of cloud-hypervisor's resize disk endpoint:
        // let response = self
        //     .client
        //     .put("http://localhost/api/v1/vm.resize-disk")
        //     .json(config)
        //     .send()
        //     .await?;
        //
        // Self::handle_response(response).await?;
        // Ok(())

        let _ = config;
        Err(CloudHypervisorApiError::OperationNotSupported)
    }
}
