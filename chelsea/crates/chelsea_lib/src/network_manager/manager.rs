use std::{net::Ipv4Addr, sync::Arc};

use anyhow::{Context, anyhow, bail};
use tokio::sync::Semaphore;
use tracing::{debug, error, info};
use util::join_errors;

use crate::{
    network::{
        VmNetwork,
        error::InitializeNetworksError,
        linux::nat::{
            add_outbound_masquerade_nat_rule, batch_add_inbound_ssh_nat_rules,
            check_inbound_ssh_nat_rule_exists, delete_outbound_masquerade_nat_rule,
        },
        utils::{enable_ip_forwarding, vm_addr_from_host_addr},
    },
    network_manager::{
        error::ReserveNetworkError,
        network_ranges::NetworkRanges,
        store::{VmNetworkManagerStore, VmNetworkRecord},
        wireguard::wg_teardown,
    },
    vm::VmWireGuardConfig,
};

/// An interface to create and delete VmNetworks. Also establishes appropriate routing rules on creation.
#[derive(Clone)]
pub struct VmNetworkManager {
    network_interface: String,
    pub store: Arc<dyn VmNetworkManagerStore>,
    network_ranges: NetworkRanges,
}

impl VmNetworkManager {
    pub async fn new(
        network_interface: String,
        network_ranges: NetworkRanges,
        store: Arc<dyn VmNetworkManagerStore>,
    ) -> anyhow::Result<Self> {
        enable_ip_forwarding().await?;

        add_outbound_masquerade_nat_rule(&network_ranges.vm_subnet, &network_interface).await?;

        Ok(Self {
            network_interface,
            store,
            network_ranges,
        })
    }

    pub async fn initialize_networks(&self) -> Result<(), InitializeNetworksError> {
        let network_count = self.network_ranges.get_ip_pair_count();
        info!(network_count, "Initializing VM networks in parallel");

        // Create a semaphore to limit concurrency to 16 parallel network creations
        // This prevents overwhelming the system while still providing significant speedup
        let semaphore = Arc::new(Semaphore::new(16));

        // A list of tasks, each of which creates a VM network without adding its SSH rule (nftables calls will be batched later in this function)
        let handles = self.network_ranges.iter().map(|(pair, ssh_port)| {
            let semaphore = semaphore.clone();
            let store = self.store.clone();
            let host_addr = pair.0.addr();

            tokio::spawn(async move {
                let _permit = semaphore.acquire_owned().await.unwrap();

                let result =
                    Self::create_or_validate_vm_network_static_batched(store, &host_addr, ssh_port)
                        .await;

                if let Err(error) = &result {
                    error!(%error, %host_addr, ssh_port, "Error initializing VmNetwork");
                }

                result
            })
        });

        // Aggregate the tasks into a list of created networks and errors
        let results = futures::future::join_all(handles).await;
        let mut networks = Vec::with_capacity(results.len());
        let mut had_error = false;

        for result in results {
            match result {
                Ok(Ok(network)) => networks.push(network),
                Ok(_) => had_error = true, // Errors already logged in task closure
                Err(error) => {
                    had_error = true;
                    error!(%error, "Error joining network creation task");
                }
            }
        }

        // Create an iterator of SSH rules in the expected format (host_ssh_port, ip_addr_dest)
        let rules = networks
            .iter()
            .map(|network| (network.ssh_port, network.vm_addr));

        // Batch apply all SSH NAT rules at once
        if rules.len() > 0 {
            info!(rule_count = rules.len(), "Batch applying SSH NAT rules");

            // Delete all networks after failing to add their inbound SSH DNAT rules
            if let Err(e) = batch_add_inbound_ssh_nat_rules(rules).await {
                error!(%e, "Failed to batch apply SSH NAT rules, cleaning up created networks");
                self.cleanup_networks_on_error(&networks).await;
                return Err(InitializeNetworksError::Other(e.to_string()));
            }
        }

        match had_error {
            false => info!("Network initialization complete."),
            true => error!("Network initialization completed with one or more errors."),
        };

        Ok(())
    }

