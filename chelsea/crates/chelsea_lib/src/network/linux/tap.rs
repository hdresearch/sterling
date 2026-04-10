use ipnet::{Ipv4Net, Ipv6Net};
use tracing::debug;

use crate::network::{linux::namespace::netns_exec, utils::ipv4_to_mac};

/// Check if TAP device exists in namespace
pub async fn tap_exists_in_namespace(
    tap_name: impl AsRef<str>,
    netns_name: impl AsRef<str>,
) -> bool {
    netns_exec(
        netns_name.as_ref(),
        &["ip", "link", "show", tap_name.as_ref()],
    )
    .await
    .is_ok()
}

/// Ensure TAP device exists in namespace with a fresh state.
/// This handles cases where:
/// 1. TAP was deleted (e.g., when the hypervisor process exits) - recreate it
/// 2. TAP exists but is stale (previous process just exited, kernel hasn't fully released) - delete and recreate
///
/// By always deleting and recreating, we ensure the TAP is in a clean state that
/// the hypervisor can open. This prevents EBUSY errors when the previous VM's
/// process has exited but kernel cleanup hasn't completed.
pub async fn tap_ensure_in_namespace(
    tap_name: impl AsRef<str>,
    tap_net_v4: &Ipv4Net,
    tap_net_v6: &Ipv6Net,
    netns_name: impl AsRef<str>,
) -> anyhow::Result<()> {
    let tap_name = tap_name.as_ref();
    let netns_name = netns_name.as_ref();

    if tap_exists_in_namespace(tap_name, netns_name).await {
        // Delete existing tap to ensure a fresh state.
        // This handles stale taps from previous VMs where the process exited
        // but kernel cleanup hasn't completed (would cause EBUSY on vm.boot).
        debug!(
            tap_name,
            netns_name, "TAP device exists in namespace; deleting for fresh state"
        );
        let _ = netns_exec(netns_name, &["ip", "link", "del", tap_name]).await;
    }

    debug!(
        tap_name,
        netns_name, "Creating fresh TAP device in namespace"
    );
    tap_add_in_namespace(tap_name, tap_net_v4, tap_net_v6, netns_name).await?;

    Ok(())
}

pub async fn tap_add_in_namespace(
    tap_name: impl AsRef<str>,
    tap_net_v4: &Ipv4Net,
    tap_net_v6: &Ipv6Net,
    netns_name: impl AsRef<str>,
) -> anyhow::Result<()> {
    let tap_name = tap_name.as_ref();
    let netns_name = netns_name.as_ref();

    // Firecracker setup script in the VM used to expect a particular MAC address format; this is no longer the case, so this can
    // almost certainly be unset and simply assigned randomly by the OS now.
    let mac_address = ipv4_to_mac(&tap_net_v4.addr());

    // Create the TAP device
    match netns_exec(
        netns_name,
        &["ip", "tuntap", "add", "dev", tap_name, "mode", "tap"],
    )
    .await
    {
        Ok(_) => {}
        Err(e) => {
            let err_str = format!("{:?}", e);
            if !err_str.contains("Device or resource busy") {
                return Err(e);
            }
            debug!(netns_name, "TAP already exists; flushing address");
            netns_exec(netns_name, &["ip", "addr", "flush", "dev", tap_name]).await?;
        }
    }

    // Set IP and MAC address
    netns_exec(
        netns_name,
        &[
            "ip",
            "addr",
            "add",
            &tap_net_v4.to_string(),
            "dev",
            tap_name,
        ],
    )
    .await?;
    netns_exec(
        netns_name,
        &[
            "ip",
            "addr",
            "add",
            &tap_net_v6.to_string(),
            "dev",
            tap_name,
        ],
    )
    .await?;

    netns_exec(
        netns_name,
        &[
            "ip",
            "link",
            "set",
            "dev",
            tap_name,
            "address",
            &mac_address.to_string(),
        ],
    )
    .await?;

    // Set device to UP state
    netns_exec(netns_name, &["ip", "link", "set", tap_name, "up"]).await?;

    Ok(())
}
