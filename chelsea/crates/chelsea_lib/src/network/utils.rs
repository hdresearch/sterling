use anyhow::{anyhow, bail};
use macaddr::MacAddr6;
use nftables::helper;
use nftables::{batch::Batch, schema, types};
use std::net::Ipv4Addr;
use std::process::Output;
use tokio::process::Command;

/// Convert an IPv4 address to a MAC address using the Firecracker format - 06:00:xx:xx:xx:xx
pub fn ipv4_to_mac(ip: &Ipv4Addr) -> MacAddr6 {
    let octets = ip.octets();
    MacAddr6::new(06, 0, octets[0], octets[1], octets[2], octets[3])
}

/// Helper function to run commands and handle errors
pub async fn run_command(args: &[&str]) -> anyhow::Result<Output> {
    let program = args[0];

    let output = Command::new(program).args(&args[1..]).output().await?;

    match output.status.success() {
        true => Ok(output),
        false => {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let cmd = args.join(" ");
            let code = output.status.code().unwrap_or(-1);
            Err(anyhow!(
                "Network Manager command failed. Command: '{cmd}'; Exit Code: {code}; stderr: {stderr}"
            ))
        }
    }
}

/// Helper function to run commands and handle errors synchronously
pub fn run_command_sync(args: &[&str]) -> anyhow::Result<Output> {
    let program = args[0];

    let output = std::process::Command::new(program)
        .args(&args[1..])
        .output()?;

    match output.status.success() {
        true => Ok(output),
        false => {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let cmd = args.join(" ");
            let code = output.status.code().unwrap_or(-1);
            Err(anyhow!(
                "Network Manager command failed. Command: '{cmd}'; Exit Code: {code}; stderr: {stderr}"
            ))
        }
    }
}

/// Ensure that IP forwarding is enabled
pub async fn enable_ip_forwarding() -> anyhow::Result<()> {
    // Enable IP forwarding on host
    run_command(&["sysctl", "-w", "net.ipv4.ip_forward=1"]).await?;
    run_command(&["sysctl", "-w", "net.ipv6.conf.all.forwarding=1"]).await?;
    run_command(&["iptables", "-P", "FORWARD", "ACCEPT"]).await?;
    run_command(&["ip6tables", "-P", "FORWARD", "ACCEPT"]).await?;

    // Create nftables batch
    let mut batch = Batch::new();

    // Create filter table
    batch.add(schema::NfListObject::Table(schema::Table {
        family: types::NfFamily::INet,
        name: "filter".into(),
        handle: None,
    }));

    // Create FORWARD chain with ACCEPT policy
    batch.add(schema::NfListObject::Chain(schema::Chain {
        family: types::NfFamily::INet,
        table: "filter".into(),
        name: "forward".into(),
        newname: None,
        handle: None,
        _type: Some(types::NfChainType::Filter),
        hook: Some(types::NfHook::Forward),
        prio: Some(0),
        dev: None,
        policy: Some(types::NfChainPolicy::Accept),
    }));

    helper::apply_ruleset(&batch.to_nftables())?;

    Ok(())
}

/// Ensures that the supplied host addr is the lower address in a point-to-point (RFC 3021) subnet, and responds the corresponding VM (higher) address.
pub fn vm_addr_from_host_addr(host_addr: &Ipv4Addr) -> anyhow::Result<Ipv4Addr> {
    let octets = host_addr.octets();
    if octets[3] % 2 == 1 {
        bail!(
            "host_addr {host_addr} must be the lower address in a point-to-point (RFC 3021) subnet."
        );
    } else {
        Ok(Ipv4Addr::new(
            octets[0],
            octets[1],
            octets[2],
            octets[3] + 1,
        ))
    }
}
