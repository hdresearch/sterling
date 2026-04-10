use std::{string::FromUtf8Error, sync::OnceLock};

use ceph::{RbdClientError, default_rbd_client};
use dto_lib::chelsea_server2::system::ChelseaVersion;
use regex::Regex;
use thiserror::Error;
use tokio::process::Command;

static RE_JAILER: OnceLock<Regex> = OnceLock::new();
static RE_FIRECRACKER: OnceLock<Regex> = OnceLock::new();
static RE_CEPH: OnceLock<Regex> = OnceLock::new();

fn re_jailer() -> &'static Regex {
    RE_JAILER.get_or_init(|| Regex::new(r"Jailer v(\d+\.\d+\.\d+)").unwrap())
}

fn re_firecracker() -> &'static Regex {
    RE_FIRECRACKER.get_or_init(|| Regex::new(r"Firecracker v(\d+\.\d+\.\d+)").unwrap())
}

fn re_ceph() -> &'static Regex {
    RE_CEPH.get_or_init(|| Regex::new(r"ceph version (\d+\.\d+\.\d+)").unwrap())
}

#[derive(Debug, Error)]
pub enum GetVersionError {
    #[error("IO error while getting version: {0}")]
    Exec(#[from] std::io::Error),
    #[error("Error parsing command output: {0}")]
    Utf8(#[from] FromUtf8Error),
    #[error("Ceph error while getting version info: {0}")]
    Ceph(#[from] RbdClientError),
}

pub async fn get_jailer_version() -> Result<String, GetVersionError> {
    let jailer_output = Command::new("jailer")
        .arg("--version")
        .output()
        .await?
        .stdout;
    let jailer_output_str = String::from_utf8(jailer_output)?;

    Ok(re_jailer()
        .captures(&jailer_output_str)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| jailer_output_str))
}

pub async fn get_firecracker_version() -> Result<String, GetVersionError> {
    let firecracker_output = Command::new("firecracker")
        .arg("--version")
        .output()
        .await?
        .stdout;
    let firecracker_output_str = String::from_utf8(firecracker_output)?;

    Ok(re_firecracker()
        .captures(&firecracker_output_str)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| firecracker_output_str))
}

pub async fn get_ceph_client_version() -> Result<String, GetVersionError> {
    let client = default_rbd_client()?;
    let ceph_client_version = client.ceph_client_version().await?;

    Ok(re_ceph()
        .captures(&ceph_client_version)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| ceph_client_version))
}

pub async fn get_ceph_cluster_version() -> Result<String, GetVersionError> {
    let client = default_rbd_client()?;
    let ceph_cluster_version = client.ceph_cluster_version().await?;

    Ok(re_ceph()
        .captures(&ceph_cluster_version)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| ceph_cluster_version))
}

pub async fn get_all_versions() -> Result<ChelseaVersion, GetVersionError> {
    let workspace_version = workspace_build::workspace_version().to_string();
    let git_hash = workspace_build::git_hash().to_string();
    let jailer_version = get_jailer_version().await?;
    let firecracker_version = get_firecracker_version().await?;
    let ceph_client_version = get_ceph_client_version().await?;
    let ceph_cluster_version = get_ceph_cluster_version().await?;

    Ok(ChelseaVersion {
        executable_name: "chelsea".to_string(),
        workspace_version,
        git_hash,
        jailer_version,
        firecracker_version,
        ceph_client_version,
        ceph_cluster_version,
    })
}
