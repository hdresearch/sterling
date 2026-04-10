use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::SystemTime,
};

use anyhow::anyhow;
use async_trait::async_trait;
use aws_sdk_s3::Client;
use tokio::fs::{read_dir, remove_file, symlink_metadata};
use tokio::sync::{Mutex, Notify};
use tracing::{debug, info, warn};
use util::{
    bytes_to_mib_ceil, join_errors,
    s3::{
        delete_objects, download_file_from_s3, get_total_s3_file_size_mib_many,
        upload_files_with_prefix,
    },
};
use uuid::Uuid;
use vers_config::VersConfig;
use vers_pg::schema::chelsea::tables::{commit::CommitFile, sleep_snapshot::SleepSnapshotFile};

use crate::{
    commit_store::VmCommitStore,
    s3_store::error::{DeleteOldestFileError, EnsureSpaceError, GetCacheUtilizationError},
    sleep_snapshot_store::VmSleepSnapshotStore,
};

/// Prefix used for temp files during downloads. Files with this prefix are
/// excluded from cache eviction and cleaned up on startup.
const TEMP_FILE_PREFIX: &str = ".tmp.";

/// Tracks the state of an in-progress download for a single file.
/// Multiple callers can wait on the same download via the `Notify`.
struct InflightDownload {
    /// Notified when the download completes (success or failure).
    notify: Arc<Notify>,
}

/// A concrete implementer of both VmCommitStore and VmSleepSnapshotStore that uses the snapshots data dir for local storage, and S3 for remote.
///
/// Concurrency safety:
/// - `inflight` coalesces concurrent downloads of the same file. The first caller
///   downloads from S3 while subsequent callers wait on a `Notify`. On completion
///   all waiters are released and find the file on disk.
/// - `pinned_files` tracks files actively in use by from_commit / sleep-wake
///   operations. The cache evictor (`ensure_space_impl`) skips pinned files so
///   that concurrent restores cannot have their snapshot files deleted mid-use.
/// - `cache_mu` serialises cache bookkeeping (ensure_space + eviction) so that
///   concurrent callers don't double-count available space or race on deletion.
pub struct S3SnapshotStore {
    /// The local snapshot directory
    snapshot_dir: PathBuf,
    /// The disk space, in MiB, that the store will reserve for itself. When a download would exceed this allocated amount, it will delete the oldest files according to `atime`.
    cache_size_mib: u32,
    /// The S3 client used for all remote operations.
    s3_client: Client,

    /// Per-file inflight download tracking. Key is the local file name (not full path).
    /// Protected by a Mutex so that the check-and-insert is atomic.
    inflight: Mutex<HashMap<String, Arc<InflightDownload>>>,

    /// Files currently pinned by active operations. Pinned files are excluded from
    /// cache eviction. Key is local file name, value is a reference count (number
    /// of concurrent users of that file).
    ///
    /// Uses a standard `std::sync::Mutex` (not tokio) so it can be locked
    /// synchronously in `UnpinGuard::drop` without requiring async-in-Drop hacks.
    /// The lock is only ever held briefly for refcount updates, so blocking is
    /// negligible.
    pinned_files: std::sync::Mutex<HashMap<String, u32>>,

    /// Serialises cache space bookkeeping (ensure_space + eviction loop) so that
    /// two concurrent callers cannot both decide there is enough room and then
    /// overshoot the cache budget.
    cache_mu: Mutex<()>,

    /// Number of S3 downloads actually performed (i.e. `do_download` completions).
    /// Useful for testing that coalescing avoids redundant downloads.
    download_count: AtomicU32,
}

impl S3SnapshotStore {
    pub async fn new(
        snapshot_dir: PathBuf,
        cache_size_mib: u32,
        s3_client: Client,
    ) -> Result<Self, std::io::Error> {
        // Ensure the snapshot_dir exists before proceeding.
        tokio::fs::create_dir_all(&snapshot_dir).await?;

        // Clean up any orphaned temp files from a previous crash.
        let mut entries = tokio::fs::read_dir(&snapshot_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let file_name = entry.file_name();
            if let Some(name) = file_name.to_str() {
                if name.starts_with(TEMP_FILE_PREFIX) {
                    match tokio::fs::remove_file(entry.path()).await {
                        Err(e) => {
                            warn!(path = %entry.path().display(), error = %e, "Failed to remove orphaned temp file");
                        }
                        Ok(_) => {
                            info!(path = %entry.path().display(), "Removed orphaned temp file from previous run");
                        }
                    }
                }
            }
        }

        Ok(Self {
            snapshot_dir,
            cache_size_mib,
            s3_client,
            inflight: Mutex::new(HashMap::new()),
            pinned_files: std::sync::Mutex::new(HashMap::new()),
            cache_mu: Mutex::new(()),
            download_count: AtomicU32::new(0),
        })
    }

