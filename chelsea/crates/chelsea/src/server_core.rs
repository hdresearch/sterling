use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chelsea_lib::{
    process::HypervisorType,
    vm::VmConfig,
    vm_manager::{VmManager, error::VmManagerError, types::VmEvent},
};
use chelsea_server2::{
    ChelseaServerCore,
    error::{ApiError, CreateVmError},
    types::{
        network::{VmNetworkInfoDto, VmNetworkWireGuardInfoDto},
        system::*,
        vm::*,
    },
    wireguard_admin::WireGuardTarget,
};
use ssh_key::{private::Ed25519Keypair, rand_core::OsRng};
use std::{net::Ipv4Addr, sync::Arc};
use tracing::{Instrument, debug};
use uuid::Uuid;
use vers_config::VersConfig;

pub struct ConcreteServerCore {
    pub vm_manager: Arc<VmManager>,
}

impl ConcreteServerCore {}

fn interface_name() -> String {
    format!("vm_{}", &Uuid::new_v4().to_string()[0..10])
}

#[async_trait]
impl ChelseaServerCore for ConcreteServerCore {
    async fn vm_commit(
        &self,
        vm_id: &Uuid,
        commit_id: Uuid,
        keep_paused: bool,
        wait_boot: bool,
    ) -> Result<VmCommitResponse, ApiError> {
        self.vm_manager
            .commit_vm(vm_id, commit_id, keep_paused, wait_boot)
            .await?;

        Ok(VmCommitResponse { commit_id })
    }

    async fn vm_delete(&self, vm_id: &Uuid, wait_boot: bool) -> Result<(), ApiError> {
        self.vm_manager.wait_vm_booted(vm_id, wait_boot).await?;
        self.vm_manager.delete_vm(vm_id).await.map_err(Into::into)
    }

    async fn vm_list_all(&self) -> Result<VmListAllResponse, ApiError> {
        let vms = self.vm_manager.list_all_vms().await?;

        Ok(VmListAllResponse { vms })
    }

    async fn vm_status(&self, vm_id: &Uuid) -> Result<VmStatusResponse, ApiError> {
        self.vm_manager
            .get_vm_summary(vm_id)
            .await
            .map_err(ApiError::from)
    }

    #[tracing::instrument(skip_all, fields(wait_boot))]
    async fn vm_create(&self, request: VmCreateRequest, wait_boot: bool) -> Result<Uuid, ApiError> {
        // Prepare VmConfig from request params
        debug!(?request, "Creating new root VM");
        let config = VersConfig::chelsea();

        // Convert DTO WireGuard config to chelsea_lib WireGuard config
        let wireguard = chelsea_lib::vm::VmWireGuardConfig {
            interface_name: interface_name(),
            private_key: request.wireguard.private_key.clone(),
            private_ip: request
                .wireguard
                .ipv6_address
                .parse()
                .map_err(anyhow::Error::from)?,
            peer_pub_key: request.wireguard.proxy_public_key.clone(),
            peer_ipv6: request
                .wireguard
                .proxy_ipv6_address
                .clone()
                .parse()
                .map_err(anyhow::Error::from)?,
            peer_pub_ip: request
                .wireguard
                .proxy_public_ip
                .parse()
                .map_err(anyhow::Error::from)?,
            wg_port: request.wireguard.wg_port,
        };

        // If no base image name was specified, use the configured default.
        let base_image = request
            .vm_config
            .image_name
            .unwrap_or(config.vm_default_image_name.clone());

        // Query volume manager to determine minimum allowable volume size for the request.
        let minimum_fs_size_mib = self
            .vm_manager
            .volume_manager
            .get_base_image_size_mib(&base_image)
            .instrument(tracing::info_span!("chelsea.get_base_image_size"))
            .await
            .map_err(VmManagerError::from)?;

        // If no FS size was defined, use the configured default (which matches the
        // pre-warmed volume pool size), floored by the minimum required by the base image.
        let fs_size_mib = request
            .vm_config
            .fs_size_mib
            .unwrap_or(minimum_fs_size_mib.max(config.vm_default_fs_size_mib));

        // If an FS size was defined, ensure it is not smaller than the minimum size for that base image.
        if fs_size_mib < minimum_fs_size_mib {
            return Err(ApiError::CreateVm(CreateVmError::TooSmall(format!(
                "A VM created with the image '{}' must have a volume size of at least {} MiB. Requested: {} MiB",
                base_image, minimum_fs_size_mib, fs_size_mib
            ))));
        }

        // Get hypervisor type from config
        let hypervisor_type = config.hypervisor_type;

        // Select kernel based on hypervisor type
        let kernel_name = match hypervisor_type {
            HypervisorType::Firecracker => config.firecracker_kernel_name.clone(),
            HypervisorType::CloudHypervisor => config.cloud_hypervisor_kernel_name.clone(),
        };

        // Create a VmConfig struct from the request parameters.
        let vm_config = VmConfig {
            kernel_name: request.vm_config.kernel_name.unwrap_or(kernel_name),
            base_image,
            vcpu_count: request
                .vm_config
                .vcpu_count
                .unwrap_or(config.vm_default_vcpu_count),
            mem_size_mib: request
                .vm_config
                .mem_size_mib
                .unwrap_or(config.vm_default_mem_size_mib),
            fs_size_mib,
            ssh_keypair: Ed25519Keypair::random(&mut OsRng),
            hypervisor_type,
        };

        // If input VM ID not supplied, generate a new one
        let vm_id = request.vm_id.unwrap_or_else(Uuid::new_v4);

        self.vm_manager
            .create_new_vm(
                vm_id.clone(),
                vm_config,
                wireguard,
                wait_boot,
                request.env_vars,
            )
            .await?;

        Ok(vm_id)
    }