    /// Cleans up a list of networks by deleting both their kernel resources and database records.
    /// Logs errors but continues cleanup for all networks in a best-effort manner.
    async fn cleanup_networks_on_error(&self, networks: &[VmNetwork]) {
        let cleanup_count = networks.len();
        info!(
            cleanup_count,
            "Cleaning up networks after initialization error"
        );

        for network in networks.iter() {
            // Delete network resources (netns, veth, etc.)
            // Note: delete() will also attempt to delete SSH NAT rules, which is safe even if they weren't created
            if let Err(errors) = network.delete().await {
                error!(
                    host_addr = %network.host_addr,
                    "Errors during network cleanup: {}",
                    join_errors(&errors, "; ")
                );
            }

            // Delete from database
            if let Err(db_error) = self.store.delete_vm_network(&network.host_addr).await {
                error!(
                    %db_error,
                    host_addr = %network.host_addr,
                    "Error removing network from database during cleanup"
                );
            }
        }
    }

    /// Static version of create_or_validate_vm_network for use in parallel tasks with batched SSH NAT rules
    async fn create_or_validate_vm_network_static_batched(
        store: Arc<dyn VmNetworkManagerStore>,
        host_addr: &Ipv4Addr,
        ssh_port: u16,
    ) -> anyhow::Result<VmNetwork> {
        let vm_addr = vm_addr_from_host_addr(host_addr)?;

        // If the network is in the DB, then check that it exists; if so, return its ID. Else, flush the record and continue on with constructing it.
        if let Some(vm_network) = store.fetch_vm_network(host_addr).await? {
            debug!(?vm_network, "Found VmNetwork in NetworkManagerStore");
            // Check that the SSH NAT rule (the final step in this process) exists, else delete the record and start over.
            // Also check that the tap device exists in the netns - it may have been cleaned up if Chelsea was restarted.
            let ssh_rule_exists = check_inbound_ssh_nat_rule_exists(ssh_port, &vm_addr).await?;
            let tap_exists = crate::network::linux::namespace::netns_exec(
                &vm_network.netns_name,
                &["ip", "link", "show", &vm_network.tap_name()],
            )
            .await
            .is_ok();

            if ssh_rule_exists && tap_exists {
                return Ok(vm_network);
            }

            debug!(
                ssh_rule_exists,
                tap_exists,
                "Network validation failed; deleting from NetworkManagerStore and recreating"
            );
            store.delete_vm_network(&host_addr).await?;
        }

        // Create network without SSH NAT rule - we'll batch apply those later
        let vm_network = VmNetwork::new_without_ssh_nat(host_addr.clone(), ssh_port).await?;

        // Add network to database
        if let Err(err) = store
            .insert_vm_network(VmNetworkRecord::from(&vm_network))
            .await
        {
            if let Err(errors) = vm_network.delete().await {
                error!(
                    "One or more errors while cleaning up VmNetwork: {}",
                    join_errors(&errors, "; ")
                )
            };
            bail!(err);
        };

        Ok(vm_network)
    }

    /// Looks for an unused network in the VmManager's store
    pub async fn reserve_network(&self) -> Result<VmNetwork, ReserveNetworkError> {
        match self
            .store
            .reserve_network()
            .await
            .map_err(|e| ReserveNetworkError::Other(e.to_string()))?
        {
            Some(network) => Ok(VmNetwork::from(network)),
            None => Err(ReserveNetworkError::NoneAvailable),
        }
    }

    pub async fn cleanup(&self) -> anyhow::Result<()> {
        delete_outbound_masquerade_nat_rule(&self.network_ranges.vm_subnet, &self.network_interface)
            .await
    }

