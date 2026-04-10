use async_trait::async_trait;
use std::path::Path;
use uuid::Uuid;
use vers_pg::schema::chelsea::tables::commit::CommitFile;

#[async_trait]
/// Represents an interface for storing commit files on some remote target, with a local cache
pub trait VmCommitStore: Send + Sync {
    /// Upload the provided files to the store, returning a Vec representing the files on the store for later retrieval.
    async fn upload_commit_files(
        &self,
        commit_id: &Uuid,
        to_upload: &[String],
    ) -> anyhow::Result<Vec<CommitFile>>;

    /// Retrieve the given files from the store, placing them into the commits subdir of the data directory.
    async fn download_commit_files(&self, to_download: &[CommitFile]) -> anyhow::Result<()>;

    /// The local path which is used for commit file uploads+downloads
    fn commit_dir<'a>(&'a self) -> &'a Path;

    /// Ensure there is at least {required_size_mib} of space available in the commit cache
    async fn ensure_space(&self, space_required_mib: u32) -> anyhow::Result<()>;
}
