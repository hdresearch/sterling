use std::{
    net::{IpAddr, SocketAddr, SocketAddrV4},
    sync::Arc,
};

use anyhow::anyhow;
use chelsea_db::ChelseaDb;
use chelsea_lib::{
    commit_store::VmCommitStore,
    network_manager::{manager::VmNetworkManager, network_ranges::NetworkRanges},
    process_manager::VmProcessManager,
    s3_store::S3SnapshotStore,
    sleep_snapshot_store::VmSleepSnapshotStore,
    system_service::SystemService,
    vm_manager::VmManager,
};
use chelsea_lib::{ready_service::VmReadyService, volume_manager::ceph::CephVmVolumeManager};
use chelsea_vmevent_server::ChelseaVmEventServer;
use mulberry::Mulberry;
use timing_layer::TimingLayer;
use tracing::{debug, error, info};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};
use util::linux::get_primary_network_interface;
use vers_config::VersConfig;
use vers_pg::db::VersPg;

use crate::{
    bootstrap::setup_chelsea_server_wireguard,
    server_core::ConcreteServerCore,
    startup_checks::{
        cleanup_orphaned_wg_interfaces, create_lockfile, ensure_ipv4_on_loopback,
        ensure_vm_cgroup_exists, get_or_create_identity, validate_vm_provisioning_parameters,
    },
    timing_log_writer::get_timing_layer,
};

