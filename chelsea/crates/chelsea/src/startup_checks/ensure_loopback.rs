use std::net::Ipv4Addr;

use anyhow::Context;

/// Assigns the provided Ipv4Addr to the loopback device if it's not already present.
pub async fn ensure_ipv4_on_loopback(ip: &Ipv4Addr) -> anyhow::Result<()> {
    let ip_str = ip.to_string();

    // Check if already assigned
    let output = tokio::process::Command::new("ip")
        .arg("addr")
        .arg("show")
        .arg("dev")
        .arg("lo")
        .output()
        .await
        .context(format!(
            "Checking if addr {ip_str} is assigned to loopback device."
        ))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!(
            "Failed to query loopback device for IP address. Exit status: {:?}, stdout: {}, stderr: {}",
            output.status,
            stdout,
            stderr
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.contains(&ip_str) {
        return Ok(());
    }

    // Assign the IP to loopback
    let output = tokio::process::Command::new("ip")
        .arg("addr")
        .arg("add")
        .arg(format!("{}/32", ip_str))
        .arg("dev")
        .arg("lo")
        .output()
        .await
        .context(format!("Assigning IP address {ip_str} to loopback device"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout_str = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!(
            "Failed to assign IP {} to loopback device. Exit status: {:?}, stdout: {}, stderr: {}",
            ip_str,
            output.status,
            stdout_str,
            stderr
        );
    }

    Ok(())
}
