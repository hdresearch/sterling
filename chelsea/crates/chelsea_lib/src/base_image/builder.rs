use std::path::{Path, PathBuf};
use std::sync::Arc;

use ceph::{RbdClientError, RbdSnapName, default_rbd_client};
use serde::{Deserialize, Serialize};
use tempfile::TempDir;
use tokio::process::Command;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
use util::defer::DeferAsync;
use utoipa::ToSchema;
use uuid::Uuid;
use vers_config::VersConfig;

use super::config::configure_filesystem;
use super::error::BaseImageError;

/// S3 key for the base VM squashfs image.
/// This image contains a properly configured Ubuntu with systemd, getty, and serial console.
const BASE_SQUASHFS_KEY: &str = "vmbase/ubuntu-24.04.squashfs.gz";

/// Returns the S3 bucket name for the base VM squashfs image.
/// Uses ROOTFS_S3_TIER env var (defaults to "development") to construct the bucket name.
// TODO: migrate ROOTFS_S3_TIER to VersConfig (pre-existing env var access)
fn base_squashfs_bucket() -> String {
    let tier = std::env::var("ROOTFS_S3_TIER").unwrap_or_else(|_| "development".to_string());
    format!("vers-{}-use1-az4-x-s3", tier)
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageSource {
    Docker { image_ref: String },
    S3 { bucket: String, key: String },
    Upload { tarball_path: String },
}

fn default_additional_capacity() -> u32 {
    256
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateBaseImageRequest {
    pub image_name: String,
    pub source: ImageSource,
    /// Additional capacity in MiB beyond the actual filesystem size (defaults to 256).
    /// The final Ceph image size = calculated rootfs size + this value.
    /// Set to 0 for minimum possible image size, or higher for more free space.
    #[serde(default = "default_additional_capacity")]
    pub size_mib: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImageCreationStatus {
    Pending,
    Downloading,
    Extracting,
    Configuring,
    CreatingRbd,
    CreatingSnapshot,
    Completed,
    Failed { error: String },
}

impl ImageCreationStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed { .. })
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

/// Returns the configured base image snapshot name from VersConfig.
fn base_image_snap_name() -> String {
    VersConfig::chelsea().ceph_base_image_snap_name.clone()
}

/// Validates that a path is safe to pass to shell commands.
///
/// This is a defense-in-depth measure. While we use `Command::new()` which doesn't
/// invoke a shell, some programs may interpret special characters in arguments.
/// Additionally, `to_string_lossy()` can produce unexpected results for non-UTF8 paths.
///
/// # Security assumptions
/// - TempDir generates paths under /tmp with safe random names
/// - Device paths from RBD are like /dev/rbd0
/// - Mount points are generated with UUIDs
/// - User-provided image names are validated separately by `validate_image_name()`
///
/// This function rejects paths containing:
/// - Shell metacharacters: $ ` \ " ' ; | & < > ( ) { } [ ] ! ? * ~
/// - Newlines or null bytes
/// - Non-UTF8 sequences (to_string_lossy would replace with �)
fn validate_path_for_command(path: &Path) -> Result<(), BaseImageError> {
    // Check for non-UTF8 paths (to_string_lossy would mangle these)
    let path_str = path.to_str().ok_or_else(|| {
        BaseImageError::Other(format!(
            "Path contains non-UTF8 characters: {}",
            path.display()
        ))
    })?;

    // Shell metacharacters that could cause issues even without a shell
    // (some programs interpret these specially)
    const DANGEROUS_CHARS: &[char] = &[
        '$', '`', '\\', '"', '\'', ';', '|', '&', '<', '>', '(', ')', '{', '}', '[', ']', '!', '?',
        '*', '~', '\n', '\r', '\0',
    ];

    for c in DANGEROUS_CHARS {
        if path_str.contains(*c) {
            return Err(BaseImageError::Other(format!(
                "Path contains unsafe character '{}': {}",
                c.escape_default(),
                path_str
            )));
        }
    }

    Ok(())
}

/// Calculates the size of a directory in MiB using `du`.
async fn calculate_dir_size_mib(path: &Path) -> Result<u32, BaseImageError> {
    validate_path_for_command(path)?;

    let output = Command::new("du")
        .args(["-sm", path.to_string_lossy().as_ref()])
        .output()
        .await
        .map_err(|e| BaseImageError::Other(format!("Failed to run du: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(BaseImageError::Other(format!("du failed: {}", stderr)));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Output format: "SIZE\tPATH"
    let size_str = stdout
        .split_whitespace()
        .next()
        .ok_or_else(|| BaseImageError::Other("Failed to parse du output".to_string()))?;

    size_str
        .parse::<u32>()
        .map_err(|e| BaseImageError::Other(format!("Failed to parse size from du output: {}", e)))
}

/// Validates image name to prevent command injection.
/// Allows alphanumeric, hyphens, underscores, periods, and forward slashes (for namespace paths).
/// Format can be either "image_name" or "namespace/image_name" (e.g., "owner_id/my-image").
fn validate_image_name(name: &str) -> Result<(), BaseImageError> {
    if name.is_empty() {
        return Err(BaseImageError::InvalidImageName(name.to_string()));
    }

    // Must start with alphanumeric
    if !name
        .chars()
        .next()
        .map(|c| c.is_ascii_alphanumeric())
        .unwrap_or(false)
    {
        return Err(BaseImageError::InvalidImageName(name.to_string()));
    }

    // Only allow safe characters (including / for namespace paths)
    let is_valid = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/');

    if !is_valid {
        return Err(BaseImageError::InvalidImageName(name.to_string()));
    }

    // Don't allow consecutive slashes or trailing slashes
    if name.contains("//") || name.ends_with('/') {
        return Err(BaseImageError::InvalidImageName(name.to_string()));
    }

    // Prevent path traversal attempts
    if name.contains("..") {
        return Err(BaseImageError::InvalidImageName(name.to_string()));
    }

    Ok(())
}

/// Attempts to locate the chelsea-agent binary.
///
/// Search order:
/// 1. `VersConfig::chelsea().agent_binary_path` (explicit config override)
/// 2. Next to the current executable (e.g. `./result/bin/chelsea-agent` in release deployments)
/// 3. `target/{release,debug}/chelsea-agent` relative to the executable's directory
///    (development — only works when the exe is in the workspace tree)
///
/// Returns `None` if the binary is not found (the base image will be created
/// without the agent — legacy notify-ready will still work).
fn find_agent_binary() -> Option<PathBuf> {
    // 1. Explicit config override
    if let Some(ref path) = VersConfig::chelsea().agent_binary_path {
        if path.exists() {
            info!(path = %path.display(), "Using chelsea-agent from config");
            return Some(path.clone());
        }
        warn!(path = %path.display(), "chelsea_agent_binary_path configured but file not found");
    }

    // 2. Next to the current executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("chelsea-agent");
            if candidate.exists() {
                info!(path = %candidate.display(), "Found chelsea-agent next to current executable");
                return Some(candidate);
            }

            // 3. Walk up from the executable to find target/{release,debug}/ in the workspace.
            // This handles the development case where the exe is at
            // target/{profile}/chelsea and the agent is at target/{profile}/chelsea-agent.
            for profile in &["release", "debug"] {
                let candidate = dir.join(format!("../../target/{}/chelsea-agent", profile));
                if candidate.exists() {
                    if let Ok(canonical) = candidate.canonicalize() {
                        info!(path = %canonical.display(), "Found chelsea-agent in workspace target directory");
                        return Some(canonical);
                    }
                }
            }
        }
    }

    None
}

pub struct BaseImageBuilder {
    status: Arc<Mutex<ImageCreationStatus>>,
}

impl BaseImageBuilder {
    pub fn new() -> Self {
        Self {
            status: Arc::new(Mutex::new(ImageCreationStatus::Pending)),
        }
    }

    pub async fn status(&self) -> ImageCreationStatus {
        self.status.lock().await.clone()
    }

    async fn set_status(&self, status: ImageCreationStatus) {
        *self.status.lock().await = status;
    }

    pub async fn create(&self, request: &CreateBaseImageRequest) -> Result<(), BaseImageError> {
        match self.create_inner(request).await {
            Ok(()) => Ok(()),
            Err(e) => {
                // Set status to Failed so polling clients see the failure
                self.set_status(ImageCreationStatus::Failed {
                    error: e.to_string(),
                })
                .await;
                Err(e)
            }
        }
    }

    async fn create_inner(&self, request: &CreateBaseImageRequest) -> Result<(), BaseImageError> {
        let image_name = &request.image_name;
        let additional_capacity_mib = request.size_mib;

        // Validate image name to prevent command injection
        validate_image_name(image_name)?;

        info!(%image_name, %additional_capacity_mib, "Starting base image creation");

        // Create a temporary directory for our work
        let temp_dir = TempDir::new().map_err(|e| {
            BaseImageError::Other(format!("Failed to create temp directory: {}", e))
        })?;
        let work_dir = temp_dir.path();

        // The final rootfs directory
        let rootfs_dir = work_dir.join("rootfs");
        tokio::fs::create_dir_all(&rootfs_dir).await.map_err(|e| {
            BaseImageError::CreateDirectory {
                path: rootfs_dir.clone(),
                source: e,
            }
        })?;

        match &request.source {
            ImageSource::Docker { image_ref } => {
                // For Docker images, we use the pre-configured base squashfs as the foundation
                // and overlay the docker image contents on top. This ensures we have a working
                // init system (systemd), serial console, and all boot requirements.

                // Step 1: Download and extract base squashfs
                self.download_and_extract_base_squashfs(&rootfs_dir).await?;

                // Step 2: Export docker image to a separate directory
                let docker_dir = work_dir.join("docker");
                tokio::fs::create_dir_all(&docker_dir).await.map_err(|e| {
                    BaseImageError::CreateDirectory {
                        path: docker_dir.clone(),
                        source: e,
                    }
                })?;
                let docker_tarball = work_dir.join("docker.tar");
                self.download_docker_image(image_ref, &docker_tarball)
                    .await?;
                self.extract_tarball(&docker_tarball, &docker_dir).await?;

                // Step 3: Merge docker contents onto base, preserving critical boot files
                self.merge_docker_onto_base(&docker_dir, &rootfs_dir)
                    .await?;
            }
            ImageSource::S3 { bucket, key } => {
                // S3 tarballs are expected to be complete rootfs images
                let tarball_path = work_dir.join("rootfs.tar");
                self.download_s3_tarball(bucket, key, &tarball_path).await?;
                self.extract_tarball(&tarball_path, &rootfs_dir).await?;
            }
            ImageSource::Upload {
                tarball_path: uploaded_path,
            } => {
                // Uploads are expected to be complete rootfs tarballs
                self.set_status(ImageCreationStatus::Downloading).await;
                info!(?uploaded_path, "Using uploaded tarball");
                let uploaded = PathBuf::from(uploaded_path);
                if !uploaded.exists() {
                    return Err(BaseImageError::Other(format!(
                        "Uploaded tarball not found: {}",
                        uploaded_path
                    )));
                }
                let tarball_path = work_dir.join("rootfs.tar");
                tokio::fs::copy(&uploaded, &tarball_path)
                    .await
                    .map_err(|e| {
                        BaseImageError::Other(format!("Failed to copy uploaded tarball: {}", e))
                    })?;
                self.extract_tarball(&tarball_path, &rootfs_dir).await?;
            }
        }

        // Configure filesystem (adds Chelsea network scripts, services, etc.)
        self.set_status(ImageCreationStatus::Configuring).await;
        let agent_binary = find_agent_binary();
        if agent_binary.is_none() {
            warn!("chelsea-agent binary not found; base image will not include the in-VM agent");
        }
        configure_filesystem(&rootfs_dir, agent_binary.as_deref())?;

        // Calculate the actual size of the prepared rootfs
        let rootfs_size_mib = calculate_dir_size_mib(&rootfs_dir).await?;

        // Total size = actual rootfs size + additional capacity requested by user
        // Minimum total size is 256 MiB to ensure enough space for filesystem overhead
        // Maximum total size is 64 GiB to prevent runaway storage consumption
        const MIN_TOTAL_SIZE_MIB: u32 = 256;
        const MAX_TOTAL_SIZE_MIB: u32 = 64 * 1024; // 64 GiB

        let total_size_mib = rootfs_size_mib + additional_capacity_mib;
        let total_size_mib = std::cmp::max(MIN_TOTAL_SIZE_MIB, total_size_mib);

        if total_size_mib > MAX_TOTAL_SIZE_MIB {
            return Err(BaseImageError::Other(format!(
                "Total image size ({} MiB) exceeds maximum allowed size ({} MiB). \
                 The rootfs is {} MiB with {} MiB additional capacity requested.",
                total_size_mib, MAX_TOTAL_SIZE_MIB, rootfs_size_mib, additional_capacity_mib
            )));
        }

        info!(
            %rootfs_size_mib,
            %additional_capacity_mib,
            %total_size_mib,
            "Calculated image size from rootfs"
        );

        // Create RBD image and copy files
        self.create_rbd_and_copy(image_name, total_size_mib, &rootfs_dir)
            .await?;

        // Create protected snapshot
        self.set_status(ImageCreationStatus::CreatingSnapshot).await;
        self.create_base_snapshot(image_name).await?;

        self.set_status(ImageCreationStatus::Completed).await;
        info!(%image_name, "Base image creation completed successfully");

        Ok(())
    }

    async fn download_docker_image(
        &self,
        image_ref: &str,
        tarball_path: &Path,
    ) -> Result<(), BaseImageError> {
        self.set_status(ImageCreationStatus::Downloading).await;

        // Check if the image exists locally first
        let inspect_output = Command::new("docker")
            .args(["image", "inspect", image_ref])
            .output()
            .await
            .map_err(|e| BaseImageError::DockerDump(format!("Failed to inspect image: {}", e)))?;

        if inspect_output.status.success() {
            info!(%image_ref, "Docker image found locally, skipping pull");
        } else {
            // Image doesn't exist locally, try to pull it
            info!(%image_ref, "Pulling Docker image");
            let output = Command::new("docker")
                .args(["pull", image_ref])
                .output()
                .await
                .map_err(|e| BaseImageError::DockerDump(format!("Failed to pull image: {}", e)))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(BaseImageError::DockerDump(format!(
                    "Docker pull failed: {}",
                    stderr
                )));
            }
        }

        // Create a container from the image
        debug!(%image_ref, "Creating container from image");
        let output = Command::new("docker")
            .args(["create", image_ref])
            .output()
            .await
            .map_err(|e| {
                BaseImageError::DockerDump(format!("Failed to create container: {}", e))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(BaseImageError::DockerDump(format!(
                "Docker create failed: {}",
                stderr
            )));
        }

        let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();

        // Set up async cleanup for the container
        let container_id_cleanup = container_id.clone();
        let mut container_defer = DeferAsync::new();
        container_defer.defer(async move {
            // Best effort cleanup - ignore errors
            let _ = Command::new("docker")
                .args(["rm", "-f", &container_id_cleanup])
                .output()
                .await;
        });

        // Export the container filesystem
        debug!(%container_id, ?tarball_path, "Exporting container filesystem");
        validate_path_for_command(tarball_path)?;
        let output = Command::new("docker")
            .args([
                "export",
                &container_id,
                "-o",
                tarball_path.to_string_lossy().as_ref(),
            ])
            .output()
            .await
            .map_err(|e| {
                BaseImageError::DockerDump(format!("Failed to export container: {}", e))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Cleanup runs automatically on drop, but let's be explicit
            container_defer.cleanup().await;
            return Err(BaseImageError::DockerDump(format!(
                "Docker export failed: {}",
                stderr
            )));
        }

        // Always cleanup the container after export (success or failure)
        container_defer.cleanup().await;

        info!(%image_ref, "Docker image exported successfully");
        Ok(())
    }

    async fn download_s3_tarball(
        &self,
        bucket: &str,
        key: &str,
        tarball_path: &Path,
    ) -> Result<(), BaseImageError> {
        self.set_status(ImageCreationStatus::Downloading).await;
        info!(%bucket, %key, "Downloading tarball from S3");

        let s3_client = util::s3::get_s3_client().await;
        util::s3::download_file_from_s3(s3_client, bucket, key, tarball_path)
            .await
            .map_err(|e| BaseImageError::S3Download(e.to_string()))?;

        info!(%bucket, %key, "S3 tarball downloaded successfully");
        Ok(())
    }

    /// Downloads the pre-configured base VM squashfs from S3 and extracts it.
    /// This base image contains a working Ubuntu with systemd, serial console, and all
    /// requirements for booting as a Firecracker VM.
    async fn download_and_extract_base_squashfs(
        &self,
        output_dir: &Path,
    ) -> Result<(), BaseImageError> {
        self.set_status(ImageCreationStatus::Downloading).await;
        info!("Downloading base VM squashfs from S3");

        // Download the compressed squashfs to a temp file
        let squashfs_gz_path = output_dir
            .parent()
            .ok_or_else(|| BaseImageError::Other("Invalid output directory".to_string()))?
            .join("base.squashfs.gz");

        let bucket = base_squashfs_bucket();
        let s3_client = util::s3::get_s3_client().await;
        util::s3::download_file_from_s3(s3_client, &bucket, BASE_SQUASHFS_KEY, &squashfs_gz_path)
            .await
            .map_err(|e| {
                BaseImageError::S3Download(format!("Failed to download base squashfs: {}", e))
            })?;

        info!("Decompressing base squashfs");
        let squashfs_path = output_dir
            .parent()
            .ok_or_else(|| BaseImageError::Other("Invalid output directory".to_string()))?
            .join("base.squashfs");

        // Decompress with gunzip
        let output = Command::new("gunzip")
            .args(["-c"])
            .arg(&squashfs_gz_path)
            .output()
            .await
            .map_err(|e| BaseImageError::ExtractTarball(format!("Failed to run gunzip: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(BaseImageError::ExtractTarball(format!(
                "gunzip failed: {}",
                stderr
            )));
        }

        // Write decompressed data to file
        tokio::fs::write(&squashfs_path, &output.stdout)
            .await
            .map_err(|e| {
                BaseImageError::Other(format!("Failed to write decompressed squashfs: {}", e))
            })?;

        // Extract squashfs using unsquashfs
        info!(?output_dir, "Extracting base squashfs");
        validate_path_for_command(output_dir)?;
        validate_path_for_command(&squashfs_path)?;
        let extract_output = Command::new("unsquashfs")
            .args([
                "-f", // force overwrite
                "-d", // destination directory
                output_dir.to_string_lossy().as_ref(),
                squashfs_path.to_string_lossy().as_ref(),
            ])
            .output()
            .await
            .map_err(|e| {
                BaseImageError::ExtractTarball(format!("Failed to run unsquashfs: {}", e))
            })?;

        if !extract_output.status.success() {
            let stderr = String::from_utf8_lossy(&extract_output.stderr);
            return Err(BaseImageError::ExtractTarball(format!(
                "unsquashfs failed: {}",
                stderr
            )));
        }

        // Clean up temporary files
        let _ = tokio::fs::remove_file(&squashfs_gz_path).await;
        let _ = tokio::fs::remove_file(&squashfs_path).await;

        info!("Base squashfs extracted successfully");
        Ok(())
    }

    /// Merges docker image contents onto the base VM image.
    /// Preserves critical boot files from the base (systemd, init, etc.) while
    /// overlaying application files from the docker image.
    async fn merge_docker_onto_base(
        &self,
        docker_dir: &Path,
        base_dir: &Path,
    ) -> Result<(), BaseImageError> {
        info!(?docker_dir, ?base_dir, "Merging docker image onto base");

        // Validate paths before passing to rsync
        validate_path_for_command(docker_dir)?;
        validate_path_for_command(base_dir)?;

        // Use rsync to copy docker contents onto base, excluding critical boot paths.
        // These exclusions ensure we keep the base's init system and boot configuration.
        let output = Command::new("rsync")
            .args([
                "-a",                // archive mode (preserves permissions, etc.)
                "--ignore-existing", // don't overwrite existing files in base
                // Exclude critical systemd/init paths that must come from the base
                "--exclude=/sbin/init",
                "--exclude=/lib/systemd/",
                "--exclude=/usr/lib/systemd/",
                "--exclude=/etc/systemd/system/*.wants/",
                "--exclude=/etc/fstab",
                "--exclude=/etc/inittab",
                // Exclude docker-specific files that don't make sense for a VM
                "--exclude=/.dockerenv",
                "--exclude=/etc/hostname", // We set this in configure_filesystem
                // The trailing slash is important - it copies contents, not the directory itself
                &format!("{}/", docker_dir.to_string_lossy()),
                &format!("{}/", base_dir.to_string_lossy()),
            ])
            .output()
            .await
            .map_err(|e| BaseImageError::CopyFiles(format!("Failed to run rsync: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(BaseImageError::CopyFiles(format!(
                "rsync merge failed: {}",
                stderr
            )));
        }

        info!("Docker image merged onto base successfully");
        Ok(())
    }

    async fn extract_tarball(
        &self,
        tarball_path: &Path,
        output_dir: &Path,
    ) -> Result<(), BaseImageError> {
        self.set_status(ImageCreationStatus::Extracting).await;
        info!(?tarball_path, ?output_dir, "Extracting tarball");

        // Validate paths before passing to tar command
        validate_path_for_command(tarball_path)?;
        validate_path_for_command(output_dir)?;

        let output = Command::new("tar")
            .args([
                "xf",
                tarball_path.to_string_lossy().as_ref(),
                "-C",
                output_dir.to_string_lossy().as_ref(),
            ])
            .output()
            .await
            .map_err(|e| BaseImageError::ExtractTarball(format!("Failed to run tar: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(BaseImageError::ExtractTarball(format!(
                "tar extraction failed: {}",
                stderr
            )));
        }

        info!(?output_dir, "Tarball extracted successfully");
        Ok(())
    }

    async fn create_rbd_and_copy(
        &self,
        image_name: &str,
        size_mib: u32,
        rootfs_dir: &Path,
    ) -> Result<(), BaseImageError> {
        self.set_status(ImageCreationStatus::CreatingRbd).await;
        let client = default_rbd_client()?;

        info!(%image_name, size_mib, "Creating RBD image");

        // If the image name contains a namespace (owner_id/image_name format),
        // ensure the namespace exists before creating the image
        if let Some(namespace) = image_name.split('/').next() {
            if image_name.contains('/') {
                info!(%namespace, "Ensuring RBD namespace exists");
                client.namespace_ensure(namespace).await?;
            }
        }

        // Create the RBD image - handle race condition where another request
        // may have created the image between our check and now
        if let Err(e) = client.image_create(image_name, size_mib).await {
            // Check if the error indicates the image already exists
            let err_str = e.to_string();
            if err_str.contains("already exists") || err_str.contains("File exists") {
                return Err(BaseImageError::ImageAlreadyExists(image_name.to_string()));
            }
            return Err(e.into());
        }

        // Set up cleanup in case of failure
        let mut defer = DeferAsync::new();
        let image_name_clone = image_name.to_string();
        defer.defer(async move {
            if let Ok(client) = default_rbd_client() {
                if let Err(e) = client.image_remove(&image_name_clone).await {
                    warn!(%image_name_clone, %e, "Failed to cleanup RBD image after error");
                }
            }
        });

        // Map the image to a block device
        let device_path = client.device_map(image_name).await?;
        debug!(?device_path, "Mapped RBD image to device");

        // Set up device cleanup
        let device_path_clone = device_path.clone();
        let mut device_defer = DeferAsync::new();
        device_defer.defer(async move {
            if let Ok(client) = default_rbd_client() {
                if let Err(e) = client.device_unmap(&device_path_clone).await {
                    warn!(?device_path_clone, %e, "Failed to unmap device after error");
                }
            }
        });

        // Format the device with ext4
        info!(?device_path, "Formatting device with ext4");
        let output = Command::new("mkfs.ext4")
            .arg(&device_path)
            .output()
            .await
            .map_err(|e| {
                BaseImageError::FormatFilesystem(format!("Failed to run mkfs.ext4: {}", e))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(BaseImageError::FormatFilesystem(format!(
                "mkfs.ext4 failed: {}",
                stderr
            )));
        }

        // Mount the device to a unique mount point to avoid conflicts with concurrent builds
        let mount_id = Uuid::new_v4();
        let mount_point = PathBuf::from(format!("/mnt/chelsea_base_image_{}", mount_id));
        tokio::fs::create_dir_all(&mount_point).await.map_err(|e| {
            BaseImageError::CreateDirectory {
                path: mount_point.clone(),
                source: e,
            }
        })?;

        // Validate paths before passing to mount command
        validate_path_for_command(&device_path)?;
        validate_path_for_command(&mount_point)?;

        info!(?device_path, ?mount_point, "Mounting device");
        let output = Command::new("mount")
            .args([
                device_path.to_string_lossy().as_ref(),
                mount_point.to_string_lossy().as_ref(),
            ])
            .output()
            .await
            .map_err(|e| BaseImageError::MountDevice(format!("Failed to run mount: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(BaseImageError::MountDevice(format!(
                "mount failed: {}",
                stderr
            )));
        }

        // Set up unmount cleanup (also removes mount directory)
        let mount_point_clone = mount_point.clone();
        let mut mount_defer = DeferAsync::new();
        mount_defer.defer(async move {
            let output = Command::new("umount")
                .arg(&mount_point_clone)
                .output()
                .await;
            if let Err(e) = output {
                warn!(?mount_point_clone, %e, "Failed to unmount after error");
            }
            // Best effort cleanup of mount directory
            let _ = tokio::fs::remove_dir(&mount_point_clone).await;
        });

        // Copy files from rootfs to the mounted device
        // Note: mount_point was already validated above for the mount command
        validate_path_for_command(rootfs_dir)?;

        info!(?rootfs_dir, ?mount_point, "Copying files to RBD image");
        let output = Command::new("cp")
            .args([
                "-a",
                &format!("{}/.", rootfs_dir.to_string_lossy()),
                mount_point.to_string_lossy().as_ref(),
            ])
            .output()
            .await
            .map_err(|e| BaseImageError::CopyFiles(format!("Failed to run cp: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(BaseImageError::CopyFiles(format!("cp failed: {}", stderr)));
        }

        // Unmount the device
        info!(?mount_point, "Unmounting device");
        let output = Command::new("umount")
            .arg(&mount_point)
            .output()
            .await
            .map_err(|e| BaseImageError::UnmountDevice(format!("Failed to run umount: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(BaseImageError::UnmountDevice(format!(
                "umount failed: {}",
                stderr
            )));
        }
        mount_defer.commit(); // Successfully unmounted, don't try to unmount again

        // Clean up the mount directory
        if let Err(e) = tokio::fs::remove_dir(&mount_point).await {
            warn!(?mount_point, %e, "Failed to remove mount directory");
        }

        // Unmap the device
        debug!(?device_path, "Unmapping device");
        client.device_unmap(&device_path).await?;
        device_defer.commit(); // Successfully unmapped, don't try again

        // Success - don't delete the image
        defer.commit();

        info!(%image_name, "RBD image created and populated successfully");
        Ok(())
    }

    async fn create_base_snapshot(&self, image_name: &str) -> Result<(), BaseImageError> {
        let client = default_rbd_client()?;

        let snap_name = RbdSnapName {
            image_name: image_name.to_string(),
            snap_name: base_image_snap_name(),
        };

        info!(%snap_name, "Creating base image snapshot");
        client.snap_create(&snap_name).await?;

        info!(%snap_name, "Protecting base image snapshot");
        client.snap_protect(&snap_name).await?;

        Ok(())
    }
}

impl Default for BaseImageBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn list_base_images() -> Result<Vec<String>, BaseImageError> {
    let client = default_rbd_client()?;
    let all_images = client.image_list().await?;

    let mut base_images = Vec::new();
    for image_name in all_images {
        match client.snap_list(&image_name).await {
            Ok(snaps) => {
                if snaps
                    .iter()
                    .any(|snap| snap.snap_name == base_image_snap_name())
                {
                    base_images.push(image_name);
                }
            }
            Err(RbdClientError::NotFound(_)) => {
                // Image disappeared between list and snap_list, ignore
                continue;
            }
            Err(err) => return Err(err.into()),
        }
    }

    Ok(base_images)
}

pub async fn base_image_exists(image_name: &str) -> Result<bool, BaseImageError> {
    let client = default_rbd_client()?;

    if !client.image_exists(image_name).await? {
        return Ok(false);
    }

    let snaps = match client.snap_list(image_name).await {
        Ok(snaps) => snaps,
        Err(RbdClientError::NotFound(_)) => return Ok(false),
        Err(err) => return Err(err.into()),
    };
    Ok(snaps
        .iter()
        .any(|snap| snap.snap_name == base_image_snap_name()))
}

/// Delete a base image from Ceph RBD.
///
/// This will:
/// 1. Check if the image exists
/// 2. Unprotect the base image snapshot (fails if clones exist)
/// 3. Remove the snapshot
/// 4. Remove the RBD image
///
/// Returns an error if the image has child clones (VMs using it).
pub async fn delete_base_image(image_name: &str) -> Result<(), BaseImageError> {
    let client = default_rbd_client()?;

    // Check if image exists
    if !client.image_exists(image_name).await? {
        return Err(BaseImageError::ImageNotFound(image_name.to_string()));
    }

    let snap_name = base_image_snap_name();
    let rbd_snap = RbdSnapName {
        image_name: image_name.to_string(),
        snap_name,
    };

    // Unprotect the snapshot - this will fail if there are child clones
    if let Err(e) = client.snap_unprotect(&rbd_snap).await {
        // Check if this is because of child clones
        let error_msg = e.to_string().to_lowercase();
        if error_msg.contains("clone") || error_msg.contains("children") {
            return Err(BaseImageError::ImageHasChildClones(image_name.to_string()));
        }
        return Err(e.into());
    }

    // Remove the snapshot
    client.snap_remove(&rbd_snap).await?;

    // Remove the RBD image
    client.image_remove(image_name).await?;

    info!(%image_name, "Base image deleted successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_base_image_request_default_size() {
        // Test that serde default works
        let json = r#"{"image_name": "test-image", "source": {"type": "docker", "image_ref": "alpine:latest"}}"#;
        let request: CreateBaseImageRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.size_mib, 256);
    }

    #[test]
    fn test_create_base_image_request_explicit_size() {
        let request = CreateBaseImageRequest {
            image_name: "test-image".to_string(),
            source: ImageSource::Docker {
                image_ref: "alpine:latest".to_string(),
            },
            size_mib: 512,
        };
        assert_eq!(request.size_mib, 512);
    }

    #[test]
    fn test_image_creation_status_is_terminal() {
        assert!(!ImageCreationStatus::Pending.is_terminal());
        assert!(!ImageCreationStatus::Downloading.is_terminal());
        assert!(ImageCreationStatus::Completed.is_terminal());
        assert!(
            ImageCreationStatus::Failed {
                error: "test".to_string()
            }
            .is_terminal()
        );
    }

    #[test]
    fn test_image_creation_status_is_failed() {
        assert!(!ImageCreationStatus::Completed.is_failed());
        assert!(
            ImageCreationStatus::Failed {
                error: "test".to_string()
            }
            .is_failed()
        );
    }

    #[test]
    fn test_validate_image_name_valid() {
        assert!(validate_image_name("ubuntu-24.04").is_ok());
        assert!(validate_image_name("my_image").is_ok());
        assert!(validate_image_name("test123").is_ok());
    }

    #[test]
    fn test_validate_image_name_invalid() {
        // Empty or bad start
        assert!(validate_image_name("").is_err());
        assert!(validate_image_name("-invalid").is_err());

        // Command injection attempts
        assert!(validate_image_name("image; rm -rf /").is_err());
        assert!(validate_image_name("image$(whoami)").is_err());

        // Path traversal
        assert!(validate_image_name("../../../etc/passwd").is_err());
        assert!(validate_image_name("image/../other").is_err());
    }
}