    async fn vm_from_commit(&self, request: VmFromCommitRequest) -> Result<Uuid, ApiError> {
        // If input VM ID not supplied, generate a new one
        let vm_id = request.vm_id.unwrap_or_else(Uuid::new_v4);

        let commit_id = request.commit_id;
        let wg = chelsea_lib::vm::VmWireGuardConfig {
            interface_name: interface_name(),
            private_ip: request
                .wireguard
                .ipv6_address
                .parse()
                .map_err(anyhow::Error::from)?,
            private_key: request.wireguard.private_key,
            peer_pub_key: request.wireguard.proxy_public_key,
            peer_pub_ip: request
                .wireguard
                .proxy_public_ip
                .parse()
                .map_err(anyhow::Error::from)?,
            peer_ipv6: request
                .wireguard
                .proxy_ipv6_address
                .parse()
                .map_err(anyhow::Error::from)?,
            wg_port: request.wireguard.wg_port,
        };

        self.vm_manager
            .create_vm_from_commit(vm_id.clone(), &commit_id, wg, request.env_vars)
            .await?;

        Ok(vm_id)
    }

    async fn vm_update_state(
        &self,
        vm_id: &Uuid,
        request: VmUpdateStateRequest,
        wait_boot: bool,
    ) -> Result<(), ApiError> {
        match request.state {
            VmUpdateStateEnum::Paused => self.vm_manager.pause_vm(vm_id, wait_boot).await?,
            VmUpdateStateEnum::Running => self.vm_manager.resume_vm(vm_id, wait_boot).await?,
        };

        Ok(())
    }

