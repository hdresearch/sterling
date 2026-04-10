use std::{
    net::{Ipv4Addr, Ipv6Addr},
    process::Command,
};

use crate::{
    network::{
        error::NewVmNetworkError,
        linux::{
            namespace::{
                netns_add, netns_del, netns_enable_packet_forwarding, netns_name_from_host_addr,
                netns_set_default_route,
            },
            nat::{add_inbound_ssh_nat_rule, delete_inbound_ssh_nat_rule},
            tap::{tap_add_in_namespace, tap_ensure_in_namespace},
            veth::{veth_add_peer_netns, veth_vm_name_from_vm_addr},
        },
        utils::ipv4_to_mac,
    },
    network_manager::{
        store::VmNetworkRecord,
        wireguard::{wg_setup, wg_teardown},
    },
    vm::VmWireGuardConfig,
};
use ipnet::{Ipv4Net, Ipv6Net};
use macaddr::MacAddr6;
use tracing::error;
use util::defer::DeferAsync;

// The following variables are hard-coded because these are non-configurable constants. Because we snapshot VMs, we must
// guarantee that all VMs believe they have the same IP address and gateway. Base images depend on these values. Do not change.
pub const TAP_NAME: &str = "tap0";
pub const TAP_NET_V4: Ipv4Net = Ipv4Net::new_assert(Ipv4Addr::new(192, 168, 1, 1), 30);
pub const TAP_NET_V6: Ipv6Net =
    Ipv6Net::new_assert(Ipv6Addr::new(64768, 65041, 57069, 4919, 0, 0, 0, 1), 126); // fd00:fe11:deed:1337::1/126
pub const GUEST_ADDR_V4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 2);
pub const GUEST_ADDR_V6: Ipv6Addr = Ipv6Addr::new(64768, 65041, 57069, 4919, 0, 0, 0, 2); // fd00:fe11:deed:1337::2/126

// This can't be 'Clone' since it mutates (wg) itself.
/// Represents a configured network and all related devices to be passed to a VM.
#[derive(Debug)]
pub struct VmNetwork {
    pub host_addr: Ipv4Addr,
    pub vm_addr: Ipv4Addr,
    pub netns_name: String,
    pub ssh_port: u16,
    // WG is required for networks "bound" to VMs, but since networks are always created at chelsea
    // boot, when a VmNetwork isn't "bound" to a VM this is None.
    pub wg: Option<VmWireGuardConfig>,
}

impl VmNetwork {
    pub async fn new(host_addr: Ipv4Addr, ssh_port: u16) -> Result<Self, NewVmNetworkError> {
        Self::new_internal(host_addr, ssh_port, true).await
    }

    /// Create a new VmNetwork without adding the SSH NAT rule (for batched initialization)
    pub async fn new_without_ssh_nat(
        host_addr: Ipv4Addr,
        ssh_port: u16,
    ) -> Result<Self, NewVmNetworkError> {
        Self::new_internal(host_addr, ssh_port, false).await
    }

    async fn new_internal(
        host_addr: Ipv4Addr,
        ssh_port: u16,
        add_ssh_nat: bool,
    ) -> Result<Self, NewVmNetworkError> {
        // Validate that host_addr is the lower address in its /31 subnet
        let host_net = Ipv4Net::new_assert(host_addr.clone(), 31);
        if host_net.network() != host_net.addr() {
            return Err(NewVmNetworkError::InvalidInput(format!(
                "Host IP {host_net} must be the lower address in a point-to-point (RFC 3021) subnet",
            )));
        }

        // Derive vm_addr from host_addr by assigning the higher address to it
        let vm_net = Ipv4Net::new_assert(host_net.broadcast(), 31);
        let vm_addr = vm_net.addr();

        let mut defer = DeferAsync::new();

        // Create namespace
        let netns_name = netns_name_from_host_addr(&host_addr);
        netns_add(&netns_name).await?;
        defer.defer({
            let netns_name = netns_name.clone();
            async move {
                if let Err(error) = netns_del(netns_name.clone()).await {
                    error!(%error, "Error while cleaning up netns");
                }
            }
        });

        // Create veth pair in netns; no defer needed
        veth_add_peer_netns(&host_addr, &netns_name).await?;

        // Add default route in namespace via veth pair
        netns_set_default_route(&netns_name, &host_addr).await?;

        // Create TAP device in netns; no defer needed
        tap_add_in_namespace(TAP_NAME, &TAP_NET_V4, &TAP_NET_V6, &netns_name).await?;

        // Enable packet forwarding and create DNAT rules in the network to forward packets to the VM
        netns_enable_packet_forwarding(
            &netns_name,
            veth_vm_name_from_vm_addr(&vm_addr),
            &GUEST_ADDR_V4,
            &GUEST_ADDR_V6,
        )
        .await?;

        // Set up SSH inbound NAT rule on the host (only if requested)
        if add_ssh_nat {
            add_inbound_ssh_nat_rule(ssh_port, &vm_addr).await?;
        }

        defer.commit();

        Ok(Self {
            host_addr,
            vm_addr,
            netns_name,
            ssh_port,
            wg: None,
        })
    }