    /// Ensure the snapshot_dir exists, creating it if it doesn't
    async fn ensure_snapshot_dir(&self) -> std::io::Result<()> {
        tokio::fs::create_dir_all(&self.snapshot_dir).await
    }

    /// Returns the number of S3 downloads actually performed.
    pub fn download_count(&self) -> u32 {
        self.download_count.load(Ordering::Relaxed)
    }

    /// Pin a set of file names so that cache eviction cannot remove them.
    /// Returns the set of names that were pinned (caller must unpin these later).
    fn pin_files(&self, file_names: &[String]) -> HashSet<String> {
        let mut pinned = self
            .pinned_files
            .lock()
            .expect("pinned_files mutex poisoned");
        let mut pinned_set = HashSet::new();
        for name in file_names {
            *pinned.entry(name.clone()).or_insert(0) += 1;
            pinned_set.insert(name.clone());
        }
        pinned_set
    }

    /// Ensure that there is at least `space_required_mib` MiB of space in the cache, deleting items by atime (ascending order) until enough space is available.
    /// Skips files that are currently pinned.
    ///
    /// MUST be called while holding `cache_mu`.
    async fn ensure_space_impl(&self, space_required_mib: u32) -> Result<(), EnsureSpaceError> {
        // First, check to make sure the request does not require more space than can possibly be freed.
        let cache_size_mib = self.cache_size_mib;
        debug!(
            cache_size_mib,
            space_required_mib, "Ensuring enough space for commit files"
        );
        if space_required_mib > self.cache_size_mib {
            return Err(EnsureSpaceError::CacheNotLargeEnough {
                space_required_mib,
                cache_size_mib,
            });
        }

        // Delete the oldest file in cache until there is enough space.
        let mut cache_available_mib = self.get_cache_available_mib().await?;

        while cache_available_mib < space_required_mib {
            let deleted_file = self.delete_least_recently_accessed_file().await?;

            cache_available_mib = self.get_cache_available_mib().await?;
            debug!(cache_available_mib, space_required_mib, file_path = %deleted_file.display(), "Deleted file");
        }

        let cache_used_mib = self.get_cache_utilization_mib().await?;
        debug!(
            space_required_mib,
            cache_size_mib,
            cache_used_mib,
            cache_available_mib,
            "Successfully ensured space in cache"
        );
        Ok(())
    }