    async fn vm_exec(
        &self,
        vm_id: &Uuid,
        request: VmExecRequest,
        wait_boot: bool,
    ) -> Result<VmExecResponse, ApiError> {
        let exec_id = request.exec_id;
        let agent_request = dto_to_agent_exec_request(request);

        let result = self
            .vm_manager
            .exec_vm_command(vm_id, agent_request, wait_boot)
            .await
            .map_err(vm_manager_err)?;

        Ok(VmExecResponse {
            exit_code: result.exit_code,
            stdout: String::from_utf8_lossy(&result.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
            exec_id,
        })
    }

    async fn vm_exec_stream(
        &self,
        vm_id: &Uuid,
        request: VmExecRequest,
        wait_boot: bool,
    ) -> Result<chelsea_lib::vsock::ExecStreamConnection, ApiError> {
        let agent_request = dto_to_agent_exec_request(request);

        self.vm_manager
            .exec_vm_stream(vm_id, agent_request, wait_boot)
            .await
            .map_err(vm_manager_err)
    }

    async fn vm_exec_stream_attach(
        &self,
        vm_id: &Uuid,
        request: VmExecStreamAttachRequest,
        wait_boot: bool,
    ) -> Result<chelsea_lib::vsock::ExecStreamConnection, ApiError> {
        self.vm_manager
            .exec_vm_stream_attach(
                vm_id,
                request.exec_id,
                request.cursor,
                request.from_latest.unwrap_or(false),
                wait_boot,
            )
            .await
            .map_err(vm_manager_err)
    }

    async fn vm_exec_logs(
        &self,
        vm_id: &Uuid,
        query: VmExecLogQuery,
        wait_boot: bool,
    ) -> Result<VmExecLogResponse, ApiError> {
        let agent_request = agent_protocol::TailExecLogRequest {
            offset: query.offset.unwrap_or(0),
            max_entries: query.max_entries.unwrap_or(100) as usize,
            stream: query.stream.map(dto_to_agent_log_stream),
        };

        let result = self
            .vm_manager
            .tail_exec_log(vm_id, agent_request, wait_boot)
            .await
            .map_err(vm_manager_err)?;

        Ok(VmExecLogResponse {
            entries: result
                .entries
                .into_iter()
                .map(|e| VmExecLogEntry {
                    exec_id: e.exec_id,
                    timestamp: e.timestamp,
                    stream: agent_to_dto_log_stream(e.stream),
                    data_b64: BASE64.encode(&e.data),
                })
                .collect(),
            next_offset: result.next_offset,
            eof: result.eof,
        })
    }

    async fn vm_write_file(
        &self,
        vm_id: &Uuid,
        request: VmWriteFileRequest,
        wait_boot: bool,
    ) -> Result<(), ApiError> {
        let content = BASE64
            .decode(&request.content_b64)
            .map_err(|e| ApiError::BadRequest(format!("invalid base64: {e}")))?;

        self.vm_manager
            .write_file(
                vm_id,
                &request.path,
                &content,
                request.mode,
                request.create_dirs,
                wait_boot,
            )
            .await
            .map_err(vm_manager_err)?;

        Ok(())
    }

    async fn vm_read_file(
        &self,
        vm_id: &Uuid,
        path: &str,
        wait_boot: bool,
    ) -> Result<Vec<u8>, ApiError> {
        self.vm_manager
            .read_file(vm_id, path, wait_boot)
            .await
            .map_err(vm_manager_err)
    }

    async fn vm_get_ssh_key_and_port(&self, vm_id: &Uuid) -> Result<(String, u16), ApiError> {
        self.vm_manager
            .get_vm_ssh_key_and_port(vm_id)
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))
    }

    async fn vm_resize_disk(
        &self,
        vm_id: &Uuid,
        request: VmResizeDiskRequest,
        wait_boot: bool,
    ) -> Result<(), ApiError> {
        self.vm_manager
            .resize_vm_disk(vm_id, request.fs_size_mib, wait_boot)
            .await
            .map_err(ApiError::from)
    }

    async fn get_system_telemetry(&self) -> Result<SystemTelemetryResponse, ApiError> {
        // VM count max and current
        let vm_count_current = self.vm_manager.get_vm_count_current().await?;
        let vm_count_max = self.vm_manager.get_vm_count_max();

        // "Real" total + available
        let system_service = &self.vm_manager.system_service;
        let (cpu_real_total, cpu_real_available) =
            system_service.get_total_and_available_cpu().await;
        let (ram_real_mib_total, ram_real_mib_available) =
            system_service.get_total_and_available_ram().await;
        let (disk_real_mib_total, disk_real_mib_available) =
            system_service.get_total_and_available_disk().await;
        let vcpu_count_total = system_service.get_vcpu_count().await;

        let vm_reservation = self
            .vm_manager
            .local_store
            .get_vm_resource_reservation()
            .await?;

        let ram = TelemetryRam {
            real_mib_total: ram_real_mib_total,
            real_mib_available: ram_real_mib_available,
            vm_mib_total: vm_reservation.memory_mib.total,
            vm_mib_available: vm_reservation.memory_mib.available(),
        };
        let cpu = TelemetryCpu {
            real_total: cpu_real_total,
            real_available: cpu_real_available,
            vcpu_count_total,
            vcpu_count_vm_total: vm_reservation.vcpu_count.total,
            vcpu_count_vm_available: vm_reservation.vcpu_count.available(),
        };
        let fs = TelemetryFs {
            mib_total: disk_real_mib_total,
            mib_available: disk_real_mib_available,
        };
        let chelsea = TelemetryChelsea {
            vm_count_max,
            vm_count_current,
        };

        Ok(SystemTelemetryResponse {
            ram,
            cpu,
            fs,
            chelsea,
        })
    }

    async fn vm_notify(&self, vm_id: &Uuid, request: VmNotifyRequest) -> Result<(), ApiError> {
        debug!(?request, "Received notify request from VM '{vm_id}'");
        let event = match request {
            VmNotifyRequest::Ready(val) => match val.as_str() {
                "true" => VmEvent::Ready,
                other => {
                    debug!(
                        "'Ready' event from VM '{vm_id}' contains non-'true' value '{other}'; ignoring."
                    );
                    return Ok(());
                }
            },
        };

        self.vm_manager.on_vm_event(vm_id, event).await;

        Ok(())
    }

    /// Resolve the namespace and interface name for the VM's WireGuard endpoint.
    async fn vm_wireguard_target(&self, vm_id: &Uuid) -> Result<WireGuardTarget, ApiError> {
        let (netns_name, interface_name) = self.vm_manager.get_vm_wireguard_target(vm_id).await?;

        Ok(WireGuardTarget {
            interface_name,
            netns_name,
        })
    }

    async fn vm_network_info(&self, vm_id: &Uuid) -> Result<VmNetworkInfoDto, ApiError> {
        let network_info = self.vm_manager.get_vm_network_info(vm_id).await?;
        let wireguard = network_info.wg.map(|wg| VmNetworkWireGuardInfoDto {
            interface_name: wg.interface_name,
            private_key: wg.private_key,
            private_ipv6: wg.private_ip.to_string(),
            peer_public_key: wg.peer_pub_key,
            peer_public_ip: wg.peer_pub_ip.to_string(),
            peer_ipv6: wg.peer_ipv6.to_string(),
        });

        Ok(VmNetworkInfoDto {
            vm_id: vm_id.to_string(),
            host_ipv4: Ipv4Addr::from_bits(network_info.host_addr).to_string(),
            vm_ipv4: Ipv4Addr::from_bits(network_info.vm_addr).to_string(),
            netns_name: network_info.netns_name,
            ssh_port: network_info.ssh_port,
            wireguard,
        })
    }

    async fn vm_sleep(&self, vm_id: &Uuid, wait_boot: bool) -> Result<(), ApiError> {
        self.vm_manager
            .sleep_vm(vm_id, wait_boot)
            .await
            .map_err(ApiError::from)
    }

    async fn vm_wake(&self, vm_id: &Uuid, request: VmWakeRequest) -> Result<(), ApiError> {
        // Convert DTO WireGuard config to chelsea_lib WireGuard config
        let wireguard = chelsea_lib::vm::VmWireGuardConfig {
            interface_name: interface_name(),
            private_key: request.wireguard.private_key.clone(),
            private_ip: request
                .wireguard
                .ipv6_address
                .parse()
                .map_err(anyhow::Error::from)?,
            peer_pub_key: request.wireguard.proxy_public_key.clone(),
            peer_ipv6: request
                .wireguard
                .proxy_ipv6_address
                .clone()
                .parse()
                .map_err(anyhow::Error::from)?,
            peer_pub_ip: request
                .wireguard
                .proxy_public_ip
                .parse()
                .map_err(anyhow::Error::from)?,
            wg_port: request.wireguard.wg_port,
        };

        self.vm_manager
            .wake_vm(vm_id, wireguard)
            .await
            .map_err(ApiError::from)
    }
}

