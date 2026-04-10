use std::net::Ipv6Addr;

use orch_wg::{WG, WgPeer};
use vers_config::VersConfig;

/// Creates a Wireguard interface for service ChelseaServer on. This interface has a single peer: The orchestrator,
/// whose wireguard configuration is expected to be in the VersConfig.
pub fn setup_chelsea_server_wireguard(
    wg_ipv6: Ipv6Addr,
    wg_private_key: String,
) -> anyhow::Result<WG> {
    let orchestrator = VersConfig::orchestrator();
    let chelsea = VersConfig::chelsea();

    let orch_peer = WgPeer {
        endpoint_ip: orchestrator.public_ip.clone(),
        remote_ipv6: orchestrator.wg_private_ip.clone(),
        port: orchestrator.wg_port,
        pub_key: orchestrator.wg_public_key.clone(),
    };

    let wg = WG::new_with_peers(
        "wgchelsea",
        wg_ipv6,
        wg_private_key,
        chelsea.wg_port,
        vec![orch_peer],
    )?;

    Ok(wg)
}
