use std::{error::Error, fmt::Display, net::Ipv4Addr, path::Path};

use tokio::process::Command;

pub async fn execute_vm(
    ssh_key_path: &Path,
    vm_ip: &Ipv4Addr,
    command: &str,
) -> Result<String, ExecuteError> {
    let output = Command::new("ssh")
        .arg("-i")
        .arg(ssh_key_path)
        .arg("-o")
        .arg("StrictHostKeyChecking=no")
        .arg("-o")
        .arg("PasswordAuthentication=no")
        .arg("-o")
        .arg("UserKnownHostsFile=/dev/null")
        .arg(format!("root@{}", vm_ip))
        .arg(command)
        .output()
        .await?;

    if !output.status.success() {
        Err(ExecuteError::Execution(format!(
            "VM execution failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )))
    } else {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

#[derive(Debug)]
pub enum ExecuteError {
    Execution(String),
    Io(std::io::Error),
}

impl From<std::io::Error> for ExecuteError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl Display for ExecuteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Execution(e) => write!(f, "Command execution failed: {e}"),
            Self::Io(e) => write!(f, "Command execution failed: {e}"),
        }
    }
}

impl Error for ExecuteError {}