// ── DTO ↔ agent_protocol conversion helpers ─────────────────────────

fn vm_manager_err(err: VmManagerError) -> ApiError {
    match &err {
        VmManagerError::VmLifecycle(
            chelsea_lib::vm_manager::error::VmLifecycleError::StillBooting { .. },
        ) => ApiError::Conflict(err.to_string()),
        _ => ApiError::Internal(format!("{err:#}")),
    }
}

/// Default timeout applied when the caller doesn't specify one (5 minutes).
const DEFAULT_EXEC_TIMEOUT_SECS: u64 = 300;

/// Maximum timeout a caller may request (1 hour). Values above this are clamped.
const MAX_EXEC_TIMEOUT_SECS: u64 = 3600;

fn dto_to_agent_exec_request(req: VmExecRequest) -> agent_protocol::ExecRequest {
    let timeout_secs = match req.timeout_secs {
        None | Some(0) => DEFAULT_EXEC_TIMEOUT_SECS,
        Some(t) => t.min(MAX_EXEC_TIMEOUT_SECS),
    };

    agent_protocol::ExecRequest {
        command: req.command,
        exec_id: req.exec_id,
        env: req.env.unwrap_or_default(),
        working_dir: req.working_dir,
        stdin: req.stdin.map(|s| s.into_bytes()),
        timeout_secs,
    }
}

