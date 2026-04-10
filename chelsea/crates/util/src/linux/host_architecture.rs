use anyhow::{anyhow, bail};
use nix::sys::utsname;

pub async fn get_host_cpu_architecture() -> anyhow::Result<String> {
    let uts = utsname::uname().map_err(|e| anyhow!("Failed to query uname: {}", e))?;
    let arch = uts.machine();

    let arch_str = arch
        .to_str()
        .ok_or_else(|| anyhow!("Unexpected non UTF-8 architecture string"))?
        .trim()
        .to_string();

    if arch_str.is_empty() {
        bail!("uname returned an empty architecture string");
    }

    Ok(arch_str)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn arch_matches_with_exec() {
        let arch = get_host_cpu_architecture()
            .await
            .expect("expected uname lookup to succeed");

        assert!(
            !arch.is_empty(),
            "expected architecture string to be non-empty"
        );
        let output = std::process::Command::new("uname")
            .arg("-m")
            .output()
            .expect("expected to execute uname -m");
        assert!(
            output.status.success(),
            "uname -m failed with status {status:?}, stderr: {stderr}",
            status = output.status,
            stderr = String::from_utf8_lossy(&output.stderr),
        );

        let expected = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let message = format!("nix uname: {arch}, exec uname: {expected}");
        println!("{message}");
        assert_eq!(arch, expected, "{message}");
    }
}
