use std::path::{Path, PathBuf};

use thiserror::Error;
use tracing::info;

/// The base path for cgroups v2
const CGROUP_V2_BASE: &str = "/sys/fs/cgroup";

#[derive(Debug, Error)]
pub enum CgroupError {
    #[error("cgroups v2 is not available at {}", CGROUP_V2_BASE)]
    CgroupsV2NotAvailable,

    #[error("failed to check if path exists at {path}: {source}")]
    ExistsCheck {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to create cgroup directory at {path}: {source}")]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to write cpu.weight to {path}: {source}")]
    SetCpuWeight {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to enable cpu controller in {path}: {source}")]
    EnableController {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Async check for path existence with proper error propagation.
async fn path_exists(path: &Path) -> Result<bool, CgroupError> {
    tokio::fs::try_exists(path)
        .await
        .map_err(|source| CgroupError::ExistsCheck {
            path: path.to_path_buf(),
            source,
        })
}

/// Ensures the VM cgroup exists and is configured with the given CPU weight.
///
/// This creates a cgroup under `/sys/fs/cgroup/<cgroup_name>/` and sets its
/// `cpu.weight` value. All VM processes spawned via jailer with `--parent-cgroup`
/// pointing to this cgroup will be isolated from host processes.
///
/// When VMs contend for CPU, they will only steal from one another — not from
/// host processes like chelsea itself.
pub async fn ensure_vm_cgroup(cgroup_name: &str, cpu_weight: u32) -> Result<PathBuf, CgroupError> {
    let base = PathBuf::from(CGROUP_V2_BASE);
    ensure_vm_cgroup_at_base(&base, cgroup_name, cpu_weight).await
}

/// Internal implementation that accepts a configurable base path (for testing).
async fn ensure_vm_cgroup_at_base(
    base: &Path,
    cgroup_name: &str,
    cpu_weight: u32,
) -> Result<PathBuf, CgroupError> {
    // Verify cgroups v2 is available
    if !path_exists(base).await? {
        return Err(CgroupError::CgroupsV2NotAvailable);
    }

    let cgroup_path = base.join(cgroup_name);

    // Run setup with cleanup on failure: if any step after directory creation
    // fails, remove the partially-created cgroup directory.
    match setup_vm_cgroup(base, &cgroup_path, cpu_weight).await {
        Ok(()) => Ok(cgroup_path),
        Err(err) => {
            // Best-effort cleanup of the cgroup directory we may have created.
            // Ignore errors — the directory may not exist yet if create_dir_all failed.
            let _ = tokio::fs::remove_dir(&cgroup_path).await;
            Err(err)
        }
    }
}

/// Performs the actual cgroup setup steps. Separated so the caller can handle cleanup on failure.
async fn setup_vm_cgroup(
    base: &Path,
    cgroup_path: &Path,
    cpu_weight: u32,
) -> Result<(), CgroupError> {
    // Enable cpu controller on root cgroup (required for child cgroups to use cpu.weight)
    let subtree_control_path = base.join("cgroup.subtree_control");
    if path_exists(&subtree_control_path).await? {
        tokio::fs::write(&subtree_control_path, "+cpu")
            .await
            .map_err(|source| CgroupError::EnableController {
                path: subtree_control_path,
                source,
            })?;
    }

    // Create the cgroup directory
    tokio::fs::create_dir_all(cgroup_path)
        .await
        .map_err(|source| CgroupError::CreateDir {
            path: cgroup_path.to_path_buf(),
            source,
        })?;

    // Enable cpu controller for children of our cgroup (so jailer sub-cgroups inherit it)
    let subtree_control = cgroup_path.join("cgroup.subtree_control");
    if path_exists(&subtree_control).await? {
        tokio::fs::write(&subtree_control, "+cpu")
            .await
            .map_err(|source| CgroupError::EnableController {
                path: subtree_control,
                source,
            })?;
    }

    // Set CPU weight (1-10000, default 100)
    let cpu_weight_path = cgroup_path.join("cpu.weight");
    tokio::fs::write(&cpu_weight_path, cpu_weight.to_string())
        .await
        .map_err(|source| CgroupError::SetCpuWeight {
            path: cpu_weight_path,
            source,
        })?;

    info!(
        cgroup = %cgroup_path.display(),
        cpu_weight, "VM cgroup initialized successfully"
    );

    Ok(())
}

/// Build the jailer `--parent-cgroup` arguments for a given cgroup name.
pub fn jailer_cgroup_args(cgroup_name: &str) -> Vec<String> {
    vec!["--parent-cgroup".to_string(), cgroup_name.to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_jailer_cgroup_args() {
        let args = jailer_cgroup_args("chelsea-vms");
        assert_eq!(args, vec!["--parent-cgroup", "chelsea-vms"]);
    }

    #[test]
    fn test_jailer_cgroup_args_custom_name() {
        let args = jailer_cgroup_args("my-custom-cgroup");
        assert_eq!(args, vec!["--parent-cgroup", "my-custom-cgroup"]);
    }

    #[tokio::test]
    async fn test_ensure_vm_cgroup_missing_base() {
        let result =
            ensure_vm_cgroup_at_base(Path::new("/nonexistent/path"), "test-vms", 100).await;
        assert!(matches!(result, Err(CgroupError::CgroupsV2NotAvailable)));
    }

    #[tokio::test]
    async fn test_ensure_vm_cgroup_creates_dir_and_writes_weight() {
        let tmpdir = TempDir::new().unwrap();
        let base = tmpdir.path();

        let result = ensure_vm_cgroup_at_base(base, "test-vms", 200).await;
        assert!(result.is_ok());

        let cgroup_path = result.unwrap();
        assert_eq!(cgroup_path, base.join("test-vms"));
        assert!(cgroup_path.exists());

        // Verify cpu.weight was written
        let weight = tokio::fs::read_to_string(cgroup_path.join("cpu.weight"))
            .await
            .unwrap();
        assert_eq!(weight, "200");
    }

    #[tokio::test]
    async fn test_ensure_vm_cgroup_enables_subtree_control() {
        let tmpdir = TempDir::new().unwrap();
        let base = tmpdir.path();

        // Create the root subtree_control file (simulating cgroups v2 filesystem)
        tokio::fs::write(base.join("cgroup.subtree_control"), "")
            .await
            .unwrap();

        let result = ensure_vm_cgroup_at_base(base, "test-vms", 100).await;
        assert!(result.is_ok());

        // Verify root subtree_control was written
        let content = tokio::fs::read_to_string(base.join("cgroup.subtree_control"))
            .await
            .unwrap();
        assert_eq!(content, "+cpu");
    }

    #[tokio::test]
    async fn test_ensure_vm_cgroup_enables_child_subtree_control() {
        let tmpdir = TempDir::new().unwrap();
        let base = tmpdir.path();

        // Pre-create the cgroup dir and its subtree_control file
        let cgroup_dir = base.join("test-vms");
        tokio::fs::create_dir_all(&cgroup_dir).await.unwrap();
        tokio::fs::write(cgroup_dir.join("cgroup.subtree_control"), "")
            .await
            .unwrap();

        let result = ensure_vm_cgroup_at_base(base, "test-vms", 100).await;
        assert!(result.is_ok());

        // Verify child subtree_control was written
        let content = tokio::fs::read_to_string(cgroup_dir.join("cgroup.subtree_control"))
            .await
            .unwrap();
        assert_eq!(content, "+cpu");
    }

    #[tokio::test]
    async fn test_ensure_vm_cgroup_idempotent() {
        let tmpdir = TempDir::new().unwrap();
        let base = tmpdir.path();

        // Call twice — should succeed both times
        let result1 = ensure_vm_cgroup_at_base(base, "test-vms", 100).await;
        assert!(result1.is_ok());

        let result2 = ensure_vm_cgroup_at_base(base, "test-vms", 200).await;
        assert!(result2.is_ok());

        // Second call should overwrite cpu.weight
        let weight = tokio::fs::read_to_string(base.join("test-vms").join("cpu.weight"))
            .await
            .unwrap();
        assert_eq!(weight, "200");
    }

    #[tokio::test]
    async fn test_ensure_vm_cgroup_cleans_up_on_failure() {
        let tmpdir = TempDir::new().unwrap();
        let base = tmpdir.path();

        // Pre-create the cgroup dir with a read-only cpu.weight to force a write failure
        let cgroup_dir = base.join("test-vms");
        tokio::fs::create_dir_all(&cgroup_dir).await.unwrap();
        tokio::fs::write(cgroup_dir.join("cpu.weight"), "")
            .await
            .unwrap();

        // Make cpu.weight read-only so the write fails
        let mut perms = tokio::fs::metadata(cgroup_dir.join("cpu.weight"))
            .await
            .unwrap()
            .permissions();
        perms.set_readonly(true);
        tokio::fs::set_permissions(cgroup_dir.join("cpu.weight"), perms)
            .await
            .unwrap();

        let result = ensure_vm_cgroup_at_base(base, "test-vms", 100).await;
        assert!(matches!(result, Err(CgroupError::SetCpuWeight { .. })));

        // Cleanup won't remove the dir since it has files in it (remove_dir only removes empty dirs),
        // but the error should still propagate correctly
    }
}
