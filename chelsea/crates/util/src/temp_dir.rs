use std::fs;
use std::io;
use std::path::PathBuf;
use tokio::fs::create_dir;
use tracing::warn;

pub struct TempDir {
    pub path: PathBuf,
    pub recursive_delete: bool,
}

impl TempDir {
    pub fn new(path: PathBuf, recursive_delete: bool) -> io::Result<Self> {
        Ok(TempDir {
            path,
            recursive_delete,
        })
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if self.recursive_delete {
            if let Err(error) = fs::remove_dir_all(&self.path) {
                warn!(%error, path = ?self.path, "Failed to delete temporary directory");
            };
        } else {
            if let Err(error) = fs::remove_dir(&self.path) {
                warn!(%error, path = ?self.path, "Failed to delete temporary directory")
            }
        }
    }
}

pub async fn create_temp_dir(path: PathBuf, recursive_delete: bool) -> io::Result<TempDir> {
    if let Err(error) = create_dir(&path).await {
        warn!(?path, %error, "Failed to create directory");
        return Err(error);
    };
    TempDir::new(path, recursive_delete)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_temp_dir_creates_directory() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("mydir");

        let td = create_temp_dir(path.clone(), false).await.unwrap();
        assert!(td.path.is_dir());
    }

    #[tokio::test]
    async fn drop_removes_empty_dir() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("empty_dir");

        let td = create_temp_dir(path.clone(), false).await.unwrap();
        assert!(path.is_dir());
        drop(td);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn recursive_delete_removes_contents() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("rec_dir");

        let td = create_temp_dir(path.clone(), true).await.unwrap();
        // Create a file inside
        tokio::fs::write(path.join("child.txt"), b"hello")
            .await
            .unwrap();
        assert!(path.join("child.txt").exists());

        drop(td);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn non_recursive_delete_fails_silently_with_contents() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("nonempty_dir");

        let td = create_temp_dir(path.clone(), false).await.unwrap();
        tokio::fs::write(path.join("child.txt"), b"hello")
            .await
            .unwrap();

        // Drop will try remove_dir (not remove_dir_all), which fails on non-empty dir.
        // It logs a warning but doesn't panic.
        drop(td);
        // Directory still exists because it wasn't empty
        assert!(path.exists());

        // Manual cleanup
        fs::remove_dir_all(&path).unwrap();
    }

    #[tokio::test]
    async fn create_fails_when_parent_missing() {
        let result = create_temp_dir(PathBuf::from("/no/such/parent/dir"), false).await;
        assert!(result.is_err());
    }
}