    /// Returns the size, in MiB, of the commits directory by summing up the size of all files therewithin
    async fn get_cache_utilization_mib(&self) -> Result<u32, GetCacheUtilizationError> {
        let mut total_size = 0;

        // Ensure the snapshot dir exists before proceeding
        self.ensure_snapshot_dir().await?;

        let mut dir = read_dir(&self.snapshot_dir).await?;
        while let Some(entry) = dir.next_entry().await? {
            // A concurrent download may rename/delete a temp file between readdir
            // and stat. Silently skip entries that vanish.
            let metadata = match entry.metadata().await {
                Ok(m) => m,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            if metadata.is_file() {
                let file_size_bytes = metadata.len();
                total_size += bytes_to_mib_ceil(file_size_bytes) as u32;
            } else {
                warn!(path = %entry.path().display(), "Unexpected, non-file item in S3CommitStore cache dir");
            }
        }

        Ok(total_size)
    }

    /// Returns the space, in MiB, available in the cache
    async fn get_cache_available_mib(&self) -> Result<u32, GetCacheUtilizationError> {
        Ok(self
            .cache_size_mib
            .saturating_sub(self.get_cache_utilization_mib().await?))
    }

    /// Delete the oldest file, according to atime, returning the Path of the deleted file.
    /// Skips files that are currently pinned and in-progress temp files.
    async fn delete_least_recently_accessed_file(&self) -> Result<PathBuf, DeleteOldestFileError> {
        // Snapshot the pinned set so we don't hold the lock across the directory scan.
        let pinned_snapshot: HashSet<String> = {
            let pinned = self
                .pinned_files
                .lock()
                .expect("pinned_files mutex poisoned");
            pinned.keys().cloned().collect()
        };

        let mut oldest_path: Option<PathBuf> = None;
        let mut oldest_atime: Option<SystemTime> = None;

        // Ensure the snapshot dir exists before proceeding
        self.ensure_snapshot_dir().await?;

        let mut dir = read_dir(&self.snapshot_dir).await?;
        while let Some(entry) = dir.next_entry().await? {
            let path = entry.path();
            // A concurrent download may rename/delete a temp file between readdir
            // and stat. Silently skip entries that vanish.
            let meta = match symlink_metadata(&path).await {
                Ok(m) => m,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            if meta.is_file() {
                if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                    // Never evict in-progress temp files.
                    if file_name.starts_with(TEMP_FILE_PREFIX) {
                        continue;
                    }

                    // Skip pinned files — they are actively in use by a from_commit or sleep-wake operation.
                    if pinned_snapshot.contains(file_name) {
                        debug!(file_name, "Skipping pinned file during cache eviction");
                        continue;
                    }
                }

                {
                    use std::os::unix::fs::MetadataExt;
                    let atime = SystemTime::UNIX_EPOCH
                        + std::time::Duration::from_secs(meta.atime() as u64);
                    if oldest_atime.map_or(true, |old| atime < old) {
                        oldest_atime = Some(atime);
                        oldest_path = Some(path);
                    }
                }
            }
        }

        match oldest_path {
            Some(oldest) => {
                remove_file(&oldest).await?;
                Ok(oldest)
            }
            None => Err(DeleteOldestFileError::NoFiles),
        }
    }

    /// Download files from S3, coalescing concurrent requests for the same file.
    ///
    /// For each file:
    /// 1. If it already exists on disk, skip it.
    /// 2. If another task is already downloading it, wait for that download to finish.
    /// 3. Otherwise, become the downloader: reserve cache space, download to a temp
    ///    file, atomically rename into place, then notify all waiters.
    ///
    /// All files referenced by the caller are pinned for the duration of the call
    /// so that cache eviction cannot remove them while the caller (vm_manager)
    /// is hard-linking them into a VM jail.
    pub async fn download_files_coalesced(
        &self,
        bucket_name: &str,
        files: &[FileToDownload<'_>],
    ) -> anyhow::Result<()> {
        // Collect all local file names so we can pin them up front.
        let all_file_names: Vec<String> = files.iter().map(|f| f.file_name.to_string()).collect();
        let pinned_set = self.pin_files(&all_file_names);

        // Use a guard to ensure we unpin even on early return / error.
        let _unpin_guard = UnpinGuard {
            pinned_files: &self.pinned_files,
            files: pinned_set,
        };

        // Ensure the snapshot dir exists before proceeding
        self.ensure_snapshot_dir().await?;

        for file_info in files {
            let file_name = file_info.file_name;
            let file_path = self.snapshot_dir.join(file_name);
            let s3_key = file_info.s3_key;

            // Fast path: file already exists on disk (previously cached or downloaded
            // by a concurrent caller that finished before us).
            if file_path.exists() {
                debug!(file_name, "File already cached, skipping download");
                continue;
            }

            // Check if another task is already downloading this file.
            let maybe_existing = {
                let mut inflight = self.inflight.lock().await;
                if let Some(entry) = inflight.get(file_name) {
                    // Someone else is downloading — grab a handle to wait on.
                    Some(Arc::clone(entry))
                } else {
                    // We are the first — register ourselves as the downloader.
                    let entry = Arc::new(InflightDownload {
                        notify: Arc::new(Notify::new()),
                    });
                    inflight.insert(file_name.to_string(), Arc::clone(&entry));
                    None
                }
            };

            if let Some(existing) = maybe_existing {
                // Wait for the other downloader to finish.
                info!(file_name, "Waiting for inflight download by another task");
                existing.notify.notified().await;

                // Check result. If the other downloader failed, the file won't be on
                // disk. We don't retry here — let the error propagate so the caller
                // can retry the whole operation.
                if !file_path.exists() {
                    return Err(anyhow!(
                        "Concurrent download of {file_name} failed: file not present after inflight download completed"
                    ));
                }
                debug!(file_name, "Inflight download completed by another task");
                continue;
            }

            // We are the downloader. Ensure cache space under the cache lock,
            // then download to a temp file and atomically rename.
            let download_result = self
                .do_download(bucket_name, s3_key, file_name, &file_path)
                .await;

            // Remove the inflight entry and notify all waiters.
            // Waiters check whether the file appeared on disk to determine success/failure.
            {
                let mut inflight = self.inflight.lock().await;
                if let Some(entry) = inflight.remove(file_name) {
                    entry.notify.notify_waiters();
                }
            }

            if let Err(e) = download_result {
                warn!(file_name, error = %e, "Failed to download file from S3");
                return Err(e);
            }
        }

        Ok(())
    }

    /// Perform the actual S3 download for a single file:
    /// 1. Acquire cache lock, compute space needed, evict if necessary.
    /// 2. Download to a temp file in the snapshot dir.
    /// 3. Atomically rename temp file to the final path.
    async fn do_download(
        &self,
        bucket_name: &str,
        s3_key: &str,
        file_name: &str,
        file_path: &Path,
    ) -> anyhow::Result<()> {
        // Determine how much space this file needs.
        let space_required_mib =
            get_total_s3_file_size_mib_many(&self.s3_client, bucket_name, std::iter::once(s3_key))
                .await
                .map_err(|errors| anyhow!(join_errors(&errors, "; ")))? as u32;

        // Acquire cache lock for space bookkeeping + eviction.
        {
            let _cache_lock = self.cache_mu.lock().await;
            self.ensure_space_impl(space_required_mib).await?;
        }

        // Ensure the snapshot dir exists before proceeding
        self.ensure_snapshot_dir().await?;

        // Download to a temporary file in the same directory (same filesystem)
        // so that the rename is atomic.
        let tmp_path = self
            .snapshot_dir
            .join(format!("{TEMP_FILE_PREFIX}{file_name}.{}", Uuid::new_v4()));

        let download_result =
            download_file_from_s3(&self.s3_client, bucket_name, s3_key, &tmp_path).await;

        match download_result {
            Ok(()) => {
                // Atomic rename into the final location.
                tokio::fs::rename(&tmp_path, file_path).await.map_err(|e| {
                    anyhow!("Failed to rename temp file to {}: {e}", file_path.display())
                })?;
                self.download_count.fetch_add(1, Ordering::Relaxed);
                info!(file_name, "Successfully downloaded and cached file");
                Ok(())
            }
            Err(e) => {
                // Clean up the temp file on failure.
                let _ = tokio::fs::remove_file(&tmp_path).await;
                Err(e.into())
            }
        }
    }
}

/// Helper to extract the local file name and S3 key from commit/snapshot files.
pub struct FileToDownload<'a> {
    pub file_name: &'a str,
    pub s3_key: &'a str,
}

/// RAII guard that unpins files when dropped, even if the caller returns early via `?`.
///
/// Uses `std::sync::Mutex` so that Drop can lock synchronously without needing
/// async-in-Drop hacks like `block_in_place`. This is safe because the lock is
/// only ever held briefly for refcount decrements.
struct UnpinGuard<'a> {
    pinned_files: &'a std::sync::Mutex<HashMap<String, u32>>,
    files: HashSet<String>,
}

