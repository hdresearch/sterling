use anyhow::{Context, anyhow};
use defguard_wireguard_rs::{
    WGApi, WireguardInterfaceApi,
    host::{Host, Peer},
    key::Key,
    net::IpAddrMask,
};
pub use dto_lib::chelsea_server2::wireguard::{
    AddWireGuardPeerRequest, WireGuardInterfaceDto, WireGuardPeerDto,
};
use std::{
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

fn system_time_to_epoch_secs(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

fn peer_to_dto(peer: &Peer) -> WireGuardPeerDto {
    WireGuardPeerDto {
        public_key: peer.public_key.to_string(),
        preshared_key: peer.preshared_key.as_ref().map(|key| key.to_string()),
        endpoint: peer.endpoint.as_ref().map(|sock| sock.to_string()),
        allowed_ips: peer.allowed_ips.iter().map(IpAddrMask::to_string).collect(),
        last_handshake_epoch_secs: peer.last_handshake.and_then(system_time_to_epoch_secs),
        rx_bytes: peer.rx_bytes,
        tx_bytes: peer.tx_bytes,
        persistent_keepalive_interval: peer.persistent_keepalive_interval,
        protocol_version: peer.protocol_version,
    }
}

fn host_to_interface(interface: String, host: Host) -> WireGuardInterfaceDto {
    let peers = host.peers.values().map(peer_to_dto).collect();
    WireGuardInterfaceDto {
        interface,
        listen_port: host.listen_port,
        private_key: host.private_key.as_ref().map(|key| key.to_string()),
        peers,
    }
}

#[derive(Clone, Debug)]
struct ParsedPeerConfig {
    public_key: Key,
    preshared_key: Option<Key>,
    allowed_ips: Vec<IpAddrMask>,
    endpoint: Option<String>,
    persistent_keepalive_interval: Option<u16>,
}

fn parse_add_peer_request(payload: &AddWireGuardPeerRequest) -> anyhow::Result<ParsedPeerConfig> {
    if payload.allowed_ips.is_empty() {
        return Err(anyhow!("allowed_ips must contain at least one entry"));
    }

    let public_key = Key::try_from(payload.public_key.as_str())
        .map_err(|error| anyhow!("invalid public_key: {error}"))?;

    let preshared_key = match &payload.preshared_key {
        Some(value) => Some(
            Key::try_from(value.as_str())
                .map_err(|error| anyhow!("invalid preshared_key: {error}"))?,
        ),
        None => None,
    };

    let mut allowed_ips = Vec::with_capacity(payload.allowed_ips.len());
    for cidr in &payload.allowed_ips {
        let mask = IpAddrMask::from_str(cidr).map_err(|_| anyhow!("invalid allowed_ip: {cidr}"))?;
        allowed_ips.push(mask);
    }

    Ok(ParsedPeerConfig {
        public_key,
        preshared_key,
        allowed_ips,
        endpoint: payload.endpoint.clone(),
        persistent_keepalive_interval: payload.persistent_keepalive_interval,
    })
}

fn build_peer_from_config(config: &ParsedPeerConfig) -> anyhow::Result<Peer> {
    let mut peer = Peer::new(config.public_key.clone());
    peer.allowed_ips = config.allowed_ips.clone();
    peer.preshared_key = config.preshared_key.clone();

    if let Some(endpoint) = &config.endpoint {
        peer.set_endpoint(endpoint)
            .map_err(|error| anyhow!("invalid endpoint: {error}"))?;
    }

    if let Some(interval) = config.persistent_keepalive_interval {
        peer.persistent_keepalive_interval = Some(interval);
    }

    Ok(peer)
}

pub fn inspect_interface(interface: &str) -> anyhow::Result<WireGuardInterfaceDto> {
    let name = interface.to_string();
    #[cfg(not(target_os = "macos"))]
    let wgapi = WGApi::<defguard_wireguard_rs::Kernel>::new(name.clone())
        .with_context(|| format!("failed to open WireGuard interface '{interface}'"))?;
    #[cfg(target_os = "macos")]
    let wgapi = WGApi::<defguard_wireguard_rs::Userspace>::new(name.clone())
        .with_context(|| format!("failed to open WireGuard interface '{interface}'"))?;

    let host = wgapi
        .read_interface_data()
        .with_context(|| format!("failed to read WireGuard interface '{interface}'"))?;
    Ok(host_to_interface(name, host))
}

pub fn add_or_update_peer(
    interface: &str,
    payload: &AddWireGuardPeerRequest,
) -> anyhow::Result<WireGuardInterfaceDto> {
    let parsed = parse_add_peer_request(payload)?;

    #[cfg(not(target_os = "macos"))]
    let wgapi = WGApi::<defguard_wireguard_rs::Kernel>::new(interface.to_string())
        .with_context(|| format!("failed to open WireGuard interface '{interface}'"))?;
    #[cfg(target_os = "macos")]
    let wgapi = WGApi::<defguard_wireguard_rs::Userspace>::new(interface.to_string())
        .with_context(|| format!("failed to open WireGuard interface '{interface}'"))?;

    let peer = build_peer_from_config(&parsed)?;

    wgapi
        .configure_peer(&peer)
        .with_context(|| format!("failed to configure peer on '{interface}'"))?;
    wgapi
        .configure_peer_routing(&[peer])
        .with_context(|| format!("failed to configure peer routing on '{interface}'"))?;

    let host = wgapi
        .read_interface_data()
        .with_context(|| format!("failed to read WireGuard interface '{interface}'"))?;

    Ok(host_to_interface(interface.to_string(), host))
}

pub fn delete_peer(interface: &str, public_key: &str) -> anyhow::Result<WireGuardInterfaceDto> {
    let peer_key =
        Key::try_from(public_key).map_err(|error| anyhow!("invalid public_key: {error}"))?;

    #[cfg(not(target_os = "macos"))]
    let wgapi = WGApi::<defguard_wireguard_rs::Kernel>::new(interface.to_string())
        .with_context(|| format!("failed to open WireGuard interface '{interface}'"))?;
    #[cfg(target_os = "macos")]
    let wgapi = WGApi::<defguard_wireguard_rs::Userspace>::new(interface.to_string())
        .with_context(|| format!("failed to open WireGuard interface '{interface}'"))?;

    let host = wgapi
        .read_interface_data()
        .with_context(|| format!("failed to read WireGuard interface '{interface}'"))?;

    if !host.peers.contains_key(&peer_key) {
        return Err(anyhow!("peer not found"));
    }

    wgapi
        .remove_peer(&peer_key)
        .with_context(|| format!("failed to remove peer from '{interface}'"))?;

    let host = wgapi
        .read_interface_data()
        .with_context(|| format!("failed to read WireGuard interface '{interface}'"))?;

    Ok(host_to_interface(interface.to_string(), host))
}

/// Target information required to operate on a VM-scoped WireGuard interface.
#[derive(Debug, Clone)]
pub struct WireGuardTarget {
    pub interface_name: String,
    pub netns_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request() -> AddWireGuardPeerRequest {
        AddWireGuardPeerRequest {
            public_key: "59ul25nwOI5ypR5npkjcjt0ZXTWQsdq4lcf+sMkpeXg=".to_string(),
            preshared_key: Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string()),
            endpoint: Some("203.0.113.42:51820".to_string()),
            allowed_ips: vec![
                "fd00:fe11:deed:0100::1/128".to_string(),
                "10.42.0.8/32".to_string(),
            ],
            persistent_keepalive_interval: Some(25),
        }
    }

    #[test]
    fn parse_add_peer_request_success() {
        let request = sample_request();
        let parsed = parse_add_peer_request(&request).expect("valid request parses");

        assert_eq!(parsed.public_key.to_string(), request.public_key);
        assert_eq!(
            parsed.preshared_key.as_ref().map(|k| k.to_string()),
            request.preshared_key
        );
        assert_eq!(parsed.allowed_ips.len(), 2);
    }

    #[test]
    fn parse_add_peer_request_rejects_missing_allowed_ips() {
        let request = AddWireGuardPeerRequest {
            allowed_ips: Vec::new(),
            ..sample_request()
        };

        let error = parse_add_peer_request(&request).expect_err("expected error");
        assert!(error.to_string().contains("allowed_ips"));
    }

    #[test]
    fn parse_add_peer_request_rejects_invalid_public_key() {
        let request = AddWireGuardPeerRequest {
            public_key: "not-a-valid-key".to_string(),
            ..sample_request()
        };

        let error = parse_add_peer_request(&request).expect_err("expected error");
        assert!(error.to_string().contains("invalid public_key"));
    }

    #[test]
    fn build_peer_from_config_rejects_invalid_endpoint() {
        let mut parsed = parse_add_peer_request(&sample_request()).expect("valid request parses");
        parsed.endpoint = Some("not-an-endpoint".to_string());

        let error = build_peer_from_config(&parsed).expect_err("expected error");
        assert!(error.to_string().contains("invalid endpoint"));
    }
}
