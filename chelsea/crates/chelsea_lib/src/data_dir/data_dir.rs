use std::{path::PathBuf, sync::OnceLock};

use tokio::fs::File;
use uuid::Uuid;
use vers_config::VersConfig;

static DATA_DIR: OnceLock<DataDir> = OnceLock::new();

/// A struct representing paths in the data directory
#[derive(Debug)]
pub struct DataDir {
    /// chelsea_monitor logs
    pub monitoring_log_dir: PathBuf,
    /// stdout/stderr logs from VMs
    pub process_log_dir: PathBuf,
    /// kernel directory
    pub kernel_dir: PathBuf,
    /// commits directory
    pub commit_dir: PathBuf,
}

impl DataDir {
    /// Instantiates a new DataDir struct, ensuring that the directories exist
    fn new(
        monitoring_log_dir: PathBuf,
        process_log_dir: PathBuf,
        kernel_dir: PathBuf,
        commit_dir: PathBuf,
    ) -> Result<Self, Vec<std::io::Error>> {
        let errors = [
            &monitoring_log_dir,
            &process_log_dir,
            &kernel_dir,
            &commit_dir,
        ]
        .map(|dir| std::fs::create_dir_all(dir))
        .into_iter()
        .filter_map(|x| x.err())
        .collect::<Vec<_>>();

        match errors.len() {
            0 => Ok(Self {
                monitoring_log_dir,
                process_log_dir,
                kernel_dir,
                commit_dir,
            }),
            _ => Err(errors),
        }
    }

    /// Returns the global DataDir (initialized from the global config). On init, will panic if unable to create the data dir - this is an unrecoverable error
    pub fn global() -> &'static Self {
        let config = VersConfig::chelsea();
        DATA_DIR.get_or_init(|| {
            Self::new(
                config.monitoring_log_dir.clone(),
                config.process_log_dir.clone(),
                config.kernel_dir.clone(),
                config.snapshot_dir.clone(),
            )
            .expect("Failed to create data dirs")
        })
    }

    /// Creates a new stdout/stderr log pair in the process logs subdir
    pub async fn create_process_logs(&self, vm_id: &Uuid) -> std::io::Result<(File, File)> {
        let stdout_filepath = self.process_log_dir.join(format!("{vm_id}.stdout.log"));
        let stderr_filepath = self.process_log_dir.join(format!("{vm_id}.stderr.log"));

        let (stdout_file, stderr_file) = futures::future::join(
            tokio::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .open(stdout_filepath),
            tokio::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .open(stderr_filepath),
        )
        .await;

        Ok((stdout_file?, stderr_file?))
    }
}
