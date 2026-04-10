use crate::error::{ApiError, ErrorStatusCode};
use crate::{
    types::network::VmNetworkInfoDto,
    wireguard_admin::{
        self, AddWireGuardPeerRequest, WireGuardInterfaceDto, WireGuardPeerDto, WireGuardTarget,
    },
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use chelsea_lib::network::linux::namespace::netns_exec;
use serde::Deserialize;
use std::{process::Output, sync::Arc};
use tracing::info;
use utoipa::OpenApi;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;
use vers_config::VersConfig;

fn internal_error(message: impl Into<String>) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, message.into())
}

fn bad_request(message: impl Into<String>) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, message.into())
}

fn api_error_response(error: ApiError) -> (StatusCode, String) {
    (error.status_code(), error.to_string())
}

#[derive(Deserialize)]
pub struct VmWireGuardPeerPath {
    vm_id: Uuid,
    public_key: String,
}

enum WgCliAction<'a> {
    List,
    Add(&'a AddWireGuardPeerRequest),
    Delete(&'a str),
}

async fn run_wgcli_action(
    target: &WireGuardTarget,
    action: WgCliAction<'_>,
) -> Result<WireGuardInterfaceDto, (StatusCode, String)> {
    match action {
        WgCliAction::List => wg_show_dump(target).await,
        WgCliAction::Add(request) => {
            let allowed_ips = request
                .allowed_ips
                .iter()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(",");

            if allowed_ips.is_empty() {
                return Err(bad_request("allowed_ips must contain at least one entry"));
            }

            let mut args = vec![
                "wg".to_string(),
                "set".to_string(),
                target.interface_name.clone(),
                "peer".to_string(),
                request.public_key.clone(),
            ];

            if let Some(preshared_key) = &request.preshared_key {
                args.push("preshared-key".to_string());
                args.push(preshared_key.clone());
            }
            if let Some(endpoint) = &request.endpoint {
                args.push("endpoint".to_string());
                args.push(endpoint.clone());
            }
            if let Some(interval) = request.persistent_keepalive_interval {
                args.push("persistent-keepalive".to_string());
                args.push(interval.to_string());
            }

            args.push("allowed-ips".to_string());
            args.push(allowed_ips);

            exec_wg_command(target, args).await?;
            wg_show_dump(target).await
        }
        WgCliAction::Delete(public_key) => {
            let args = vec![
                "wg".to_string(),
                "set".to_string(),
                target.interface_name.clone(),
                "peer".to_string(),
                public_key.to_string(),
                "remove".to_string(),
            ];

            exec_wg_command(target, args).await?;
            wg_show_dump(target).await
        }
    }
}

async fn exec_wg_command(
    target: &WireGuardTarget,
    args: Vec<String>,
) -> Result<Output, (StatusCode, String)> {
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    netns_exec(&target.netns_name, &arg_refs)
        .await
        .map_err(map_netns_exec_error)
}

fn map_netns_exec_error(error: anyhow::Error) -> (StatusCode, String) {
    let msg = error.to_string();
    if msg.contains("No such device") || msg.contains("Device not found") {
        (StatusCode::NOT_FOUND, msg)
    } else if msg.contains("peer not found") || msg.contains("Unable to find peer") {
        (StatusCode::NOT_FOUND, msg)
    } else if msg.contains("invalid") || msg.contains("allowed_ips") {
        bad_request(msg)
    } else {
        internal_error(msg)
    }
}

async fn wg_show_dump(
    target: &WireGuardTarget,
) -> Result<WireGuardInterfaceDto, (StatusCode, String)> {
    let args = vec![
        "wg".to_string(),
        "show".to_string(),
        target.interface_name.clone(),
        "dump".to_string(),
    ];

    let output = exec_wg_command(target, args).await?;

    parse_wg_dump(&target.interface_name, &output.stdout)
        .map_err(|msg| internal_error(format!("failed to parse wg output: {msg}")))
}

fn parse_wg_dump(interface: &str, stdout: &[u8]) -> Result<WireGuardInterfaceDto, String> {
    let dump = String::from_utf8(stdout.to_vec())
        .map_err(|_| "wg dump output was not valid UTF-8".to_string())?;

    let mut lines = dump.lines();
    let first_line = lines
        .next()
        .ok_or_else(|| "wg dump output missing interface line".to_string())?;
    let parts: Vec<&str> = first_line.split('\t').collect();
    if parts.len() < 4 {
        return Err("wg dump interface line had unexpected format".to_string());
    }

    let private_key = normalize_field(parts[0]);
    let listen_port: u16 = parts[2]
        .parse()
        .map_err(|_| "wg dump listen port was not a number".to_string())?;

    let mut peers = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 8 {
            return Err(format!("wg dump peer line had unexpected format: {line}"));
        }

        let public_key = fields[0].to_string();
        let preshared_key = normalize_field(fields[1]);
        let endpoint = normalize_field(fields[2]);
        let allowed_ips = parse_allowed_ips(fields[3]);
        let latest_handshake = parse_u64(fields[4]);
        let rx_bytes = parse_u64(fields[5]).unwrap_or(0);
        let tx_bytes = parse_u64(fields[6]).unwrap_or(0);
        let persistent_keepalive = parse_u16(fields[7]).filter(|v| *v != 0);

        peers.push(WireGuardPeerDto {
            public_key,
            preshared_key,
            endpoint,
            allowed_ips,
            last_handshake_epoch_secs: latest_handshake.filter(|v| *v != 0),
            rx_bytes,
            tx_bytes,
            persistent_keepalive_interval: persistent_keepalive,
            protocol_version: None,
        });
    }

    Ok(WireGuardInterfaceDto {
        interface: interface.to_string(),
        listen_port,
        private_key,
        peers,
    })
}