mod bootstrap;
mod server_core;
mod startup_checks;
mod timing_log_writer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // fetch_fs.sh depends on this
    let exe = std::env::current_exe()?;
    unsafe {
        std::env::set_var(
            "CHELSEA_BIN_DIR",
            exe.parent()
                .ok_or(anyhow!("Unable to deduce CHELSEA_BIN_DIR"))?,
        );
    }

    let config = VersConfig::chelsea();

    // Initialize timing layer
    let timing_layer: Option<TimingLayer> = get_timing_layer();

    // Initialize tracing subscribers
    if let Some(layer) = timing_layer {
        tracing::subscriber::set_global_default(
            tracing_subscriber::registry()
                .with(EnvFilter::from_default_env())
                .with(
                    fmt::layer()
                        .with_ansi(!cfg!(orch_test))
                        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE),
                )
                .with(layer),
        )
        .expect("failed to set global tracing subscriber");
    } else {
        tracing::subscriber::set_global_default(
            tracing_subscriber::registry()
                .with(EnvFilter::from_default_env())
                .with(
                    fmt::layer()
                        .with_ansi(!cfg!(orch_test))
                        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE),
                ),
        )
        .expect("failed to set global tracing subscriber");
    }

    // Ensure that there is no other instance of the process is running
    let _lockfile = create_lockfile().await.unwrap();

    // Ensure that VM provisioning parameters are valid
    validate_vm_provisioning_parameters()?;

    // Ensure VM cgroup exists for process isolation
    ensure_vm_cgroup_exists(&config.vm_cgroup_name, config.vm_cgroup_cpu_weight).await?;

    // Create DB connections
    let chelsea_db = ChelseaDb::instance().await;
    let vers_pg = Arc::new(VersPg::new().await?);

    // Initialize network manager
    let network_interface = get_primary_network_interface().await?;
    debug!(network_interface, "Found primary network interface");
    let network_manager = Arc::new(
        VmNetworkManager::new(
            network_interface,
            NetworkRanges::new(config.vm_subnet, config.vm_ssh_port_range.clone())?,
            chelsea_db.clone(),
        )
        .await?,
    );

    // Clean up any orphaned WireGuard interfaces from prior crashes/failures.
    // Must run before initialize_networks() to avoid conflicts.
    cleanup_orphaned_wg_interfaces()?;

    info!(
        "Setting up VM networks (this will take a while, especially if you've allocated a larger VM subnet."
    );
    network_manager.initialize_networks().await?;

    // Construct event server address and requisite chelsea_notify_boot_url_template variable (see notify-ready.service)
    let event_server_addr = SocketAddrV4::new(config.event_server_addr, config.event_server_port);
    let chelsea_notify_boot_url_template =
        ChelseaVmEventServer::chelsea_notify_boot_url_template(&event_server_addr.into());

    // Initialize resource managers/services
    let process_manager = Arc::new(VmProcessManager::new(chelsea_db.clone(), vers_pg.clone()));
    let system_service = Arc::new(SystemService::default());
    let volume_manager = Arc::new(CephVmVolumeManager::new(chelsea_db.clone()).await?);
    volume_manager.start_pool().await;
    let s3_client = util::s3::get_s3_client().await.clone();
    let s3_snapshot_store = Arc::new(
        S3SnapshotStore::new(
            config.snapshot_dir.clone(),
            config.snapshot_dir_max_size_mib,
            s3_client,
        )
        .await?,
    );
    let ready_service = Arc::new(VmReadyService::new(
        chelsea_db.clone(),
        chelsea_notify_boot_url_template,
    ));
    let volume_manager_for_shutdown = volume_manager.clone();
    let hypervisor_type = config.hypervisor_type;
    let vm_manager = Arc::new(VmManager {
        local_store: chelsea_db,
        remote_store: vers_pg.clone(),
        network_manager: network_manager.clone(),
        process_manager,
        system_service: system_service.clone(),
        volume_manager,
        commit_store: s3_snapshot_store.clone() as Arc<dyn VmCommitStore>,
        sleep_snapshot_store: s3_snapshot_store as Arc<dyn VmSleepSnapshotStore>,
        ready_service: Arc::clone(&ready_service),
        vm_locks: std::sync::Mutex::new(std::collections::HashMap::new()),
        hypervisor_type,
    });

    // Create the "server core" (Interface for server to interact with chelsea logic.)
    let core = Arc::new(ConcreteServerCore {
        vm_manager: vm_manager.clone(),
    });

    // Fetch information about this node from Postgres. If not found, insert a record instead
    let node_record = get_or_create_identity(
        &vers_pg,
        system_service.get_vcpu_count().await as i32,
        system_service.get_total_and_available_ram().await.0 as i64,
        system_service.get_total_and_available_disk().await.0 as i64,
        network_manager.get_vm_network_count() as i32,
    )
    .await?;

    // Create the chelsea server's Wireguard interface
    let chelsea_server_port = config.server_port;
    let (_chelsea_server_wg, chelsea_server_addr) = (
        // Hold the WG struct; its Drop impl will automatically remove the interface
        setup_chelsea_server_wireguard(node_record.wg_ipv6.clone(), node_record.wg_private_key)?,
        SocketAddr::new(IpAddr::V6(node_record.wg_ipv6), chelsea_server_port),
    );

    // Start the chelsea server
    let chelsea_server_handle = tokio::spawn({
        let core = core.clone();
        async move {
            if let Err(error) = chelsea_server2::run_server(core, chelsea_server_addr).await {
                error!(%error, "ChelseaServer2 task exited with error");
            };
        }
    });

    // Create the VM event server
    ensure_ipv4_on_loopback(event_server_addr.ip()).await?;
    let event_server = ChelseaVmEventServer::new(core, event_server_addr.into());

    // Start the event server
    let event_server_handle = tokio::spawn(async move {
        if let Err(error) = event_server.start().await {
            error!(%error, "EventServer task exited with error");
        }
    });

    // Create a host monitoring system
    let mulberry = Mulberry::new(vm_manager, &node_record.node_id);

    // Start the monitoring system
    let mulberry_handle = mulberry.start_all();

    info!("Chelsea started successfully. Press Ctrl+C to exit");

    let err = tokio::select! {
        result = chelsea_server_handle => result.err().map(anyhow::Error::from),
        result = event_server_handle => result.err().map(anyhow::Error::from),
        () = mulberry_handle => None,
        result = tokio::signal::ctrl_c() => result.err().map(anyhow::Error::from)
    };

    // Graceful shutdown
    info!("Shutting down...");
    volume_manager_for_shutdown.shutdown_pool().await;

    match err {
        Some(err) => Err(err),
        None => Ok(()),
    }
}