impl<'a> Drop for UnpinGuard<'a> {
    fn drop(&mut self) {
        let files = std::mem::take(&mut self.files);
        if files.is_empty() {
            return;
        }
        let mut pinned = self
            .pinned_files
            .lock()
            .expect("pinned_files mutex poisoned");
        for name in &files {
            if let Some(count) = pinned.get_mut(name) {
                *count -= 1;
                if *count == 0 {
                    pinned.remove(name);
                }
            }
        }
    }
}

#[async_trait]
impl VmCommitStore for S3SnapshotStore {
    /// Uploads a list of files to S3. The `to_upload` array is assumed to be relative to self.snapshot_dir.
    async fn upload_commit_files(
        &self,
        commit_id: &Uuid,
        to_upload: &[String],
    ) -> anyhow::Result<Vec<CommitFile>> {
        let commit_id = &commit_id.to_string();
        let bucket_name = VersConfig::chelsea().aws_commit_bucket_name.clone();
        let to_upload = to_upload
            .into_iter()
            .map(|file_name| self.commit_dir().join(file_name));

        Ok(upload_files_with_prefix(&bucket_name, to_upload, commit_id)
            .await?
            .into_iter()
            .map(|key| CommitFile { key })
            .collect::<Vec<_>>())
    }

    async fn download_commit_files(&self, remote_files: &[CommitFile]) -> anyhow::Result<()> {
        let bucket_name = &VersConfig::chelsea().aws_commit_bucket_name;

        // Build the list of files to download, extracting local file names.
        let mut files_to_download = Vec::new();
        for file in remote_files {
            let file_name = file.file_name().map_err(|e| anyhow!(e))?;
            files_to_download.push(FileToDownload {
                file_name,
                s3_key: &file.key,
            });
        }

        self.download_files_coalesced(bucket_name, &files_to_download)
            .await
    }