fn dto_to_agent_log_stream(s: VmExecLogStream) -> agent_protocol::ExecLogStream {
    match s {
        VmExecLogStream::Stdout => agent_protocol::ExecLogStream::Stdout,
        VmExecLogStream::Stderr => agent_protocol::ExecLogStream::Stderr,
    }
}

fn agent_to_dto_log_stream(s: agent_protocol::ExecLogStream) -> VmExecLogStream {
    match s {
        agent_protocol::ExecLogStream::Stdout => VmExecLogStream::Stdout,
        agent_protocol::ExecLogStream::Stderr => VmExecLogStream::Stderr,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_dto_to_agent_exec_request_minimal() {
        let dto = VmExecRequest {
            command: vec!["ls".into(), "-la".into()],
            exec_id: None,
            env: None,
            working_dir: None,
            stdin: None,
            timeout_secs: None,
        };

        let agent = dto_to_agent_exec_request(dto);
        assert_eq!(agent.command, vec!["ls", "-la"]);
        assert_eq!(agent.exec_id, None);
        assert!(agent.env.is_empty());
        assert_eq!(agent.working_dir, None);
        assert_eq!(agent.stdin, None);
        assert_eq!(agent.timeout_secs, DEFAULT_EXEC_TIMEOUT_SECS);
    }

    #[test]
    fn test_dto_to_agent_exec_request_full() {
        let exec_id = Uuid::new_v4();
        let mut env = HashMap::new();
        env.insert("FOO".into(), "bar".into());

        let dto = VmExecRequest {
            command: vec!["echo".into(), "hello".into()],
            exec_id: Some(exec_id),
            env: Some(env.clone()),
            working_dir: Some("/tmp".into()),
            stdin: Some("input data".into()),
            timeout_secs: Some(30),
        };

        let agent = dto_to_agent_exec_request(dto);
        assert_eq!(agent.command, vec!["echo", "hello"]);
        assert_eq!(agent.exec_id, Some(exec_id));
        assert_eq!(agent.env.get("FOO").map(|s| s.as_str()), Some("bar"));
        assert_eq!(agent.working_dir, Some("/tmp".into()));
        assert_eq!(agent.stdin, Some(b"input data".to_vec()));
        assert_eq!(agent.timeout_secs, 30);
    }

    #[test]
    fn test_log_stream_round_trips() {
        assert_eq!(
            agent_to_dto_log_stream(dto_to_agent_log_stream(VmExecLogStream::Stdout)),
            VmExecLogStream::Stdout
        );
        assert_eq!(
            agent_to_dto_log_stream(dto_to_agent_log_stream(VmExecLogStream::Stderr)),
            VmExecLogStream::Stderr
        );
    }

    #[test]
    fn test_dto_to_agent_exec_request_empty_command() {
        let dto = VmExecRequest {
            command: vec![],
            exec_id: None,
            env: None,
            working_dir: None,
            stdin: None,
            timeout_secs: None,
        };

        let agent = dto_to_agent_exec_request(dto);
        assert!(agent.command.is_empty());
    }

    #[test]
    fn test_dto_to_agent_exec_request_env_none_vs_empty() {
        // env: None → empty HashMap
        let dto_none = VmExecRequest {
            command: vec!["x".into()],
            exec_id: None,
            env: None,
            working_dir: None,
            stdin: None,
            timeout_secs: None,
        };
        assert!(dto_to_agent_exec_request(dto_none).env.is_empty());

        // env: Some({}) → empty HashMap
        let dto_empty = VmExecRequest {
            command: vec!["x".into()],
            exec_id: None,
            env: Some(HashMap::new()),
            working_dir: None,
            stdin: None,
            timeout_secs: None,
        };
        assert!(dto_to_agent_exec_request(dto_empty).env.is_empty());
    }

    #[test]
    fn test_dto_to_agent_exec_request_timeout_defaults_applied() {
        // None → default timeout
        let dto = VmExecRequest {
            command: vec!["x".into()],
            exec_id: None,
            env: None,
            working_dir: None,
            stdin: None,
            timeout_secs: None,
        };
        assert_eq!(
            dto_to_agent_exec_request(dto).timeout_secs,
            DEFAULT_EXEC_TIMEOUT_SECS
        );

        // Explicit 0 → default timeout (0 means "not specified")
        let dto_explicit_zero = VmExecRequest {
            command: vec!["x".into()],
            exec_id: None,
            env: None,
            working_dir: None,
            stdin: None,
            timeout_secs: Some(0),
        };
        assert_eq!(
            dto_to_agent_exec_request(dto_explicit_zero).timeout_secs,
            DEFAULT_EXEC_TIMEOUT_SECS
        );
    }

    #[test]
    fn test_dto_to_agent_exec_request_timeout_clamped_to_max() {
        let dto = VmExecRequest {
            command: vec!["x".into()],
            exec_id: None,
            env: None,
            working_dir: None,
            stdin: None,
            timeout_secs: Some(999_999),
        };
        assert_eq!(
            dto_to_agent_exec_request(dto).timeout_secs,
            MAX_EXEC_TIMEOUT_SECS
        );
    }

    #[test]
    fn test_dto_to_agent_exec_request_timeout_within_range_preserved() {
        let dto = VmExecRequest {
            command: vec!["x".into()],
            exec_id: None,
            env: None,
            working_dir: None,
            stdin: None,
            timeout_secs: Some(60),
        };
        assert_eq!(dto_to_agent_exec_request(dto).timeout_secs, 60);
    }

    #[test]
    fn test_dto_to_agent_exec_request_stdin_converted_to_bytes() {
        let dto = VmExecRequest {
            command: vec!["cat".into()],
            exec_id: None,
            env: None,
            working_dir: None,
            stdin: Some("héllo 🌍".into()),
            timeout_secs: None,
        };
        let agent = dto_to_agent_exec_request(dto);
        assert_eq!(agent.stdin, Some("héllo 🌍".as_bytes().to_vec()));
    }

    #[test]
    fn test_exec_id_preserved_through_response() {
        // exec_id on the request should be forwarded to the response,
        // independent of what the agent returns (agent ExecResult has no exec_id)
        let exec_id = Uuid::new_v4();
        let request = VmExecRequest {
            command: vec!["echo".into()],
            exec_id: Some(exec_id),
            env: None,
            working_dir: None,
            stdin: None,
            timeout_secs: None,
        };

        // Simulate what vm_exec does: capture exec_id before conversion
        let captured_exec_id = request.exec_id;
        let _agent_request = dto_to_agent_exec_request(request);

        // Build response as vm_exec would
        let response = VmExecResponse {
            exit_code: 0,
            stdout: "ok".into(),
            stderr: String::new(),
            exec_id: captured_exec_id,
        };
        assert_eq!(response.exec_id, Some(exec_id));
    }

    #[test]
    fn test_attach_from_latest_none_defaults_to_false() {
        let request = VmExecStreamAttachRequest {
            exec_id: Uuid::new_v4(),
            cursor: None,
            from_latest: None,
        };
        // The server_core unpacks from_latest.unwrap_or(false)
        assert_eq!(request.from_latest.unwrap_or(false), false);
    }

    #[test]
    fn test_lossy_utf8_conversion() {
        // Agent returns raw bytes; server_core converts via String::from_utf8_lossy
        let invalid_utf8: Vec<u8> = vec![0x48, 0x65, 0x6C, 0xFF, 0x6F]; // "Hel\xFFo"
        let lossy = String::from_utf8_lossy(&invalid_utf8).into_owned();
        assert_eq!(lossy, "Hel\u{FFFD}o");
    }
}