/// Treats `(none)`/empty values from `wg` output as `None`.
fn normalize_field(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "(none)" || trimmed == "(hidden)" {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Splits the comma-separated allowed IP list emitted by `wg`.
fn parse_allowed_ips(field: &str) -> Vec<String> {
    let trimmed = field.trim();
    if trimmed.is_empty() || trimmed == "(none)" {
        Vec::new()
    } else {
        trimmed
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }
}

/// Parses a decimal value, returning `None` on failure.
fn parse_u64(field: &str) -> Option<u64> {
    field.trim().parse::<u64>().ok()
}

/// Parses a decimal value, returning `None` on failure.
fn parse_u16(field: &str) -> Option<u16> {
    field.trim().parse::<u16>().ok()
}

/// Retrieve Chelsea's WireGuard interface state from the host kernel.
#[utoipa::path(
    get,
    path = "/api/admin/wireguard",
    responses(
        (status = 200, description = "WireGuard interface details", body = WireGuardInterfaceDto),
        (status = 500, description = "Failed to inspect WireGuard interface")
    )
)]
pub async fn get_wireguard_handler() -> Result<Json<WireGuardInterfaceDto>, (StatusCode, String)> {
    let vers_config = VersConfig::chelsea();
    let interface = vers_config.wg_interface_name.clone();

    info!(interface = %interface, "admin_wireguard_get");
    wireguard_admin::inspect_interface(&interface)
        .map(Json)
        .map_err(|error| internal_error(error.to_string()))
}

/// Add or update a WireGuard peer on Chelsea's interface.
#[utoipa::path(
    post,
    path = "/api/admin/wireguard/peers",
    request_body = AddWireGuardPeerRequest,
    responses(
        (status = 201, description = "WireGuard peer added", body = WireGuardInterfaceDto),
        (status = 400, description = "Invalid WireGuard peer request"),
        (status = 500, description = "Failed to configure WireGuard peer")
    )
)]
pub async fn add_wireguard_peer_handler(
    Json(payload): Json<AddWireGuardPeerRequest>,
) -> Result<(StatusCode, Json<WireGuardInterfaceDto>), (StatusCode, String)> {
    info!(
        public_key = %payload.public_key,
        allowed_ips = %payload.allowed_ips.join(","),
        "admin_wireguard_add_peer"
    );

    let config = VersConfig::chelsea();
    let interface = config.wg_interface_name.clone();

    wireguard_admin::add_or_update_peer(&interface, &payload)
        .map(|dto| (StatusCode::CREATED, Json(dto)))
        .map_err(|error| {
            let msg = error.to_string();
            if msg.contains("invalid") || msg.contains("allowed_ips") {
                bad_request(msg)
            } else {
                internal_error(msg)
            }
        })
}

/// Remove a WireGuard peer from Chelsea's interface.
#[utoipa::path(
    delete,
    path = "/api/admin/wireguard/peers/{public_key}",
    params(
        ("public_key" = String, Path, description = "Base64 or hex encoded WireGuard public key")
    ),
    responses(
        (status = 200, description = "WireGuard peer removed", body = WireGuardInterfaceDto),
        (status = 404, description = "WireGuard peer not found"),
        (status = 500, description = "Failed to remove WireGuard peer")
    )
)]
pub async fn delete_wireguard_peer_handler(
    Path(public_key): Path<String>,
) -> Result<Json<WireGuardInterfaceDto>, (StatusCode, String)> {
    info!(public_key = %public_key, "admin_wireguard_delete_peer");

    let config = VersConfig::chelsea();
    let interface = config.wg_interface_name.clone();

    wireguard_admin::delete_peer(&interface, &public_key)
        .map(Json)
        .map_err(|error| {
            let msg = error.to_string();
            if msg.contains("peer not found") {
                (StatusCode::NOT_FOUND, error.to_string())
            } else if msg.contains("invalid public_key") {
                bad_request(msg)
            } else {
                internal_error(msg)
            }
        })
}

