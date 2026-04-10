use anyhow::{anyhow, Result};
use tokio::process::Command;

pub async fn get_all_network_interfaces() -> Result<Vec<String>> {
    let output = Command::new("ip").arg("link").output().await?;
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let interfaces: Vec<_> = stdout_str
        .lines()
        .step_by(2)
        .filter_map(|line| {
            line.split_whitespace()
                .nth(1)
                .map(|interface_name| interface_name.replace(":", ""))
        })
        .collect();

    Ok(interfaces)
}

pub async fn get_primary_network_interface() -> Result<String> {
    get_all_network_interfaces().await.and_then(|interfaces| {
        interfaces.get(1).cloned().ok_or(anyhow!(
            "Failed to determine primary network interface using ip link"
        ))
    })
}