    fn commit_dir<'a>(&'a self) -> &'a Path {
        self.snapshot_dir.as_path()
    }

    async fn ensure_space(&self, space_required_mib: u32) -> anyhow::Result<()> {
        let _cache_lock = self.cache_mu.lock().await;
        self.ensure_space_impl(space_required_mib)
            .await
            .map_err(anyhow::Error::from)
    }
}

#[async_trait]
impl VmSleepSnapshotStore for S3SnapshotStore {
    /// Upload the provided files to the store, returning a Vec representing the files on the store for later retrieval.
    async fn upload_sleep_snapshot_files(
        &self,
        sleep_snapshot_id: &Uuid,
        to_upload: &[String],
    ) -> anyhow::Result<Vec<SleepSnapshotFile>> {
        let sleep_snapshot_id = &sleep_snapshot_id.to_string();
        let bucket_name = VersConfig::chelsea().aws_sleep_snapshot_bucket_name.clone();
        let to_upload = to_upload
            .into_iter()
            .map(|file_name| self.commit_dir().join(file_name));

        Ok(
            upload_files_with_prefix(&bucket_name, to_upload, sleep_snapshot_id)
                .await?
                .into_iter()
                .map(|key| SleepSnapshotFile { key })
                .collect::<Vec<_>>(),
        )
    }

    /// Retrieve the given files from the store, placing them into the sleep_snapshots subdir of the data directory.
    async fn download_sleep_snapshot_files(
        &self,
        remote_files: &[SleepSnapshotFile],
    ) -> anyhow::Result<()> {
        let bucket_name = &VersConfig::chelsea().aws_sleep_snapshot_bucket_name;

        // Build the list of files to download, extracting local file names.
        let mut files_to_download = Vec::new();
        for file in remote_files {
            let file_name = file.file_name().map_err(|e| anyhow!(e))?;
            files_to_download.push(FileToDownload {
                file_name,
                s3_key: &file.key,
            });
        }

        self.download_files_coalesced(bucket_name, &files_to_download)
            .await
    }

