use std::net::Ipv4Addr;

use ipnet::Ipv4Net;
use tracing::debug;

use crate::network::{
    linux::namespace::netns_exec,
    utils::{run_command, vm_addr_from_host_addr},
};

/// Create a new point-to-point (RFC 3021) veth pair with IP addresses host_addr and host_addr+1 in the given namespace. Will throw an error if host_addr is not the lower address in a point-to-point network. If it exists, its IPs will be flushed and re-added
pub async fn veth_add_peer_netns(
    host_addr: &Ipv4Addr,
    netns_name: impl AsRef<str>,
) -> anyhow::Result<()> {
    let vm_addr = vm_addr_from_host_addr(host_addr)?;
    let vm_net = Ipv4Net::new_assert(vm_addr, 31);
    let host_net = Ipv4Net::new_assert(host_addr.clone(), 31);

    let netns_name = netns_name.as_ref();

    let (veth_host_name, veth_vm_name) = veth_names_from_addr_pair(&host_addr, &vm_addr);

    // Create the veth pair
    match run_command(&[
        "ip",
        "link",
        "add",
        &veth_host_name,
        "type",
        "veth",
        "peer",
        "name",
        &veth_vm_name,
        "netns",
        netns_name,
    ])
    .await
    {
        Ok(_) => {}
        Err(e) => {
            let err_str = format!("{:?}", e);
            if err_str.contains("File exists") {
                // Check if the peer device exists in the namespace
                let peer_exists = netns_exec(netns_name, &["ip", "link", "show", &veth_vm_name])
                    .await
                    .is_ok();

                if peer_exists {
                    // Both devices exist; flush IP addresses before re-adding
                    debug!(
                        veth_vm_name,
                        veth_host_name,
                        %vm_addr,
                        %host_addr,
                        "Found existing veth pair; flushing IP addresses"
                    );
                    let _ = run_command(&["ip", "addr", "flush", "dev", &veth_host_name]).await;
                    let _ = netns_exec(netns_name, &["ip", "addr", "flush", "dev", &veth_vm_name])
                        .await;
                } else {
                    // Host-side device exists but peer is missing (namespace was likely recreated).
                    // Delete the stale host-side device and recreate the pair.
                    debug!(
                        veth_vm_name,
                        veth_host_name,
                        %vm_addr,
                        %host_addr,
                        "Found stale host-side veth (peer missing in namespace); deleting and recreating"
                    );
                    run_command(&["ip", "link", "del", &veth_host_name]).await?;
                    run_command(&[
                        "ip",
                        "link",
                        "add",
                        &veth_host_name,
                        "type",
                        "veth",
                        "peer",
                        "name",
                        &veth_vm_name,
                        "netns",
                        netns_name,
                    ])
                    .await?;
                }
            } else {
                return Err(e);
            }
        }
    }

    // Set IP address (host side)
    run_command(&[
        "ip",
        "addr",
        "add",
        &host_net.to_string(),
        "dev",
        &veth_host_name,
    ])
    .await?;

    // Set IP address (netns side)
    netns_exec(
        netns_name,
        &[
            "ip",
            "addr",
            "add",
            &vm_net.to_string(),
            "dev",
            &veth_vm_name,
        ],
    )
    .await?;

    // Set devices to UP state
    run_command(&["ip", "link", "set", &veth_host_name, "up"]).await?;
    netns_exec(netns_name, &["ip", "link", "set", &veth_vm_name, "up"]).await?;

    Ok(())
}

/// Generate veth pair names from the host IP
pub fn veth_names_from_addr_pair(host_addr: &Ipv4Addr, vm_addr: &Ipv4Addr) -> (String, String) {
    (
        veth_host_name_from_host_addr(host_addr),
        veth_vm_name_from_vm_addr(vm_addr),
    )
}

pub fn veth_host_name_from_host_addr(host_addr: &Ipv4Addr) -> String {
    format!(
        "vh_{}",
        host_addr
            .octets()
            .into_iter()
            .map(|x| format!("{:03}", x))
            .collect::<Vec<_>>()
            .join("")
    )
}

pub fn veth_vm_name_from_vm_addr(vm_addr: &Ipv4Addr) -> String {
    format!(
        "vv_{}",
        vm_addr
            .octets()
            .into_iter()
            .map(|x| format!("{:03}", x))
            .collect::<Vec<_>>()
            .join("")
    )
}