/// Retrieve WireGuard state for a specific VM.
#[utoipa::path(
    get,
    path = "/api/admin/vm/{vm_id}/wireguard",
    params(
        ("vm_id" = Uuid, Path, description = "VM ID (v4 UUID)")
    ),
    responses(
        (status = 200, description = "WireGuard interface details", body = WireGuardInterfaceDto),
        (status = 404, description = "VM or WireGuard interface not found"),
        (status = 500, description = "Failed to inspect WireGuard interface")
    )
)]
pub async fn vm_wireguard_get_handler(
    State(core): State<Arc<dyn crate::ChelseaServerCore>>,
    Path(vm_id): Path<Uuid>,
) -> Result<Json<WireGuardInterfaceDto>, (StatusCode, String)> {
    let target = core
        .vm_wireguard_target(&vm_id)
        .await
        .map_err(api_error_response)?;

    info!(
        vm_id = %vm_id,
        interface = %target.interface_name,
        namespace = %target.netns_name,
        "admin_vm_wireguard_get"
    );

    run_wgcli_action(&target, WgCliAction::List).await.map(Json)
}

/// Add or update a peer on a VM-scoped WireGuard interface.
#[utoipa::path(
    post,
    path = "/api/admin/vm/{vm_id}/wireguard/peers",
    params(
        ("vm_id" = Uuid, Path, description = "VM ID (v4 UUID)")
    ),
    request_body = AddWireGuardPeerRequest,
    responses(
        (status = 201, description = "WireGuard peer added", body = WireGuardInterfaceDto),
        (status = 404, description = "VM or WireGuard interface not found"),
        (status = 400, description = "Invalid WireGuard configuration"),
        (status = 500, description = "Failed to configure WireGuard peer")
    )
)]
pub async fn vm_wireguard_add_handler(
    State(core): State<Arc<dyn crate::ChelseaServerCore>>,
    Path(vm_id): Path<Uuid>,
    Json(payload): Json<AddWireGuardPeerRequest>,
) -> Result<(StatusCode, Json<WireGuardInterfaceDto>), (StatusCode, String)> {
    let target = core
        .vm_wireguard_target(&vm_id)
        .await
        .map_err(api_error_response)?;

    info!(
        vm_id = %vm_id,
        interface = %target.interface_name,
        allowed_ips = %payload.allowed_ips.join(","),
        "admin_vm_wireguard_add_peer"
    );

    run_wgcli_action(&target, WgCliAction::Add(&payload))
        .await
        .map(|dto| (StatusCode::CREATED, Json(dto)))
}

/// Remove a peer from a VM-scoped WireGuard interface.
#[utoipa::path(
    delete,
    path = "/api/admin/vm/{vm_id}/wireguard/peers/{public_key}",
    params(
        ("vm_id" = Uuid, Path, description = "VM ID (v4 UUID)"),
        ("public_key" = String, Path, description = "Peer public key")
    ),
    responses(
        (status = 200, description = "WireGuard peer removed", body = WireGuardInterfaceDto),
        (status = 404, description = "VM or WireGuard peer not found"),
        (status = 500, description = "Failed to remove WireGuard peer")
    )
)]
pub async fn vm_wireguard_delete_handler(
    State(core): State<Arc<dyn crate::ChelseaServerCore>>,
    Path(path): Path<VmWireGuardPeerPath>,
) -> Result<Json<WireGuardInterfaceDto>, (StatusCode, String)> {
    let target = core
        .vm_wireguard_target(&path.vm_id)
        .await
        .map_err(api_error_response)?;

    info!(
        vm_id = %path.vm_id,
        interface = %target.interface_name,
        public_key = %path.public_key,
        "admin_vm_wireguard_delete_peer"
    );

    run_wgcli_action(&target, WgCliAction::Delete(&path.public_key))
        .await
        .map(Json)
}

