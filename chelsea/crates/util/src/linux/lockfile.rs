use std::path::{Path, PathBuf};

use sysinfo::System;
use thiserror::Error;

use crate::{create_temp_file, temp_file::TempFile};

#[derive(Error, Debug)]
pub enum CreateLockfileError {
    #[error("Failed to read lockfile: {0}")]
    ReadLockfile(std::io::Error),
    #[error("Failed to parse lockfile contents as PID: {0}")]
    ParseContents(#[from] std::num::ParseIntError),
    #[error("Lockfile is present; suggests another instance of this process is running at PID {0}. If this is an error, you may remove {1}")]
    AlreadyRunning(u32, PathBuf),
    #[error("Error while removing stale lockfile")]
    RemoveStaleLockfile(std::io::Error),
    #[error("Failed to create lockfile: {0}")]
    CreateLockfile(std::io::Error),
    #[error("Failed to write PID to lockfile: {0}")]
    WriteLockfile(std::io::Error),
}

pub async fn create_lockfile(lockfile_path: &Path) -> Result<TempFile, CreateLockfileError> {
    if lockfile_path.exists() {
        // Before returning an error, check to see if that process is actually still running
        let other_pid: u32 = tokio::fs::read_to_string(lockfile_path)
            .await
            .map_err(CreateLockfileError::ReadLockfile)?
            .parse()?;

        let sys = System::new_all();

        if sys.process(sysinfo::Pid::from_u32(other_pid)).is_some() {
            return Err(CreateLockfileError::AlreadyRunning(
                other_pid,
                lockfile_path.to_path_buf(),
            ));
        } else {
            tokio::fs::remove_file(lockfile_path)
                .await
                .map_err(CreateLockfileError::RemoveStaleLockfile)?;
        }
    }

    let lockfile = create_temp_file(lockfile_path.to_path_buf())
        .map_err(CreateLockfileError::CreateLockfile)?;
    let pid = std::process::id();
    tokio::fs::write(&lockfile.path, format!("{pid}").as_bytes())
        .await
        .map_err(CreateLockfileError::WriteLockfile)?;

    Ok(lockfile)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn creates_lockfile_with_current_pid() {
        let dir = tempfile::tempdir().unwrap();
        let lockfile_path = dir.path().join("test.lock");

        let lockfile = create_lockfile(&lockfile_path).await.unwrap();
        assert!(lockfile.path.exists());

        let contents = tokio::fs::read_to_string(&lockfile.path).await.unwrap();
        let pid: u32 = contents.parse().unwrap();
        assert_eq!(pid, std::process::id());
    }

    #[tokio::test]
    async fn lockfile_cleaned_up_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let lockfile_path = dir.path().join("test.lock");

        let lockfile = create_lockfile(&lockfile_path).await.unwrap();
        let path_clone = lockfile.path.clone();
        assert!(path_clone.exists());

        drop(lockfile);
        assert!(!path_clone.exists());
    }

    #[tokio::test]
    async fn rejects_lockfile_when_process_is_running() {
        let dir = tempfile::tempdir().unwrap();
        let lockfile_path = dir.path().join("test.lock");

        // Create a lockfile with our own PID (which is definitely running)
        let _lockfile = create_lockfile(&lockfile_path).await.unwrap();

        // Second attempt should fail with AlreadyRunning
        let result = create_lockfile(&lockfile_path).await;
        assert!(
            matches!(result, Err(CreateLockfileError::AlreadyRunning(_, _))),
            "expected AlreadyRunning, got {result:?}"
        );
    }

    #[tokio::test]
    async fn removes_stale_lockfile_from_dead_process() {
        let dir = tempfile::tempdir().unwrap();
        let lockfile_path = dir.path().join("test.lock");

        // Write a lockfile with a PID that (almost certainly) doesn't exist
        // PID 4194304 is above the typical Linux max (default max_pid is 4194304, so this won't exist)
        tokio::fs::write(&lockfile_path, "4194300").await.unwrap();

        // Should succeed by cleaning up the stale lockfile
        let lockfile = create_lockfile(&lockfile_path).await.unwrap();
        let contents = tokio::fs::read_to_string(&lockfile.path).await.unwrap();
        assert_eq!(contents.parse::<u32>().unwrap(), std::process::id());

        drop(lockfile);
    }
}
