//! WireGuard interface lifecycle for VM networking.
//!
//! Each VM gets a WG interface created in the global namespace, then moved
//! to the VM's network namespace. The interface is configured with port 0
//! in the global namespace (to avoid port conflicts) and the real listen
//! port is set after the move. This is critical — binding the real port
//! in the global namespace will cause EADDRINUSE if another VM uses the
//! same port, and failed setups leak interfaces that block future attempts.
//!
//! An [`InterfaceGuard`] ensures cleanup on error: if any step after
//! `create_interface()` fails, the guard's `Drop` impl deletes the
//! interface from wherever it currently lives (global ns or target netns).
//!
//! See: PR #880 (EADDRINUSE fix), PR #589 (original regression).

use anyhow::anyhow;
use defguard_wireguard_rs::{
    InterfaceConfiguration, WGApi, WireguardInterfaceApi, host::Peer, key::Key, net::IpAddrMask,
};
use std::{
    net::SocketAddr,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    process::Command,
    str::FromStr,
};

// TODO: this value should be passed in by orch; hardcoded as a hacky workaround.
const PEER_PORT: u16 = 51282;
const KEEPALIVE: u16 = 45;

/// RAII guard that deletes a WireGuard interface on drop unless disarmed.
///
/// Tracks whether the interface has been moved to a network namespace so it
/// can issue the correct deletion command (global ns vs inside a netns).
struct InterfaceGuard<'a> {
    name: &'a str,
    /// `None` means the interface is in the global namespace.
    /// `Some(ns)` means it has been moved into that netns.
    namespace: Option<&'a str>,
    disarmed: bool,
}

impl<'a> InterfaceGuard<'a> {
    fn new(name: &'a str) -> Self {
        Self {
            name,
            namespace: None,
            disarmed: false,
        }
    }

    /// Record that the interface has been moved into the given namespace.
    fn moved_to_namespace(&mut self, namespace: &'a str) {
        self.namespace = Some(namespace);
    }

    /// Disarm the guard — the interface will not be deleted on drop.
    /// Call this only after all setup steps have succeeded.
    fn disarm(&mut self) {
        self.disarmed = true;
    }
}

impl Drop for InterfaceGuard<'_> {
    fn drop(&mut self) {
        if self.disarmed {
            return;
        }

        tracing::warn!(
            interface = self.name,
            namespace = ?self.namespace,
            "Cleaning up WireGuard interface after setup failure"
        );

        let result = match self.namespace {
            None => {
                // Interface is still in the global namespace
                Command::new("ip")
                    .arg("link")
                    .arg("del")
                    .arg(self.name)
                    .output()
            }
            Some(ns) => {
                // Interface was moved to a netns
                Command::new("ip")
                    .arg("netns")
                    .arg("exec")
                    .arg(ns)
                    .arg("ip")
                    .arg("link")
                    .arg("del")
                    .arg(self.name)
                    .output()
            }
        };

        match result {
            Ok(output) if !output.status.success() => {
                tracing::error!(
                    interface = self.name,
                    stderr = %String::from_utf8_lossy(&output.stderr),
                    "Failed to delete WireGuard interface during cleanup"
                );
            }
            Err(e) => {
                tracing::error!(
                    interface = self.name,
                    error = %e,
                    "Failed to execute cleanup command for WireGuard interface"
                );
            }
            _ => {
                tracing::info!(
                    interface = self.name,
                    "Successfully cleaned up WireGuard interface after setup failure"
                );
            }
        }
    }
}

