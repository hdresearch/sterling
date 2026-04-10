use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};

use tokio::{
    process::Command,
    time::{sleep, timeout},
};
use tracing::{debug, warn};
use vers_config::VersConfig;

use crate::{
    RbdClientError, RbdSnapName,
    types::{RbdImageInfo, RbdImageStatus, RbdSnapshotInfo},
};

fn get_keyring_path_by_id(id: &str) -> PathBuf {
    Path::new("/etc/ceph").join(format!("ceph.client.{id}.keyring"))
}

static RBD_CLIENT: OnceLock<Result<RbdClient, RbdClientError>> = OnceLock::new();
pub fn default_rbd_client() -> Result<&'static RbdClient, RbdClientError> {
    RBD_CLIENT
        .get_or_init(|| {
            RbdClient::new(
                "chelsea".to_string(),
                "rbd".to_string(),
                Duration::from_secs(VersConfig::chelsea().ceph_client_timeout_secs),
            )
        })
        .as_ref()
        .map_err(|e| e.to_owned())
}

/// A struct that enables access to a Ceph RBD pool. By creating a client with id `id`, we will check for the existence of a file at /etc/ceph/ceph.client.{id}.keyring.
#[derive(Debug)]
pub struct RbdClient {
    id: String,
    pool_name: String,
    timeout_duration: Duration,
}

impl RbdClient {
    /// Create a new rbd client; suggested values id: "chelsea", pool: "rbd"
    pub fn new(
        id: String,
        pool_name: String,
        timeout_duration: Duration,
    ) -> Result<Self, RbdClientError> {
        let keyring_path = get_keyring_path_by_id(&id);
        if !keyring_path.exists() {
            Err(RbdClientError::KeyringNotFound(keyring_path))
        } else {
            Ok(Self {
                id,
                pool_name,
                timeout_duration,
            })
        }
    }