/// Retrieve detailed network information for a VM.
#[utoipa::path(
    get,
    path = "/api/admin/vm/{vm_id}/network",
    params(
        ("vm_id" = Uuid, Path, description = "VM ID (v4 UUID)")
    ),
    responses(
        (status = 200, description = "VM network information", body = VmNetworkInfoDto),
        (status = 404, description = "VM not found"),
        (status = 500, description = "Failed to retrieve VM network information")
    )
)]
pub async fn vm_network_info_handler(
    State(core): State<Arc<dyn crate::ChelseaServerCore>>,
    Path(vm_id): Path<Uuid>,
) -> Result<Json<VmNetworkInfoDto>, (StatusCode, String)> {
    core.vm_network_info(&vm_id)
        .await
        .map(Json)
        .map_err(api_error_response)
}

/// Retrieve the current Vers configuration.
#[utoipa::path(
    get,
    path = "/api/admin/config",
    responses(
        (status = 200, description = "Current configuration", body = VersConfig)
    )
)]
async fn get_config_handler() -> Json<&'static VersConfig> {
    let config = VersConfig::global();
    Json(config)
}

#[derive(OpenApi)]
#[openapi(paths(
    get_wireguard_handler,
    add_wireguard_peer_handler,
    delete_wireguard_peer_handler,
    vm_wireguard_get_handler,
    vm_wireguard_add_handler,
    vm_wireguard_delete_handler,
    vm_network_info_handler,
    get_config_handler
))]
pub struct AdminApiDoc;

pub fn create_admin_router(
    core: Arc<dyn crate::ChelseaServerCore>,
) -> (Router, utoipa::openapi::OpenApi) {
    OpenApiRouter::with_openapi(AdminApiDoc::openapi())
        .routes(routes!(get_wireguard_handler))
        .routes(routes!(add_wireguard_peer_handler))
        .routes(routes!(delete_wireguard_peer_handler))
        .routes(routes!(
            vm_wireguard_get_handler,
            vm_wireguard_add_handler,
            vm_wireguard_delete_handler
        ))
        .route(
            "/api/admin/vm/{vm_id}/network",
            get(vm_network_info_handler),
        )
        .routes(routes!(get_config_handler))
        .with_state(core)
        .split_for_parts()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_wg_dump_single_peer() {
        let dump = "privatekey123\tpublickey123\t51820\t0\npeerpublickey\tpresharedkey\t203.0.113.42:51820\tfd00:fe11:deed:0100::1/128,10.42.0.8/32\t1731548105\t1024\t2048\t25\n";
        let dto = parse_wg_dump("wgvm0", dump.as_bytes()).expect("parse succeeds");
        assert_eq!(dto.interface, "wgvm0");
        assert_eq!(dto.listen_port, 51820);
        assert_eq!(dto.private_key.as_deref(), Some("privatekey123"));
        assert_eq!(dto.peers.len(), 1);
        let peer = &dto.peers[0];
        assert_eq!(peer.public_key, "peerpublickey");
        assert_eq!(peer.preshared_key.as_deref(), Some("presharedkey"));
        assert_eq!(peer.endpoint.as_deref(), Some("203.0.113.42:51820"));
        assert_eq!(
            peer.allowed_ips,
            vec![
                "fd00:fe11:deed:0100::1/128".to_string(),
                "10.42.0.8/32".to_string()
            ]
        );
        assert_eq!(peer.last_handshake_epoch_secs, Some(1731548105));
        assert_eq!(peer.rx_bytes, 1024);
        assert_eq!(peer.tx_bytes, 2048);
        assert_eq!(peer.persistent_keepalive_interval, Some(25));
    }

    #[test]
    fn parse_wg_dump_empty_peer_list() {
        let dump = "privatekey123\tpublickey123\t51820\t0\n";
        let dto = parse_wg_dump("wgvm1", dump.as_bytes()).expect("parse succeeds");
        assert!(dto.peers.is_empty());
    }

    #[test]
    fn parse_wg_dump_handles_none_fields() {
        let dump =
            "privatekey123\tpublickey123\t51820\t0\npeerkey\t(none)\t(none)\t(none)\t0\t0\t0\t0\n";
        let dto = parse_wg_dump("wgvm2", dump.as_bytes()).expect("parse succeeds");
        let peer = &dto.peers[0];
        assert!(peer.preshared_key.is_none());
        assert!(peer.endpoint.is_none());
        assert!(peer.allowed_ips.is_empty());
        assert!(peer.last_handshake_epoch_secs.is_none());
        assert_eq!(peer.rx_bytes, 0);
        assert_eq!(peer.tx_bytes, 0);
        assert!(peer.persistent_keepalive_interval.is_none());
    }
}