    /// Sets up WG resources on the network and attaches the config to this VmNetwork instance.
    #[tracing::instrument(skip(self, wg_config))]
    pub fn wg_setup(&mut self, wg_config: VmWireGuardConfig) -> anyhow::Result<()> {
        tracing::info!(namespace = ?self.netns_name, "setting up wg on namespace");
        // Store a copy of configuration for future teardown
        wg_setup(
            &wg_config.interface_name,
            wg_config.wg_port,
            &wg_config.private_key,
            wg_config.private_ip.clone(),
            &wg_config.peer_pub_key,
            wg_config.peer_pub_ip.clone(),
            wg_config.peer_ipv6.clone(),
            &self.netns_name,
        )?;

        // Add a DNAT rule to masquerade the Wireguard interface's traffic to the VM's address
        let result = Command::new("ip")
            .arg("netns")
            .arg("exec")
            .arg(&self.netns_name)
            .arg("ip6tables")
            .arg("-t")
            .arg("nat")
            .arg("-A")
            .arg("PREROUTING")
            .arg("-i")
            .arg(&wg_config.interface_name)
            .arg("-d")
            .arg(&wg_config.private_ip.to_string())
            .arg("-j")
            .arg("DNAT")
            .arg("--to-destination")
            .arg(GUEST_ADDR_V6.to_string())
            .output();

        self.wg = Some(wg_config);

        if let Err(e) = result {
            return Err(anyhow::anyhow!("Failed to add WG DNAT rule: {:?}", e));
        }
        let output = result.unwrap();
        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "ip6tables DNAT rule failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(())
    }

    /// Tear down any WG resources set up on the network
    #[tracing::instrument(skip(self))]
    pub fn wg_teardown(&mut self) -> anyhow::Result<()> {
        tracing::info!(namespace = ?self.netns_name, "tearing down wg on namespace");
        if let Some(wg_config) = self.wg.take() {
            tracing::debug!(interface = ?&wg_config.interface_name, "removing interface");
            wg_teardown(&self.netns_name, &wg_config.interface_name);
        } else {
            tracing::warn!("There was no wg config attached to vm_network")
        };
        Ok(())
    }

    /// Deletes the VmNetwork's netns as well as the corresponding inbound SSH NAT rule
    pub async fn delete(&self) -> Result<(), Vec<anyhow::Error>> {
        let results = tokio::join!(
            delete_inbound_ssh_nat_rule(self.ssh_port, &self.vm_addr),
            netns_del(&self.netns_name),
        );

        let errors: Vec<anyhow::Error> = vec![results.0.err(), results.1.err()]
            .into_iter()
            .filter_map(|r| r)
            .collect();

        match errors.is_empty() {
            true => Ok(()),
            false => Err(errors),
        }
    }

    pub fn guest_mac(&self) -> MacAddr6 {
        ipv4_to_mac(&GUEST_ADDR_V4)
    }

    pub fn tap_name(&self) -> String {
        TAP_NAME.to_string()
    }

    /// Ensure the TAP device exists in this network's namespace.
    /// This handles the case where TAP was deleted (e.g., when a VM process exits)
    /// but the namespace is being reused for a new VM.
    /// Should be called before spawning a VM to ensure network connectivity.
    pub async fn ensure_tap(&self) -> anyhow::Result<()> {
        tap_ensure_in_namespace(TAP_NAME, &TAP_NET_V4, &TAP_NET_V6, &self.netns_name).await
    }
}

impl From<VmNetworkRecord> for VmNetwork {
    fn from(value: VmNetworkRecord) -> Self {
        Self {
            host_addr: Ipv4Addr::from_bits(value.host_addr),
            vm_addr: Ipv4Addr::from_bits(value.vm_addr),
            netns_name: value.netns_name,
            ssh_port: value.ssh_port,
            wg: value.wg,
        }
    }
}
