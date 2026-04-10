use std::{os::unix::fs::PermissionsExt, path::Path};

use ssh_key::PrivateKey;
use uuid::Uuid;

use crate::create_temp_file;

pub async fn exec_ssh(
    ssh_private_key: &PrivateKey,
    host: &str,
    command: &str,
) -> anyhow::Result<()> {
    let temp_key_string = ssh_private_key
        .to_openssh(ssh_key::LineEnding::LF)?
        .to_string();

    let temp_key_file =
        create_temp_file(Path::new("/dev/shm").join(format!("id_rsa_{}", Uuid::new_v4())))?;
    tokio::fs::write(&temp_key_file.path, temp_key_string).await?;
    tokio::fs::set_permissions(&temp_key_file.path, std::fs::Permissions::from_mode(0o600)).await?;

    let output = tokio::process::Command::new("ssh")
        .arg("-i")
        .arg(&temp_key_file.path)
        .arg("-o")
        .arg("StrictHostKeyChecking=no")
        .arg("-o")
        .arg("PasswordAuthentication=no")
        .arg("-o")
        .arg("UserKnownHostsFile=/dev/null")
        .arg(format!("root@{host}"))
        .arg(command)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(anyhow::anyhow!(
            "SSH command failed (exit {}): stdout: '{}', stderr: '{}'",
            output.status.code().unwrap_or(-1),
            stdout,
            stderr
        ));
    }

    Ok(())
}