    /// Execute an rbd command as this client, using "rbd --id $id ...".
    async fn exec_rbd<I, S>(&self, args: I) -> Result<String, RbdClientError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = timeout(
            self.timeout_duration,
            Command::new("rbd")
                .arg("--id")
                .arg(&self.id)
                .args(args)
                .output(),
        )
        .await
        .map_err(|_| RbdClientError::Exec("rbd command timed out".to_string()))?
        .map_err(|e| RbdClientError::Exec(e.to_string()))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code().unwrap_or(-1);
            if stderr.contains("(2) No such file or directory") {
                return Err(RbdClientError::NotFound(stderr));
            }
            Err(RbdClientError::ExitCode(exit_code, stdout, stderr))
        } else {
            Ok(stdout)
        }
    }

    /// Execute a ceph command as this client, using "ceph --user $id ...".
    async fn exec_ceph<I, S>(&self, args: I) -> Result<String, RbdClientError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = timeout(
            self.timeout_duration,
            Command::new("ceph")
                .arg("--user")
                .arg(&self.id)
                .args(args)
                .output(),
        )
        .await
        .map_err(|_| RbdClientError::Exec("ceph command timed out".to_string()))?
        .map_err(|e| RbdClientError::Exec(e.to_string()))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code().unwrap_or(-1);
            Err(RbdClientError::ExitCode(exit_code, stdout, stderr))
        } else {
            Ok(stdout)
        }
    }

    /// rbd namespace create $pool_name/$namespace
    /// Creates a namespace in the pool. Namespaces are used to organize images by owner.
    pub async fn namespace_create(&self, namespace: &str) -> Result<(), RbdClientError> {
        debug!(%namespace, pool_name = %self.pool_name, "Creating RBD namespace");

        let namespace_spec = format!("{}/{}", self.pool_name, namespace);
        self.exec_rbd(["namespace", "create", namespace_spec.as_str()])
            .await
            .map(|_| ())
    }

    /// rbd namespace ls $pool_name - check if namespace exists
    /// Returns true if the namespace exists, false otherwise.
    pub async fn namespace_exists(&self, namespace: &str) -> Result<bool, RbdClientError> {
        debug!(%namespace, pool_name = %self.pool_name, "Checking if RBD namespace exists");

        let output = self
            .exec_rbd(["namespace", "ls", self.pool_name.as_str()])
            .await?;
        let namespaces: Vec<&str> = output.lines().map(|l| l.trim()).collect();
        Ok(namespaces.contains(&namespace))
    }

    /// Ensure a namespace exists, creating it if necessary.
    /// This is idempotent - safe to call multiple times.
    pub async fn namespace_ensure(&self, namespace: &str) -> Result<(), RbdClientError> {
        if !self.namespace_exists(namespace).await? {
            // Try to create - might race with another caller, so ignore "already exists" errors
            match self.namespace_create(namespace).await {
                Ok(()) => {
                    debug!(%namespace, "Created RBD namespace");
                    Ok(())
                }
                Err(RbdClientError::ExitCode(_, _, ref stderr))
                    if stderr.contains("already exists") =>
                {
                    debug!(%namespace, "RBD namespace already exists (race condition)");
                    Ok(())
                }
                Err(e) => Err(e),
            }
        } else {
            Ok(())
        }
    }

    /// rbd ls $pool_name
    pub async fn image_list(&self) -> Result<Vec<String>, RbdClientError> {
        let output = self.exec_rbd(["ls", self.pool_name.as_str()]).await?;
        let images = output
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Ok(images)
    }

    /// rbd create $pool_name/$image_name --size $size
    pub async fn image_create(
        &self,
        image_name: &str,
        size_mib: u32,
    ) -> Result<(), RbdClientError> {
        debug!(%image_name, %size_mib, "Creating RBD image");

        let image_spec = format!("{}/{}", self.pool_name, image_name);
        self.exec_rbd([
            "create",
            image_spec.as_str(),
            "--size",
            size_mib.to_string().as_str(),
        ])
        .await
        .map(|_| ())
    }

    /// rbd rm $pool_name/$image_name - image must have no snaps
    pub async fn image_remove(&self, image_name: &str) -> Result<(), RbdClientError> {
        debug!(%image_name, pool_name = %self.pool_name, "Removing RBD image");

        let image_spec = format!("{}/{}", self.pool_name, image_name);
        self.exec_rbd(["rm", image_spec.as_str()]).await.map(|_| ())
    }

    /// rbd info $pool_name/$image_name - returns Ok(true) if exists, Ok(false) if not.
    pub async fn image_exists(&self, image_name: &str) -> Result<bool, RbdClientError> {
        debug!(%image_name, "Checking if RBD image exists");

        let image_spec = format!("{}/{}", self.pool_name, image_name);
        let output = self.exec_rbd(["info", image_spec.as_str()]).await;

        match output {
            Ok(_) => Ok(true),
            Err(RbdClientError::ExitCode(_, _, _)) | Err(RbdClientError::NotFound(_)) => Ok(false),
            Err(other) => Err(other),
        }
    }

    /// rbd info $pool_name/$image_name
    pub async fn image_info(&self, image_name: &str) -> Result<RbdImageInfo, RbdClientError> {
        let image_spec = format!("{}/{}", self.pool_name, image_name);

        let output = self
            .exec_rbd(["info", image_spec.as_str(), "--format", "json"])
            .await?;

        serde_json::from_str(&output)
            .map_err(|e| RbdClientError::Exec(format!("Failed to parse rbd info json: {e}")))
    }

    /// rbd snap ls $pool_name/image_name
    pub async fn snap_list(&self, image_name: &str) -> Result<Vec<RbdSnapName>, RbdClientError> {
        let image_spec = format!("{}/{}", self.pool_name, image_name);
        let output = self
            .exec_rbd(["snap", "ls", image_spec.as_str(), "--format", "json"])
            .await?;

        let snaps: Vec<RbdSnapshotInfo> = serde_json::from_str(&output)
            .map_err(|e| RbdClientError::Exec(format!("Failed to parse snap ls json: {e}")))?;

        let snap_names: Vec<RbdSnapName> = snaps
            .into_iter()
            .map(|snap| RbdSnapName {
                image_name: image_name.to_string(),
                snap_name: snap.name,
            })
            .collect();

        Ok(snap_names)
    }

    /// Constructs a snapshot spec in the format required by RBD commands.
    ///
    /// The snap_name.image_name may contain a namespace path (e.g., "owner_id/my-image").
    /// This function constructs the full spec: pool/[namespace/]image@snap
    ///
    /// Examples:
    /// - Non-namespaced: pool="rbd", image="my-image", snap="snap1" -> "rbd/my-image@snap1"
    /// - Namespaced: pool="rbd", image="owner_id/my-image", snap="snap1" -> "rbd/owner_id/my-image@snap1"
    fn format_snap_spec(&self, snap_name: &RbdSnapName) -> String {
        format!(
            "{}/{}@{}",
            self.pool_name, snap_name.image_name, snap_name.snap_name
        )
    }

    /// rbd snap create $pool_name/[namespace/]$image_name@$snap_name
    ///
    /// The image_name in snap_name may include a namespace prefix (e.g., "owner_id/my-image")
    /// for images stored in RBD namespaces.
    pub async fn snap_create(&self, snap_name: &RbdSnapName) -> Result<(), RbdClientError> {
        let snap_spec = self.format_snap_spec(snap_name);
        debug!(%snap_spec, "Creating RBD snapshot");

        self.exec_rbd(["snap", "create", &snap_spec])
            .await
            .map(|_| ())
    }

    /// rbd snap rm $pool_name/[namespace/]$image_name@$snap_name - snap must not be protected
    pub async fn snap_remove(&self, snap_name: &RbdSnapName) -> Result<(), RbdClientError> {
        let snap_spec = self.format_snap_spec(snap_name);
        debug!(%snap_spec, "Removing RBD snapshot");

        self.exec_rbd(["snap", "rm", &snap_spec]).await.map(|_| ())
    }

    /// rbd snap protect $pool_name/[namespace/]$image_name@$snap_name
    pub async fn snap_protect(&self, snap_name: &RbdSnapName) -> Result<(), RbdClientError> {
        let snap_spec = self.format_snap_spec(snap_name);
        debug!(%snap_spec, "Protecting RBD snapshot");

        self.exec_rbd(["snap", "protect", &snap_spec])
            .await
            .map(|_| ())
    }

    /// rbd snap unprotect $pool_name/[namespace/]$image_name@$snap_name - snap must not have child clones
    pub async fn snap_unprotect(&self, snap_name: &RbdSnapName) -> Result<(), RbdClientError> {
        let snap_spec = self.format_snap_spec(snap_name);
        debug!(%snap_spec, "Unprotecting RBD snapshot");

        self.exec_rbd(["snap", "unprotect", &snap_spec])
            .await
            .map(|_| ())
    }

    /// rbd clone $pool_name/[namespace/]$image_name@$snap_name $pool_name/$new_image_name
    ///
    /// Clones a protected snapshot to create a new image. The source image may be in a
    /// namespace (e.g., "owner_id/base-image") while the destination is typically at
    /// the pool root (e.g., "vm-uuid").
    ///
    /// Uses clone format v2 to support cross-namespace clones, which is required when
    /// the source and destination are in different namespaces.
    pub async fn snap_clone(
        &self,
        snap_name: &RbdSnapName,
        new_image_name: &str,
    ) -> Result<(), RbdClientError> {
        let snap_spec = self.format_snap_spec(snap_name);
        let clone_spec = format!("{}/{}", self.pool_name, new_image_name);
        debug!(%snap_spec, %clone_spec, "Cloning RBD snapshot (format v2)");

        self.exec_rbd([
            "clone",
            "--rbd-default-clone-format",
            "2",
            &snap_spec,
            &clone_spec,
        ])
        .await
        .map(|_| ())
    }

    /// rbd snap purge $pool_name/$image_name - removes all snaps created of image
    pub async fn snap_purge(&self, image_name: &str) -> Result<(), RbdClientError> {
        debug!(%image_name, pool_name = %self.pool_name, "Purging all RBD snapshots for image");

        let image_spec = format!("{}/{}", self.pool_name, image_name);
        self.exec_rbd(["snap", "purge", image_spec.as_str()])
            .await
            .map(|_| ())
    }

    /// rbd map $pool_name/$image_name - returns the path to the device that was created
    pub async fn device_map(&self, image_name: &str) -> Result<PathBuf, RbdClientError> {
        debug!(%image_name, pool_name = %self.pool_name, "Mapping RBD image to device");

        let image_spec = format!("{}/{}", self.pool_name, image_name);
        self.exec_rbd(["map", image_spec.as_str()])
            .await
            .map(|s| PathBuf::from(s.trim().to_string()))
    }

    /// rbd device unmap $device_path
    ///
    /// Retries with exponential backoff when the device is busy (EBUSY / exit
    /// code 16). This commonly happens when a VM process was just killed and
    /// the kernel hasn't fully released the block device yet.
    pub async fn device_unmap<P: AsRef<Path>>(&self, device_path: P) -> Result<(), RbdClientError> {
        debug!(device_path = ?device_path.as_ref(), "Unmapping RBD device");

        const MAX_ATTEMPTS: u8 = 8;
        const FORCE_AFTER_ATTEMPT: u8 = 3; // Use -o force after this many failed attempts
        const INITIAL_DELAY: Duration = Duration::from_millis(200);
        const MAX_DELAY: Duration = Duration::from_secs(5);

        let mut last_err = None;
        let mut delay = INITIAL_DELAY;

        for attempt in 0..MAX_ATTEMPTS {
            // After FORCE_AFTER_ATTEMPT failures, use -o force to forcefully unmap
            let use_force = attempt >= FORCE_AFTER_ATTEMPT;

            let result = if use_force {
                self.exec_rbd([
                    OsStr::new("device"),
                    OsStr::new("unmap"),
                    OsStr::new("-o"),
                    OsStr::new("force"),
                    device_path.as_ref().as_os_str(),
                ])
                .await
            } else {
                self.exec_rbd([
                    OsStr::new("device"),
                    OsStr::new("unmap"),
                    device_path.as_ref().as_os_str(),
                ])
                .await
            };

            match result {
                Ok(_) => return Ok(()),
                Err(e) => {
                    let err_msg = e.to_string();
                    if err_msg.contains("Device or resource busy") && attempt < MAX_ATTEMPTS - 1 {
                        warn!(
                            attempt,
                            max_attempts = MAX_ATTEMPTS,
                            use_force,
                            delay_ms = delay.as_millis(),
                            device_path = ?device_path.as_ref(),
                            "rbd device unmap failed due to busy device; retrying"
                        );
                        last_err = Some(e);
                        sleep(delay).await;
                        delay = (delay * 2).min(MAX_DELAY);
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        Err(last_err
            .unwrap_or_else(|| RbdClientError::Other("device unmap failed repeatedly".into())))
    }

    /// rbd resize $image_name --size $size. --allow-shrink is forbidden by default; this method only allows growing the image.
    pub async fn image_grow(&self, image_name: &str, size_mib: u32) -> Result<(), RbdClientError> {
        if self.image_info(image_name).await?.size_mib() == size_mib {
            debug!(
                image_name = %image_name,
                pool_name = %self.pool_name,
                size_mib,
                "Skipping RBD grow; new size is equal to original"
            );
            return Ok(());
        }

        debug!(
            image_name = %image_name,
            pool_name = %self.pool_name,
            size_mib,
            "Growing RBD image size"
        );

        let image_spec = format!("{}/{}", self.pool_name, image_name);

        self.exec_rbd([
            "resize",
            image_spec.as_str(),
            "--size",
            size_mib.to_string().as_str(),
        ])
        .await
        .map(|_| ())
    }

    /// rbd status $pool_name/$image_name - returns true if the image has any watchers (i.e. is mapped/in use)
    pub async fn image_has_watchers(&self, image_name: &str) -> Result<bool, RbdClientError> {
        let image_spec = format!("{}/{}", self.pool_name, image_name);
        let json = self
            .exec_rbd(["status", image_spec.as_str(), "--format", "json"])
            .await?;
        let status: RbdImageStatus = serde_json::from_str(&json)
            .map_err(|e| RbdClientError::Exec(format!("Failed to parse rbd status json: {e}")))?;
        Ok(!status.watchers.is_empty())
    }

    /// Returns true if the given snapshot has any clone-children in the pool.
    ///
    /// Runs `rbd children --format json $pool/$image@$snap`. Returns false if the
    /// snapshot is not protected (unprotected snaps cannot have children).
    pub async fn snap_has_children(&self, snap_name: &RbdSnapName) -> Result<bool, RbdClientError> {
        let snap_spec = self.format_snap_spec(snap_name);
        debug!(%snap_spec, "Checking if RBD snapshot has children");

        let output = self
            .exec_rbd(["children", "--format", "json", &snap_spec])
            .await;

        let json = output?;
        let children: Vec<serde_json::Value> = serde_json::from_str(&json)
            .map_err(|e| RbdClientError::Exec(format!("Failed to parse rbd children json: {e}")))?;
        Ok(!children.is_empty())
    }

    pub async fn ceph_status(&self) -> Result<String, RbdClientError> {
        self.exec_ceph(["status"]).await
    }

    /// Get the Ceph client version by running `ceph --version`.
    pub async fn ceph_client_version(&self) -> Result<String, RbdClientError> {
        self.exec_ceph(["--version"]).await
    }

    /// Get the Ceph cluster version by running `ceph version`.
    pub async fn ceph_cluster_version(&self) -> Result<String, RbdClientError> {
        self.exec_ceph(["version"]).await
    }
}