    pub async fn rehydrate_network(
        &self,
        vm_network_host_addr: &Ipv4Addr,
    ) -> anyhow::Result<VmNetwork> {
        self.store
            .fetch_vm_network(vm_network_host_addr)
            .await?
            .ok_or(anyhow!(
                "Failed to find network with host IP {}",
                vm_network_host_addr
            ))
    }

    /// Callback invoked when a VmNetwork's parent VM is created
    #[tracing::instrument(skip_all)]
    pub async fn on_vm_created(
        &self,
        vm_network_host_addr: &Ipv4Addr,
        wg: VmWireGuardConfig,
    ) -> anyhow::Result<()> {
        tracing::info!(?vm_network_host_addr, "running on_vm_created");

        let mut network = self
            .store
            .fetch_vm_network(vm_network_host_addr)
            .await?
            .ok_or(anyhow!(
                "Failed to find network with host address {vm_network_host_addr}"
            ))?;

        tracing::trace!(wg = ?&wg, "trying to setup wg for vm");

        // wg_setup uses blocking Command::new calls and retry sleeps, so run it
        // off the async runtime to avoid blocking the tokio executor.
        let wg_clone = wg.clone();
        tokio::task::spawn_blocking(move || network.wg_setup(wg_clone))
            .await
            .context("WireGuard setup task panicked")??;

        if self
            .store
            .set_wg_on_vm_network(vm_network_host_addr, Some(wg))
            .await?
            .is_none()
        {
            tracing::warn!(
                ?vm_network_host_addr,
                "on_vm_created: tried to set wg vm network, but couldn't find vm_network"
            );

            bail!("on_vm_created: tried to set wg vm network, but couldn't find vm_network");
        }

        Ok(())
    }

    /// Tear down wireguard and update the corresponding record in the store
    async fn wg_teardown(&self, vm_network_host_addr: &Ipv4Addr) -> anyhow::Result<()> {
        if let Some(mut vm_network) = self.store.fetch_vm_network(vm_network_host_addr).await? {
            if let Some(wg_config) = vm_network.wg.take() {
                wg_teardown(&vm_network.netns_name, &wg_config.interface_name);
            } else {
                tracing::debug!(
                    ?vm_network_host_addr,
                    "on_vm_killed: no wg config attached to vm_network"
                );
            };
        } else {
            bail!(
                "on_vm_killed: tried to fetch vm network to tear down connected wg interface, but couldn't find vm_network"
            );
        };

        if self
            .store
            .set_wg_on_vm_network(vm_network_host_addr, None)
            .await?
            .is_none()
        {
            tracing::warn!(
                ?vm_network_host_addr,
                "on_vm_killed: tried to fetch vm network to tear down connected wg interface, but couldn't find vm_network"
            );
        }

        Ok(())
    }

    /// Callback invoked when a VmNetwork's parent VM is killed
    pub async fn on_vm_killed(&self, vm_network_host_addr: &Ipv4Addr) -> anyhow::Result<()> {
        self.wg_teardown(vm_network_host_addr).await?;
        self.release_reserved_network(vm_network_host_addr).await
    }

    /// Callback invoked when a VmNetwork's parent VM is put to sleep
    pub async fn on_vm_sleep(&self, vm_network_host_addr: &Ipv4Addr) -> anyhow::Result<()> {
        self.wg_teardown(vm_network_host_addr).await?;
        self.release_reserved_network(vm_network_host_addr).await
    }

    pub async fn release_reserved_network(
        &self,
        vm_network_host_addr: &Ipv4Addr,
    ) -> anyhow::Result<()> {
        Ok(self.store.unreserve_network(vm_network_host_addr).await?)
    }

    /// Returns the number of VmNetworks that this VmNetworkManager will create
    pub fn get_vm_network_count(&self) -> usize {
        self.network_ranges.get_ip_pair_count()
    }