    /// The local path which is used for sleep_snapshot file uploads+downloads
    fn sleep_snapshot_dir<'a>(&'a self) -> &'a Path {
        self.snapshot_dir.as_path()
    }

    /// Ensure there is at least {required_size_mib} of space available in the sleep_snapshot cache
    async fn ensure_space(&self, space_required_mib: u32) -> anyhow::Result<()> {
        let _cache_lock = self.cache_mu.lock().await;
        self.ensure_space_impl(space_required_mib)
            .await
            .map_err(anyhow::Error::from)
    }

    async fn delete_sleep_snapshot_files(
        &self,
        to_delete: &[SleepSnapshotFile],
    ) -> anyhow::Result<()> {
        let bucket_name = &VersConfig::chelsea().aws_sleep_snapshot_bucket_name;
        let keys = to_delete.into_iter().map(|file| &file.key);
        delete_objects(&self.s3_client, bucket_name, keys).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    /// Create a store backed by a temp directory.
    /// Uses a dummy S3 client — unit tests here don't make S3 calls.
    async fn make_store(cache_size_mib: u32) -> (S3SnapshotStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let config = aws_sdk_s3::Config::builder()
            .behavior_version_latest()
            .region(aws_sdk_s3::config::Region::new("us-east-1"))
            .build();
        let client = Client::from_conf(config);
        let store = S3SnapshotStore::new(dir.path().to_path_buf(), cache_size_mib, client)
            .await
            .unwrap();
        (store, dir)
    }

    /// Write a file of approximately `size_bytes` into the store's snapshot dir.
    fn write_file(dir: &Path, name: &str, size_bytes: usize) -> PathBuf {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&vec![0u8; size_bytes]).unwrap();
        path
    }

    // -- Cache utilization tests --

    #[tokio::test]
    async fn test_empty_cache_utilization_is_zero() {
        let (store, _dir) = make_store(100).await;
        assert_eq!(store.get_cache_utilization_mib().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_cache_utilization_counts_files() {
        let (store, dir) = make_store(100).await;
        // Write a 1 MiB file
        write_file(dir.path(), "file1", 1024 * 1024);
        let used = store.get_cache_utilization_mib().await.unwrap();
        assert_eq!(used, 1);
    }

    #[tokio::test]
    async fn test_cache_utilization_rounds_up() {
        let (store, dir) = make_store(100).await;
        // Write 1 byte — should round up to 1 MiB
        write_file(dir.path(), "tiny", 1);
        let used = store.get_cache_utilization_mib().await.unwrap();
        assert_eq!(used, 1);
    }

    #[tokio::test]
    async fn test_cache_available_mib() {
        let (store, dir) = make_store(10).await;
        write_file(dir.path(), "file1", 3 * 1024 * 1024);
        let available = store.get_cache_available_mib().await.unwrap();
        assert_eq!(available, 7);
    }

    #[tokio::test]
    async fn test_cache_available_saturates_at_zero() {
        let (store, dir) = make_store(1).await;
        write_file(dir.path(), "file1", 5 * 1024 * 1024);
        let available = store.get_cache_available_mib().await.unwrap();
        assert_eq!(available, 0);
    }

    // -- Pin / unpin tests --

    #[tokio::test]
    async fn test_pin_files_increments_refcount() {
        let (store, _dir) = make_store(100).await;
        let names = vec!["a".to_string(), "b".to_string()];

        store.pin_files(&names);
        store.pin_files(&names);

        let pinned = store.pinned_files.lock().unwrap();
        assert_eq!(*pinned.get("a").unwrap(), 2);
        assert_eq!(*pinned.get("b").unwrap(), 2);
    }

    #[tokio::test]
    async fn test_unpin_guard_decrements_refcount() {
        let (store, _dir) = make_store(100).await;
        let names = vec!["a".to_string()];

        // Pin twice
        store.pin_files(&names);
        let pinned_set = store.pin_files(&names);

        // Drop one guard — refcount should go from 2 to 1
        {
            let _guard = UnpinGuard {
                pinned_files: &store.pinned_files,
                files: pinned_set,
            };
        }

        let pinned = store.pinned_files.lock().unwrap();
        assert_eq!(*pinned.get("a").unwrap(), 1);
    }

    #[tokio::test]
    async fn test_unpin_guard_removes_at_zero() {
        let (store, _dir) = make_store(100).await;
        let names = vec!["a".to_string()];

        let pinned_set = store.pin_files(&names);

        {
            let _guard = UnpinGuard {
                pinned_files: &store.pinned_files,
                files: pinned_set,
            };
        }

        let pinned = store.pinned_files.lock().unwrap();
        assert!(!pinned.contains_key("a"));
    }

    // -- Eviction tests --

    #[tokio::test]
    async fn test_delete_least_recently_accessed_skips_pinned() {
        let (store, dir) = make_store(100).await;
        write_file(dir.path(), "old_pinned", 1024);
        // Sleep briefly so "newer" has a different atime
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        write_file(dir.path(), "newer", 1024);

        // Pin the older file
        store.pin_files(&["old_pinned".to_string()]);

        // Should delete "newer" since "old_pinned" is pinned
        let deleted = store.delete_least_recently_accessed_file().await.unwrap();
        assert_eq!(deleted.file_name().unwrap().to_str().unwrap(), "newer");

        // "old_pinned" should still exist
        assert!(dir.path().join("old_pinned").exists());
    }

    #[tokio::test]
    async fn test_delete_least_recently_accessed_no_files_errors() {
        let (store, _dir) = make_store(100).await;
        let result = store.delete_least_recently_accessed_file().await;
        assert!(matches!(result, Err(DeleteOldestFileError::NoFiles)));
    }

    #[tokio::test]
    async fn test_delete_least_recently_accessed_all_pinned_errors() {
        let (store, dir) = make_store(100).await;
        write_file(dir.path(), "only_file", 1024);
        store.pin_files(&["only_file".to_string()]);

        let result = store.delete_least_recently_accessed_file().await;
        assert!(matches!(result, Err(DeleteOldestFileError::NoFiles)));
    }

    #[tokio::test]
    async fn test_delete_least_recently_accessed_skips_temp_files() {
        let (store, dir) = make_store(100).await;
        // Create a temp file (simulating an in-progress download) and a normal file
        write_file(dir.path(), ".tmp.foo.some-uuid", 1024);
        write_file(dir.path(), "evictable", 1024);

        let deleted = store.delete_least_recently_accessed_file().await.unwrap();
        assert_eq!(deleted.file_name().unwrap().to_str().unwrap(), "evictable");

        // Temp file should still exist
        assert!(dir.path().join(".tmp.foo.some-uuid").exists());
    }

    #[tokio::test]
    async fn test_delete_least_recently_accessed_only_temp_files_errors() {
        let (store, dir) = make_store(100).await;
        write_file(dir.path(), ".tmp.bar.some-uuid", 1024);

        let result = store.delete_least_recently_accessed_file().await;
        assert!(matches!(result, Err(DeleteOldestFileError::NoFiles)));
    }

    // -- Orphaned temp file cleanup on startup --

    #[tokio::test]
    async fn test_new_cleans_up_orphaned_temp_files() {
        let dir = TempDir::new().unwrap();
        // Simulate orphaned temp files from a previous crash
        write_file(dir.path(), ".tmp.snapshot.abc-123", 1024);
        write_file(dir.path(), ".tmp.other.def-456", 1024);
        // And a real cached file
        write_file(dir.path(), "real_file.bin", 1024);

        assert!(dir.path().join(".tmp.snapshot.abc-123").exists());
        assert!(dir.path().join(".tmp.other.def-456").exists());

        let config = aws_sdk_s3::Config::builder()
            .behavior_version_latest()
            .region(aws_sdk_s3::config::Region::new("us-east-1"))
            .build();
        let client = Client::from_conf(config);
        let _store = S3SnapshotStore::new(dir.path().to_path_buf(), 100, client)
            .await
            .unwrap();

        // Temp files should be cleaned up
        assert!(!dir.path().join(".tmp.snapshot.abc-123").exists());
        assert!(!dir.path().join(".tmp.other.def-456").exists());
        // Real file should survive
        assert!(dir.path().join("real_file.bin").exists());
    }

    // -- ensure_space_impl tests --

    #[tokio::test]
    async fn test_ensure_space_noop_when_enough_room() {
        let (store, _dir) = make_store(100).await;
        // Empty cache, 100 MiB budget — requesting 50 should be fine
        store.ensure_space_impl(50).await.unwrap();
    }

    #[tokio::test]
    async fn test_ensure_space_rejects_oversized_request() {
        let (store, _dir) = make_store(10).await;
        let result = store.ensure_space_impl(20).await;
        assert!(matches!(
            result,
            Err(EnsureSpaceError::CacheNotLargeEnough { .. })
        ));
    }

    #[tokio::test]
    async fn test_ensure_space_evicts_to_make_room() {
        let (store, dir) = make_store(5).await;
        // Fill 4 MiB
        write_file(dir.path(), "file1", 2 * 1024 * 1024);
        write_file(dir.path(), "file2", 2 * 1024 * 1024);

        // Request 3 MiB — only 1 MiB available, need to evict
        store.ensure_space_impl(3).await.unwrap();

        // At least one file should have been deleted
        let used = store.get_cache_utilization_mib().await.unwrap();
        let available = 5 - used;
        assert!(available >= 3);
    }

    #[tokio::test]
    async fn test_ensure_space_skips_pinned_during_eviction() {
        let (store, dir) = make_store(3).await;
        // Fill cache: 2 MiB pinned + 1 MiB unpinned = 3 MiB, 0 available
        write_file(dir.path(), "pinned_file", 2 * 1024 * 1024);
        write_file(dir.path(), "evictable", 1024 * 1024);

        store.pin_files(&["pinned_file".to_string()]);

        // Request 1 MiB — should evict "evictable" but not "pinned_file"
        store.ensure_space_impl(1).await.unwrap();

        assert!(dir.path().join("pinned_file").exists());
        assert!(!dir.path().join("evictable").exists());
    }
}
