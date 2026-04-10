use std::path::Path;

use async_tempfile::TempFile;
use sysinfo::System;
use thiserror::Error;
use tokio::io::AsyncWriteExt;

const LOCKFILE_DIR: &str = "/run";
const LOCKFILE_NAME: &str = "chelsea.pid";

#[derive(Error, Debug)]
pub enum CreateLockfileError {
    #[error("Failed to read lockfile: {0}")]
    ReadLockfile(std::io::Error),
    #[error("Failed to parse lockfile contents as PID: {0}")]
    ParseContents(#[from] std::num::ParseIntError),
    #[error(
        "Lockfile is present; suggests another instance of this process is running at PID {0}. If this is an error, you may remove {LOCKFILE_NAME} from the temp directory."
    )]
    AlreadyRunning(u32),
    #[error("Error while removing stale lockfile")]
    RemoveStaleLockfile(std::io::Error),
    #[error("Failed to create lockfile: {0}")]
    CreateLockfile(async_tempfile::Error),
    #[error("Failed to write PID to lockfile: {0}")]
    WriteLockfile(std::io::Error),
}

pub async fn create_lockfile() -> Result<TempFile, CreateLockfileError> {
    let lockfile_path = Path::new(LOCKFILE_DIR).join(LOCKFILE_NAME);
    if lockfile_path.exists() {
        // Before returning an error, check to see if that process is actually still running
        let other_pid: u32 = tokio::fs::read_to_string(&lockfile_path)
            .await
            .map_err(CreateLockfileError::ReadLockfile)?
            .parse()?;

        let sys = System::new_all();

        if sys.process(sysinfo::Pid::from_u32(other_pid)).is_some() {
            return Err(CreateLockfileError::AlreadyRunning(other_pid));
        } else {
            tokio::fs::remove_file(lockfile_path)
                .await
                .map_err(CreateLockfileError::RemoveStaleLockfile)?;
        }
    }

    let mut lockfile = TempFile::new_with_name_in(LOCKFILE_NAME, Path::new(LOCKFILE_DIR))
        .await
        .map_err(|e| CreateLockfileError::CreateLockfile(e))?;
    let pid = std::process::id();
    lockfile
        .write_all(format!("{pid}").as_bytes())
        .await
        .map_err(CreateLockfileError::WriteLockfile)?;

    Ok(lockfile)
}
