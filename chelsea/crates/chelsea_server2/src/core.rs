use crate::{error::ApiError, types::vm::*, wireguard_admin::WireGuardTarget};
use chelsea_lib::vsock::ExecStreamConnection;
use dto_lib::chelsea_server2::vm::VmWriteFileRequest;
use dto_lib::chelsea_server2::{network::VmNetworkInfoDto, system::SystemTelemetryResponse};
use uuid::Uuid;

/// A trait that must be implemented by a struct passed to an instance of ChelseaServer
#[async_trait::async_trait]
pub trait ChelseaServerCore: Send + Sync {
    async fn vm_commit(
        &self,
        vm_id: &Uuid,
        commit_id: Uuid,
        keep_paused: bool,
        wait_boot: bool,
    ) -> Result<VmCommitResponse, ApiError>;
    async fn vm_delete(&self, vm_id: &Uuid, wait_boot: bool) -> Result<(), ApiError>;
    async fn vm_list_all(&self) -> Result<VmListAllResponse, ApiError>;
    async fn vm_status(&self, vm_id: &Uuid) -> Result<VmStatusResponse, ApiError>;
    async fn vm_create(&self, request: VmCreateRequest, wait_boot: bool) -> Result<Uuid, ApiError>;
    async fn vm_from_commit(&self, request: VmFromCommitRequest) -> Result<Uuid, ApiError>;
    async fn vm_update_state(
        &self,
        vm_id: &Uuid,
        request: VmUpdateStateRequest,
        wait_boot: bool,
    ) -> Result<(), ApiError>;
    async fn vm_exec(
        &self,
        vm_id: &Uuid,
        request: VmExecRequest,
        wait_boot: bool,
    ) -> Result<VmExecResponse, ApiError>;
    async fn vm_exec_stream(
        &self,
        vm_id: &Uuid,
        request: VmExecRequest,
        wait_boot: bool,
    ) -> Result<ExecStreamConnection, ApiError>;
    async fn vm_exec_stream_attach(
        &self,
        vm_id: &Uuid,
        request: VmExecStreamAttachRequest,
        wait_boot: bool,
    ) -> Result<ExecStreamConnection, ApiError>;
    async fn vm_exec_logs(
        &self,
        vm_id: &Uuid,
        query: VmExecLogQuery,
        wait_boot: bool,
    ) -> Result<VmExecLogResponse, ApiError>;
    async fn vm_write_file(
        &self,
        vm_id: &Uuid,
        request: VmWriteFileRequest,
        wait_boot: bool,
    ) -> Result<(), ApiError>;
    async fn vm_read_file(
        &self,
        vm_id: &Uuid,
        path: &str,
        wait_boot: bool,
    ) -> Result<Vec<u8>, ApiError>;
    async fn vm_get_ssh_key_and_port(&self, vm_id: &Uuid) -> Result<(String, u16), ApiError>;
    async fn vm_resize_disk(
        &self,
        vm_id: &Uuid,
        request: VmResizeDiskRequest,
        wait_boot: bool,
    ) -> Result<(), ApiError>;
    async fn vm_notify(&self, vm_id: &Uuid, request: VmNotifyRequest) -> Result<(), ApiError>;
    async fn get_system_telemetry(&self) -> Result<SystemTelemetryResponse, ApiError>;
    async fn vm_wireguard_target(&self, vm_id: &Uuid) -> Result<WireGuardTarget, ApiError>;
    async fn vm_network_info(&self, vm_id: &Uuid) -> Result<VmNetworkInfoDto, ApiError>;
    async fn vm_sleep(&self, vm_id: &Uuid, wait_boot: bool) -> Result<(), ApiError>;
    async fn vm_wake(&self, vm_id: &Uuid, request: VmWakeRequest) -> Result<(), ApiError>;
}
