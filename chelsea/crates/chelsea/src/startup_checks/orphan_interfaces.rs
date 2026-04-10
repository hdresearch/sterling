use chelsea_lib::network_manager::wireguard::{delete_wg_interface, list_orphaned_wg_interfaces};
use tracing::{debug, info, warn};

/// Clean up any orphaned WireGuard interfaces left in the global namespace
/// from prior crashes or failed `wg_setup` calls.
///
/// In steady state, all VM WireGuard interfaces (`vm_*`) should be inside
/// a network namespace — never in the global namespace. Any found here are
/// leftovers from interrupted setup that didn't complete the netns move,
/// or from a process crash between `create_interface()` and cleanup.
///
/// This should be called early in startup, before `initialize_networks()`.
pub fn cleanup_orphaned_wg_interfaces() -> anyhow::Result<u32> {
    let orphans = list_orphaned_wg_interfaces()?;

    if orphans.is_empty() {
        debug!("No orphaned WireGuard interfaces found in global namespace");
        return Ok(0);
    }

    warn!(
        count = orphans.len(),
        interfaces = ?orphans,
        "Found orphaned WireGuard interfaces in global namespace, cleaning up"
    );

    let mut cleaned = 0u32;
    for name in &orphans {
        match delete_wg_interface(name) {
            Ok(()) => {
                info!(interface = %name, "Deleted orphaned WireGuard interface");
                cleaned += 1;
            }
            Err(e) => {
                tracing::error!(
                    interface = %name,
                    error = %e,
                    "Failed to delete orphaned WireGuard interface"
                );
            }
        }
    }

    warn!(
        cleaned,
        total = orphans.len(),
        "Orphaned WireGuard interface cleanup complete"
    );

    Ok(cleaned)
}
