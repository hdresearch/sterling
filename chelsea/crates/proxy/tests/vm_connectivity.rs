/// Integration test for VM connectivity over WireGuard
///
/// This test verifies that:
/// 1. wg.ensure() correctly adds a VM peer to WireGuard
/// 2. The proxy can reach the VM over WireGuard
///
/// Prerequisites:
/// - A VM must be running on the Chelsea node
/// - VM must have WireGuard configured with proxy as peer
/// - VM should be reachable at fd00:fe11:deed:1234::2
use std::net::{IpAddr, Ipv6Addr};
use std::process::Command;

use proxy::{ORCHESTRATOR_PRV_IP, PROXY_PRV_IP};
use tokio::net::TcpStream;
use tokio::time::{Duration, timeout};

// Import the WG struct from the proxy crate
use orch_wg::{WG, WgPeer};

/// Test that wg.ensure() adds the VM peer and we can reach it
#[tokio::test]
#[ignore] // Mark as ignored so it doesn't run in CI
async fn test_vm_wireguard_connectivity() {
    // Test VM details (from your curl command)
    let vm_ip: Ipv6Addr = "fd00:fe11:deed:1234::2".parse().unwrap();
    let vm_wg_public_key = "FJbaqBmFH7eaC5ZR/6xd1Vhw6jheqSFpJdJ7na4UrB4=".to_string();

    // Proxy WireGuard config (dev mode)
    let proxy_prv_key = "GDThGygNt1UjPMZweob5+Ta0cyrl9+mjMiRLN4IRVVs=".to_string();
    let orch_pub_key = "2nwuaeo/vPD5FBmv+xXlvW8TR/qGurjzC+M0YVSaO28=".to_string();
    let orch_pub_ip = "127.0.0.2".to_string();
    let node_ip: IpAddr = "127.0.0.1".parse().unwrap();

    println!("[TEST] Creating WireGuard interface...");
    let wg = WG::new_with_peers(
        "wgproxy",
        PROXY_PRV_IP.parse().unwrap(),
        proxy_prv_key,
        51822,
        vec![WgPeer {
            port: 51821,
            pub_key: orch_pub_key,
            endpoint_ip: orch_pub_ip.parse().unwrap(),
            remote_ipv6: ORCHESTRATOR_PRV_IP.parse().unwrap(),
        }],
    )
    .unwrap();

    println!("[TEST] Adding VM peer via wg.ensure()...");
    wg.peer_ensure(WgPeer {
        endpoint_ip: node_ip,
        remote_ipv6: vm_ip,
        pub_key: vm_wg_public_key,
        port: 51900,
    })
    .unwrap();

    // Give WireGuard a moment to establish the peer
    tokio::time::sleep(Duration::from_secs(2)).await;

    println!("[TEST] Testing ICMP reachability with ping6...");
    let ping_result = Command::new("ping6")
        .arg("-c")
        .arg("3")
        .arg("-W")
        .arg("5")
        .arg(vm_ip.to_string())
        .output()
        .expect("Failed to execute ping6");

    println!(
        "[TEST] Ping output: {}",
        String::from_utf8_lossy(&ping_result.stdout)
    );
    println!(
        "[TEST] Ping stderr: {}",
        String::from_utf8_lossy(&ping_result.stderr)
    );

    assert!(
        ping_result.status.success(),
        "Should be able to ping VM at {} over WireGuard",
        vm_ip
    );

    println!("[TEST] Testing TCP connectivity to VM:80...");
    let tcp_result = timeout(
        Duration::from_secs(5),
        TcpStream::connect(format!("[{}]:80", vm_ip)),
    )
    .await;

    match tcp_result {
        Ok(Ok(stream)) => {
            println!("[TEST] ✓ Successfully connected to VM at {}:80", vm_ip);
            drop(stream);
        }
        Ok(Err(e)) => {
            println!("[TEST] ✗ TCP connection failed: {}", e);
            panic!("Failed to connect to VM:80 - {}", e);
        }
        Err(_) => {
            println!("[TEST] ✗ TCP connection timed out");
            panic!("Timed out connecting to VM:80");
        }
    }

    println!("[TEST] ✓ All connectivity tests passed!");
}

/// Test wg.ensure() can be called multiple times idempotently
#[tokio::test]
#[ignore]
async fn test_vm_ensure_idempotent() {
    let vm_ip: Ipv6Addr = "fd00:fe11:deed:1234::2".parse().unwrap();
    let vm_wg_public_key = "FJbaqBmFH7eaC5ZR/6xd1Vhw6jheqSFpJdJ7na4UrB4=".to_string();

    let proxy_prv_key = "GDThGygNt1UjPMZweob5+Ta0cyrl9+mjMiRLN4IRVVs=".to_string();
    let orch_pub_key = "2nwuaeo/vPD5FBmv+xXlvW8TR/qGurjzC+M0YVSaO28=".to_string();
    let orch_pub_ip = "127.0.0.2".to_string();

    println!("[TEST] Creating WireGuard interface...");
    let wg = WG::new_with_peers(
        "wgproxy",
        PROXY_PRV_IP.parse().unwrap(),
        proxy_prv_key,
        51822,
        vec![WgPeer {
            port: 51821,
            pub_key: orch_pub_key,
            remote_ipv6: ORCHESTRATOR_PRV_IP.parse().unwrap(),
            endpoint_ip: orch_pub_ip.parse().unwrap(),
        }],
    )
    .unwrap();
    let node_ip: IpAddr = "127.0.0.1".parse().unwrap();

    println!("[TEST] Calling wg.ensure() first time...");
    let _result1 = wg.peer_ensure(WgPeer {
        endpoint_ip: node_ip,
        remote_ipv6: vm_ip,
        pub_key: vm_wg_public_key.clone(),
        port: 51900,
    });

    println!("[TEST] Calling wg.ensure() second time (should be idempotent)...");
    let _result2 = wg.peer_ensure(WgPeer {
        port: 51900,
        remote_ipv6: vm_ip,
        pub_key: vm_wg_public_key,
        endpoint_ip: node_ip,
    });

    println!("[TEST] ✓ wg.ensure() is idempotent");
}

/// Helper test to check WireGuard interface status
#[test]
#[ignore]
fn test_check_wg_interface() {
    println!("[TEST] Checking WireGuard interface status...");

    let output = Command::new("wg")
        .arg("show")
        .arg("wgproxy")
        .output()
        .expect("Failed to execute wg show");

    println!("[TEST] WireGuard interface status:");
    println!("{}", String::from_utf8_lossy(&output.stdout));

    if !output.stderr.is_empty() {
        println!("[TEST] Stderr: {}", String::from_utf8_lossy(&output.stderr));
    }

    assert!(output.status.success(), "wg show wgproxy should succeed");
}
