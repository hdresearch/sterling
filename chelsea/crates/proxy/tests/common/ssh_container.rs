//! SSH server container for integration testing
//!
//! Provides a real SSH server running in a Docker container that we can
//! test SSH-over-TLS forwarding against.

#![allow(dead_code)]

use anyhow::Result;
use std::process::{Command, Stdio};
use testcontainers::{
    GenericImage, ImageExt,
    core::{ContainerPort, WaitFor},
    runners::AsyncRunner,
};

/// SSH server test container configuration
pub struct SshContainer {
    /// The running container
    _container: testcontainers::ContainerAsync<GenericImage>,

    /// SSH port exposed on host
    pub host_port: u16,

    /// Internal SSH port (always 22)
    pub container_port: u16,

    /// SSH username
    pub username: String,

    /// SSH password
    pub password: String,
}

impl SshContainer {
    /// Start a new SSH server container
    ///
    /// Uses the `linuxserver/openssh-server` image which provides:
    /// - OpenSSH server on port 2222 (remapped to 22)
    /// - User authentication with password
    /// - Ready-to-use test environment
    pub async fn start() -> Result<Self> {
        // Use linuxserver/openssh-server - a minimal SSH server for testing
        let image = GenericImage::new("linuxserver/openssh-server", "latest")
            .with_wait_for(WaitFor::message_on_stdout("done."))
            .with_exposed_port(ContainerPort::Tcp(2222));

        // Start the container with environment variables
        let container = image
            .with_env_var("PUID", "1000")
            .with_env_var("PGID", "1000")
            .with_env_var("TZ", "Etc/UTC")
            .with_env_var("PASSWORD_ACCESS", "true")
            .with_env_var("USER_PASSWORD", "testpass")
            .with_env_var("USER_NAME", "testuser")
            .start()
            .await?;

        let host_port = container.get_host_port_ipv4(2222).await?;

        // Wait for SSH server to be ready
        Self::wait_for_ssh_ready(host_port).await?;

        Ok(Self {
            _container: container,
            host_port,
            container_port: 22,
            username: "testuser".to_string(),
            password: "testpass".to_string(),
        })
    }

    /// Wait for SSH server to be ready by attempting connections
    async fn wait_for_ssh_ready(port: u16) -> Result<()> {
        let max_attempts = 30;
        let mut attempts = 0;

        loop {
            attempts += 1;
            if attempts > max_attempts {
                anyhow::bail!("SSH server did not become ready in time");
            }

            // Try to connect with nc (netcat) to see if port is open
            let output = Command::new("nc")
                .args(&["-z", "127.0.0.1", &port.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();

            if output.is_ok() && output.unwrap().success() {
                // Port is open, wait a bit more for SSH to fully initialize
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                println!("[TEST] SSH server ready on port {}", port);
                return Ok(());
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    }

    /// Get the SSH connection string
    pub fn connection_string(&self) -> String {
        format!("{}@127.0.0.1:{}", self.username, self.host_port)
    }

    /// Test SSH connection directly (without proxy)
    ///
    /// Useful for verifying the container is working before testing the proxy
    pub async fn test_direct_connection(&self) -> Result<String> {
        let output = Command::new("sshpass")
            .args(&[
                "-p",
                &self.password,
                "ssh",
                "-o",
                "StrictHostKeyChecking=no",
                "-o",
                "UserKnownHostsFile=/dev/null",
                "-o",
                "PreferredAuthentications=password",
                "-p",
                &self.host_port.to_string(),
                &format!("{}@127.0.0.1", self.username),
                "echo 'SSH connection successful'",
            ])
            .output()?;

        if !output.status.success() {
            anyhow::bail!(
                "SSH connection failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Execute a command via SSH
    pub async fn exec_ssh_command(&self, command: &str) -> Result<String> {
        let output = Command::new("sshpass")
            .args(&[
                "-p",
                &self.password,
                "ssh",
                "-o",
                "StrictHostKeyChecking=no",
                "-o",
                "UserKnownHostsFile=/dev/null",
                "-o",
                "PreferredAuthentications=password",
                "-p",
                &self.host_port.to_string(),
                &format!("{}@127.0.0.1", self.username),
                command,
            ])
            .output()?;

        if !output.status.success() {
            anyhow::bail!(
                "SSH command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Get the container's internal IP address
    ///
    /// This simulates the VM's WireGuard IP in production
    pub fn internal_ip(&self) -> Result<String> {
        // In a real setup, this would be the VM's IPv6 WireGuard address
        // For testing, we'll use the container's IP or localhost
        Ok("127.0.0.1".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that we can start an SSH container and connect to it
    #[tokio::test]
    #[ignore] // Ignore by default since it requires Docker
    async fn test_ssh_container_basic() -> Result<()> {
        let container = SshContainer::start().await?;

        println!("SSH container started on port {}", container.host_port);
        println!("Connection string: {}", container.connection_string());

        // Test direct connection
        let result = container.test_direct_connection().await?;
        assert_eq!(result, "SSH connection successful");

        Ok(())
    }

    /// Test executing commands in the SSH container
    #[tokio::test]
    #[ignore] // Ignore by default since it requires Docker
    async fn test_ssh_container_exec() -> Result<()> {
        let container = SshContainer::start().await?;

        // Execute a simple command
        let result = container.exec_ssh_command("uname -s").await?;
        assert_eq!(result, "Linux");

        // Execute another command
        let result = container.exec_ssh_command("whoami").await?;
        assert_eq!(result, container.username);

        Ok(())
    }
}
