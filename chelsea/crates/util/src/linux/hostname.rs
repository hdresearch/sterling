use anyhow::{Context, Result};
use std::net::IpAddr;
use std::process::Command;
use std::str::FromStr;

/// Returns a Vec<IpAddr> parsed from the output of `hostname -I`.
pub fn get_host_ip_addrs() -> Result<Vec<IpAddr>> {
    let output = Command::new("hostname")
        .arg("-I")
        .output()
        .context("failed to execute `hostname -I`")?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "hostname -I exited with non-0 status: {}",
            output.status
        ));
    }

    let stdout = String::from_utf8(output.stdout)
        .context("output from `hostname -I` was not valid UTF-8")?;

    let addrs = stdout
        .split_whitespace()
        .filter_map(|s| IpAddr::from_str(s).ok())
        .collect();

    Ok(addrs)
}
