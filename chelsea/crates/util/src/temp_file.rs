use std::fs;
use std::io;
use std::path::PathBuf;
use tracing::warn;

#[derive(Debug)]
pub struct TempFile {
    pub path: PathBuf,
}

impl TempFile {
    pub fn new(path: PathBuf) -> Self {
        TempFile { path }
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.path) {
            warn!(%error, path = ?self.path, "Failed to delete temporary file");
        }
    }
}

pub fn create_temp_file(path: PathBuf) -> io::Result<TempFile> {
    // Create an empty file
    std::fs::File::create(&path)?;
    Ok(TempFile::new(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_temp_file_creates_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("testfile");

        let tf = create_temp_file(path.clone()).unwrap();
        assert!(tf.path.exists());
        assert_eq!(std::fs::read(&tf.path).unwrap().len(), 0);
    }

    #[test]
    fn drop_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("will_be_deleted");

        let tf = create_temp_file(path.clone()).unwrap();
        assert!(path.exists());
        drop(tf);
        assert!(!path.exists());
    }

    #[test]
    fn new_does_not_create_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent");

        // TempFile::new just wraps the path — doesn't touch the filesystem
        let _tf = TempFile::new(path.clone());
        assert!(!path.exists());
    }

    #[test]
    fn create_fails_when_parent_missing() {
        let result = create_temp_file(PathBuf::from("/no/such/parent/dir/file.tmp"));
        assert!(result.is_err());
    }
}