    pub async fn on_vm_resumed(&self, _vm_network_host_addr: &Ipv4Addr) -> anyhow::Result<()> {
        // Currently, the network manager does not care about this
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::store_error::StoreError;

    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex as TokioMutex;

    /// Mock store for testing that tracks deletions
    struct MockVmNetworkManagerStore {
        deleted_networks: Arc<TokioMutex<Vec<Ipv4Addr>>>,
    }

    impl MockVmNetworkManagerStore {
        fn new() -> Self {
            Self {
                deleted_networks: Arc::new(TokioMutex::new(Vec::new())),
            }
        }

        fn get_deleted_networks(&self) -> Arc<TokioMutex<Vec<Ipv4Addr>>> {
            self.deleted_networks.clone()
        }
    }

    #[async_trait::async_trait]
    impl VmNetworkManagerStore for MockVmNetworkManagerStore {
        async fn insert_vm_network(&self, _vm_network: VmNetworkRecord) -> Result<(), StoreError> {
            Ok(())
        }

        async fn fetch_vm_network(
            &self,
            _host_addr: &Ipv4Addr,
        ) -> Result<Option<VmNetwork>, StoreError> {
            Ok(None)
        }

        async fn check_vm_network_exists(&self, _host_addr: &Ipv4Addr) -> Result<bool, StoreError> {
            Ok(false)
        }

        async fn delete_vm_network(&self, host_addr: &Ipv4Addr) -> Result<(), StoreError> {
            self.deleted_networks.lock().await.push(*host_addr);
            Ok(())
        }

        async fn reserve_network(&self) -> Result<Option<VmNetwork>, StoreError> {
            Ok(None)
        }

        async fn unreserve_network(&self, _host_addr: &Ipv4Addr) -> Result<(), StoreError> {
            Ok(())
        }

        async fn set_wg_on_vm_network(
            &self,
            _host_addr: &Ipv4Addr,
            _vm_network: Option<VmWireGuardConfig>,
        ) -> Result<Option<()>, StoreError> {
            Ok(Some(()))
        }
    }

    #[tokio::test]
    async fn test_cleanup_networks_on_error() {
        // Create a mock store
        let mock_store = Arc::new(MockVmNetworkManagerStore::new());
        let deleted_networks = mock_store.get_deleted_networks();

        // Create a test network manager
        let network_ranges = NetworkRanges::new(
            "10.0.0.0/30".parse().unwrap(), // 4 IPs, 2 pairs
            3000..3002,                     // 2 ports
        )
        .unwrap();

        let manager = VmNetworkManager {
            network_interface: "eth0".to_string(),
            store: mock_store.clone(),
            network_ranges,
        };

        // Create some test networks to clean up
        let test_networks = vec![
            VmNetwork {
                host_addr: Ipv4Addr::new(10, 0, 0, 0),
                vm_addr: Ipv4Addr::new(10, 0, 0, 1),
                netns_name: "test_netns_1".to_string(),
                ssh_port: 3000,
                wg: None,
            },
            VmNetwork {
                host_addr: Ipv4Addr::new(10, 0, 0, 2),
                vm_addr: Ipv4Addr::new(10, 0, 0, 3),
                netns_name: "test_netns_2".to_string(),
                ssh_port: 3001,
                wg: None,
            },
        ];

        // Call cleanup_networks_on_error
        manager.cleanup_networks_on_error(&test_networks).await;

        // Verify that both networks were deleted from the store
        let deleted = deleted_networks.lock().await;
        assert_eq!(deleted.len(), 2);
        assert!(deleted.contains(&Ipv4Addr::new(10, 0, 0, 0)));
        assert!(deleted.contains(&Ipv4Addr::new(10, 0, 0, 2)));
    }

    #[tokio::test]
    async fn test_cleanup_networks_on_error_empty_list() {
        // Create a mock store
        let mock_store = Arc::new(MockVmNetworkManagerStore::new());
        let deleted_networks = mock_store.get_deleted_networks();

        // Create a test network manager
        let network_ranges =
            NetworkRanges::new("10.0.0.0/30".parse().unwrap(), 3000..3002).unwrap();

        let manager = VmNetworkManager {
            network_interface: "eth0".to_string(),
            store: mock_store.clone(),
            network_ranges,
        };

        // Call cleanup with empty list
        manager.cleanup_networks_on_error(&[]).await;

        // Verify no deletions occurred
        let deleted = deleted_networks.lock().await;
        assert_eq!(deleted.len(), 0);
    }

    #[tokio::test]
    async fn test_cleanup_networks_continues_on_db_error() {
        // Create a store that fails on the first deletion
        struct FailingMockStore {
            deleted_networks: Arc<TokioMutex<Vec<Ipv4Addr>>>,
            fail_count: Arc<TokioMutex<usize>>,
        }

        impl FailingMockStore {
            fn new() -> Self {
                Self {
                    deleted_networks: Arc::new(TokioMutex::new(Vec::new())),
                    fail_count: Arc::new(TokioMutex::new(1)), // Fail once
                }
            }
        }

        #[async_trait::async_trait]
        impl VmNetworkManagerStore for FailingMockStore {
            async fn insert_vm_network(
                &self,
                _vm_network: VmNetworkRecord,
            ) -> Result<(), StoreError> {
                Ok(())
            }

            async fn fetch_vm_network(
                &self,
                _host_addr: &Ipv4Addr,
            ) -> Result<Option<VmNetwork>, StoreError> {
                Ok(None)
            }

            async fn check_vm_network_exists(
                &self,
                _host_addr: &Ipv4Addr,
            ) -> Result<bool, StoreError> {
                Ok(false)
            }

            async fn delete_vm_network(&self, host_addr: &Ipv4Addr) -> Result<(), StoreError> {
                let mut fail_count = self.fail_count.lock().await;
                if *fail_count > 0 {
                    *fail_count -= 1;
                    return Err(StoreError::from_display("Simulated deletion failure"));
                }
                self.deleted_networks.lock().await.push(*host_addr);
                Ok(())
            }

            async fn reserve_network(&self) -> Result<Option<VmNetwork>, StoreError> {
                Ok(None)
            }

            async fn set_wg_on_vm_network(
                &self,
                _host_addr: &Ipv4Addr,
                _vm_network: Option<VmWireGuardConfig>,
            ) -> Result<Option<()>, StoreError> {
                Ok(None)
            }

            async fn unreserve_network(&self, _host_addr: &Ipv4Addr) -> Result<(), StoreError> {
                Ok(())
            }
        }

        let mock_store = Arc::new(FailingMockStore::new());
        let deleted_networks = mock_store.deleted_networks.clone();

        let network_ranges = NetworkRanges::new(
            "10.0.0.0/29".parse().unwrap(), // 8 IPs, 4 pairs
            3000..3004,
        )
        .unwrap();

        let manager = VmNetworkManager {
            network_interface: "eth0".to_string(),
            store: mock_store.clone(),
            network_ranges,
        };

        let test_networks = vec![
            VmNetwork {
                host_addr: Ipv4Addr::new(10, 0, 0, 0),
                vm_addr: Ipv4Addr::new(10, 0, 0, 1),
                netns_name: "test_netns_1".to_string(),
                ssh_port: 3000,
                wg: None,
            },
            VmNetwork {
                host_addr: Ipv4Addr::new(10, 0, 0, 2),
                vm_addr: Ipv4Addr::new(10, 0, 0, 3),
                netns_name: "test_netns_2".to_string(),
                ssh_port: 3001,
                wg: None,
            },
        ];

        // Cleanup should continue despite the first deletion failing
        manager.cleanup_networks_on_error(&test_networks).await;

        // Verify the second network was still deleted (first one failed)
        let deleted = deleted_networks.lock().await;
        assert_eq!(deleted.len(), 1);
        assert!(deleted.contains(&Ipv4Addr::new(10, 0, 0, 2)));
    }
}
