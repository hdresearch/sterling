use std::net::{Ipv4Addr, Ipv6Addr};
use std::process::Output;

use anyhow::bail;
use tracing::debug;

use crate::network::utils::run_command;

/// Create a new network namespace with a given name
pub async fn netns_add(netns_name: impl AsRef<str>) -> Result<(), anyhow::Error> {
    let netns_name = netns_name.as_ref();

    // Create the namespace
    if let Err(e) = run_command(&["ip", "netns", "add", netns_name]).await {
        let err_str = format!("{:?}", e);
        if !err_str.contains("File exists") {
            bail!(err_str);
        }
        debug!(
            netns_name,
            "Netns already exists; setting loop as up and continuing"
        )
    }

    // Set loopback device as up
    netns_exec(netns_name, &["ip", "link", "set", "lo", "up"]).await?;

    Ok(())
}

/// Execute a command within a given namespace
pub async fn netns_exec(netns_name: impl AsRef<str>, args: &[&str]) -> anyhow::Result<Output> {
    let mut cmd_args = vec!["ip", "netns", "exec", netns_name.as_ref()];
    cmd_args.extend_from_slice(args);

    run_command(&cmd_args).await
}

/// Enable packet forwarding and create DNAT rules in the network to forward packets to the VM
pub async fn netns_enable_packet_forwarding(
    netns_name: impl AsRef<str>,
    veth_vm_name: impl AsRef<str>,
    guest_addr_v4: &Ipv4Addr,
    guest_addr_v6: &Ipv6Addr,
) -> anyhow::Result<()> {
    let netns_name = netns_name.as_ref();
    let veth_vm_name = veth_vm_name.as_ref();

    // IPv4: PREROUTING DNAT
    let command0 = [
        "iptables",
        "-t",
        "nat",
        "-A",
        "PREROUTING",
        "-i",
        veth_vm_name,
        "-j",
        "DNAT",
        "--to",
        &guest_addr_v4.to_string(),
    ];
    // IPv4: POSTROUTING MASQUERADE
    let command1 = [
        "iptables",
        "-t",
        "nat",
        "-A",
        "POSTROUTING",
        "-o",
        veth_vm_name,
        "-j",
        "MASQUERADE",
    ];

    // IPv6: PREROUTING DNAT
    let command2 = [
        "ip6tables",
        "-t",
        "nat",
        "-A",
        "PREROUTING",
        "-i",
        veth_vm_name,
        "-j",
        "DNAT",
        "--to-destination",
        &guest_addr_v6.to_string(),
    ];
    // IPv6: POSTROUTING MASQUERADE
    let command3 = [
        "ip6tables",
        "-t",
        "nat",
        "-A",
        "POSTROUTING",
        "-o",
        veth_vm_name,
        "-j",
        "MASQUERADE",
    ];

    let command4 = ["iptables", "-P", "FORWARD", "ACCEPT"];
    let command5 = ["sysctl", "-w", "net.ipv4.ip_forward=1"];
    let command6 = ["ip6tables", "-P", "FORWARD", "ACCEPT"];
    let command7 = ["sysctl", "-w", "net.ipv6.conf.all.forwarding=1"];

    let (result0, result1, result2, result3, result4, result5, result6, result7) = tokio::join!(
        netns_exec(netns_name, &command0),
        netns_exec(netns_name, &command1),
        netns_exec(netns_name, &command2),
        netns_exec(netns_name, &command3),
        netns_exec(netns_name, &command4),
        netns_exec(netns_name, &command5),
        netns_exec(netns_name, &command6),
        netns_exec(netns_name, &command7),
    );

    (
        result0?, result1?, result2?, result3?, result4?, result5?, result6?, result7?,
    );

    Ok(())
}

pub async fn netns_del(netns_name: impl AsRef<str>) -> anyhow::Result<()> {
    let netns_name = netns_name.as_ref();
    run_command(&["ip", "netns", "del", netns_name])
        .await
        .map(|_| ())?;
    Ok(())
}

/// Generate netns netns_name from the vm IP
pub fn netns_name_from_host_addr(host_addr: &Ipv4Addr) -> String {
    format!(
        "vm_{}",
        host_addr
            .octets()
            .into_iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join("_")
    )
}

/// Set the default route for the namespace
pub async fn netns_set_default_route(
    netns_name: impl AsRef<str>,
    default_route: &Ipv4Addr,
) -> anyhow::Result<()> {
    let netns_name = netns_name.as_ref();

    // Add default route in namespace
    match netns_exec(
        netns_name,
        &[
            "ip",
            "route",
            "add",
            "default",
            "via",
            &default_route.to_string(),
        ],
    )
    .await
    {
        Ok(_) => {}
        Err(e) => {
            let err_str = format!("{:?}", e);
            if !err_str.contains("File exists") {
                return Err(e);
            }
            debug!(netns_name, %default_route, "Default route already exists");
        }
    }

    Ok(())
}