pub fn wg_setup(
    interface_name: &str,
    vm_wg_port: u16,
    private_key: &str,
    private_ip: Ipv6Addr,
    peer_pub_key: &str,
    peer_pub_ip: Ipv4Addr,
    peer_prv_ip: Ipv6Addr,
    namespace: &str,
) -> anyhow::Result<()> {
    tracing::debug!(
        interface_name,
        vm_wg_port,
        ?private_ip,
        namespace,
        "Starting WireGuard setup"
    );

    // Defensively clean up any stale WireGuard interfaces in the target
    // namespace. This handles cases where a previous teardown failed silently
    // (e.g., the VM process was still holding the interface) and the network
    // slot was recycled. Without this, `ip link set up` on the new interface
    // would fail with EADDRINUSE because the old interface's UDP socket is
    // still bound.
    //
    // Note: interface names are random (vm_{uuid prefix}), so we can't check
    // by name — we must find all WireGuard-type interfaces in the namespace.
    if let Ok(output) = Command::new("ip")
        .arg("netns")
        .arg("exec")
        .arg(namespace)
        .arg("ip")
        .arg("-o")
        .arg("link")
        .arg("show")
        .arg("type")
        .arg("wireguard")
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stale_ifaces: Vec<&str> = stdout
                .lines()
                .filter_map(|line| {
                    // Format: "N: vm_XXXX@NONE: <flags> ..."
                    let name = line.split_whitespace().nth(1)?;
                    let name = name.trim_end_matches(':');
                    Some(name.split('@').next().unwrap_or(name))
                })
                .collect();

            for stale in &stale_ifaces {
                tracing::warn!(
                    stale_interface = stale,
                    new_interface = interface_name,
                    namespace,
                    "Found stale WireGuard interface in namespace; cleaning up before setup"
                );
                let _ = Command::new("ip")
                    .arg("netns")
                    .arg("exec")
                    .arg(namespace)
                    .arg("ip")
                    .arg("link")
                    .arg("del")
                    .arg(stale)
                    .status();
            }

            if !stale_ifaces.is_empty() {
                // Brief pause to let the kernel fully release UDP sockets
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    let wgapi = WGApi::<defguard_wireguard_rs::Kernel>::new(interface_name.to_string())?;
    #[cfg(target_os = "macos")]
    let wgapi = WGApi::<defguard_wireguard_rs::Userspace>::new(interface_name.to_string())?;

    let peer_pub_key: Key = Key::from_str(peer_pub_key)?;
    let mut peer = Peer::new(peer_pub_key);
    let endpoint = SocketAddr::new(IpAddr::V4(peer_pub_ip), PEER_PORT);
    let private_ip_addr_mask = IpAddrMask::new(IpAddr::V6(private_ip), 128);
    let peer_prv_ip_addr_mask = IpAddrMask::new(IpAddr::V6(peer_prv_ip), 128);
    peer.endpoint = Some(endpoint);
    peer.persistent_keepalive_interval = Some(KEEPALIVE);
    peer.allowed_ips.push(peer_prv_ip_addr_mask.clone());

    let interface_config = InterfaceConfiguration {
        name: interface_name.to_string(),
        prvkey: private_key.to_string(),
        addresses: vec![private_ip_addr_mask.clone()],
        // Use port 0 in the global namespace to avoid EADDRINUSE conflicts.
        // The WG interface is created here, then moved to the VM's netns,
        // where the actual listen port is set. Binding the real port in the
        // global namespace races with other VMs and leaks zombie sockets on failure.
        port: 0,
        peers: vec![peer],
        mtu: None,
    };

    wgapi.create_interface()?;

    // Guard ensures the interface is cleaned up if any subsequent step fails.
    // Must be created immediately after create_interface() succeeds.
    let mut guard = InterfaceGuard::new(interface_name);

    wgapi.configure_interface(&interface_config)?;

    // Move the configured interface to the VM's network namespace
    tracing::debug!(
        interface_name,
        namespace,
        "Moving WireGuard interface to namespace"
    );
    let output = Command::new("ip")
        .arg("link")
        .arg("set")
        .arg(&interface_name)
        .arg("netns")
        .arg(namespace)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!(
            interface_name,
            namespace,
            stderr = %stderr,
            "Failed to move wireguard interface to namespace"
        );
        return Err(anyhow!(
            "Failed to move wireguard interface to new namespace: {}",
            stderr
        ));
    }

    // Interface is now in the netns — update the guard so it cleans up
    // in the right place if a later step fails.
    guard.moved_to_namespace(namespace);

    // Set the actual WireGuard listen port inside the namespace, where it
    // cannot collide with ports bound in the global namespace.
    let output = Command::new("ip")
        .arg("netns")
        .arg("exec")
        .arg(namespace)
        .arg("wg")
        .arg("set")
        .arg(&interface_name)
        .arg("listen-port")
        .arg(&vm_wg_port.to_string())
        .output()?;
    if !output.status.success() {
        return Err(anyhow!(
            "Failed to set WireGuard listen port in namespace: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Add the IPv6 address inside the namespace (addresses are lost when moving namespaces)
    tracing::debug!(
        interface_name,
        namespace,
        ip = %private_ip_addr_mask,
        "Adding IPv6 address to WireGuard interface in namespace"
    );
    let output = Command::new("ip")
        .arg("netns")
        .arg("exec")
        .arg(namespace)
        .arg("ip")
        .arg("-6")
        .arg("addr")
        .arg("add")
        .arg(&private_ip_addr_mask.to_string())
        .arg("dev")
        .arg(&interface_name)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!(
            interface_name,
            namespace,
            ip = %private_ip_addr_mask,
            stderr = %stderr,
            "Failed to add IPv6 address (address may already be in use)"
        );
        return Err(anyhow!(
            "Failed to add IPv6 address to wireguard interface: {}",
            stderr
        ));
    }

    // Bring the interface up inside the namespace.
    // Retry with backoff on EADDRINUSE — this happens when the kernel hasn't
    // fully released the UDP socket from a recently-deleted WireGuard interface
    // that was using the same port in this namespace.
    const UP_MAX_ATTEMPTS: u8 = 6;
    const UP_INITIAL_DELAY: std::time::Duration = std::time::Duration::from_millis(200);

    let mut up_last_err = String::new();
    let mut up_delay = UP_INITIAL_DELAY;

    for attempt in 0..UP_MAX_ATTEMPTS {
        let output = Command::new("ip")
            .arg("netns")
            .arg("exec")
            .arg(namespace)
            .arg("ip")
            .arg("link")
            .arg("set")
            .arg(&interface_name)
            .arg("up")
            .output()?;

        if output.status.success() {
            if attempt > 0 {
                tracing::info!(
                    interface_name,
                    namespace,
                    attempt,
                    "WireGuard interface came UP after retry"
                );
            }
            up_last_err.clear();
            break;
        }

        up_last_err = String::from_utf8_lossy(&output.stderr).to_string();

        if up_last_err.contains("Address already in use") && attempt < UP_MAX_ATTEMPTS - 1 {
            tracing::warn!(
                interface_name,
                namespace,
                attempt,
                delay_ms = up_delay.as_millis(),
                "WireGuard UP failed with EADDRINUSE; retrying after delay"
            );
            std::thread::sleep(up_delay);
            up_delay = (up_delay * 2).min(std::time::Duration::from_secs(2));
        } else {
            break;
        }
    }

    if !up_last_err.is_empty() {
        return Err(anyhow!(
            "Failed to set wireguard interface status to UP: {}",
            up_last_err
        ));
    }

    // Add route to peer for new interface (Routes are also lost when moving namespaces)
    let output = Command::new("ip")
        .arg("netns")
        .arg("exec")
        .arg(namespace)
        .arg("ip")
        .arg("route")
        .arg("add")
        .arg(&peer_prv_ip_addr_mask.to_string())
        .arg("dev")
        .arg(&interface_name)
        .output()?;
    if !output.status.success() {
        return Err(anyhow!(
            "Failed to add route to peer {} (dev {}): {}",
            peer_prv_ip,
            interface_name,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // NOTE: We do NOT set WireGuard as the default route for VMs.
    // VMs should use their TAP/veth networking for general connectivity,
    // and WireGuard for direct communication with the orchestrator.
    // Setting WireGuard as default would break VM internet access and SSH.

    // All steps succeeded — disarm the guard so the interface is kept.
    guard.disarm();

    Ok(())
}

pub fn wg_teardown(namespace: impl AsRef<str>, wg_interface_name: impl AsRef<str>) {
    let namespace = namespace.as_ref();
    let iface = wg_interface_name.as_ref();

    match Command::new("ip")
        .arg("netns")
        .arg("exec")
        .arg(namespace)
        .arg("ip")
        .arg("link")
        .arg("del")
        .arg(iface)
        .output()
    {
        Ok(output) if output.status.success() => {
            tracing::debug!(namespace, interface = iface, "WireGuard interface deleted");
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // "Cannot find device" is expected if the interface was already cleaned up
            if !stderr.contains("Cannot find device") {
                tracing::warn!(
                    namespace,
                    interface = iface,
                    stderr = %stderr,
                    exit_code = ?output.status.code(),
                    "Failed to delete WireGuard interface during teardown"
                );
            }
        }
        Err(e) => {
            tracing::error!(
                namespace,
                interface = iface,
                error = %e,
                "Failed to execute WireGuard teardown command"
            );
        }
    }
}

/// List all WireGuard interfaces in the global namespace that match the `vm_` prefix.
///
/// In steady state, there should be zero — all successfully-created VM WG interfaces
/// are moved to a network namespace. Any found here are orphans from failed or
/// interrupted `wg_setup` calls.
pub fn list_orphaned_wg_interfaces() -> anyhow::Result<Vec<String>> {
    let output = Command::new("ip")
        .arg("-o")
        .arg("link")
        .arg("show")
        .arg("type")
        .arg("wireguard")
        .output()?;

    if !output.status.success() {
        return Err(anyhow!(
            "Failed to list wireguard interfaces: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let orphans: Vec<String> = stdout
        .lines()
        .filter_map(|line| {
            // Format: "N: vm_XXXX@NONE: <flags> ..."
            // Extract interface name (second field, strip trailing colon and @suffix)
            let name = line.split_whitespace().nth(1)?;
            let name = name.trim_end_matches(':');
            let name = name.split('@').next()?;
            if name.starts_with("vm_") {
                Some(name.to_string())
            } else {
                None
            }
        })
        .collect();

    Ok(orphans)
}

/// Delete a WireGuard interface from the global namespace.
pub fn delete_wg_interface(name: &str) -> anyhow::Result<()> {
    let output = Command::new("ip")
        .arg("link")
        .arg("del")
        .arg(name)
        .output()?;

    if !output.status.success() {
        return Err(anyhow!(
            "Failed to delete wireguard interface {}: {}",
            name,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}
