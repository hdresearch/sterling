use std::path::{Path, PathBuf};

use sha2::{Digest, Sha512};

use crate::PathBufExt;

const CHECKSUM_ALGORITHM: &str = "sha512";

/// Writes the checksum of the specified file to the specified path
pub async fn write_checksum(checksum_path: &Path, file_path: &Path) -> Result<(), std::io::Error> {
    let mut sha512 = Sha512::new();
    std::io::copy(
        &mut std::fs::OpenOptions::new().read(true).open(file_path)?,
        &mut sha512,
    )?;
    let checksum = sha512.finalize();

    tokio::fs::create_dir_all(checksum_path.parent().unwrap_or(Path::new(""))).await?;
    tokio::fs::write(checksum_path, checksum).await
}

/// Writes the checksum of the specified file to the same directory as the original, and returns the new file
pub async fn write_checksum_to_same_directory(file_path: &Path) -> std::io::Result<PathBuf> {
    let checksum_path = with_checksum_extension(file_path);
    write_checksum(&checksum_path, file_path).await?;
    Ok(checksum_path)
}

/// Returns a copy of the given path with the appropriate file extension, eg: .sha512
pub fn with_checksum_extension(path: impl AsRef<Path>) -> PathBuf {
    path.as_ref()
        .to_path_buf()
        .with_added_extension(checksum_extension())
}

pub const fn checksum_extension() -> &'static str {
    CHECKSUM_ALGORITHM
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_extension_is_sha512() {
        assert_eq!(checksum_extension(), "sha512");
    }

    #[test]
    fn with_checksum_extension_adds_to_existing_ext() {
        let path = PathBuf::from("/tmp/image.ext4");
        assert_eq!(
            with_checksum_extension(&path),
            PathBuf::from("/tmp/image.ext4.sha512")
        );
    }

    #[test]
    fn with_checksum_extension_adds_when_no_ext() {
        let path = PathBuf::from("/tmp/myfile");
        assert_eq!(
            with_checksum_extension(&path),
            PathBuf::from("/tmp/myfile.sha512")
        );
    }

    #[test]
    fn with_checksum_extension_with_multiple_dots() {
        let path = PathBuf::from("/data/archive.tar.gz");
        assert_eq!(
            with_checksum_extension(&path),
            PathBuf::from("/data/archive.tar.gz.sha512")
        );
    }

    #[tokio::test]
    async fn write_checksum_creates_valid_sha512() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("data.bin");
        let checksum_path = dir.path().join("data.bin.sha512");

        tokio::fs::write(&file_path, b"hello world").await.unwrap();
        write_checksum(&checksum_path, &file_path).await.unwrap();

        assert!(checksum_path.exists());
        let checksum_bytes = tokio::fs::read(&checksum_path).await.unwrap();
        // SHA-512 produces 64 bytes (512 bits)
        assert_eq!(checksum_bytes.len(), 64);
    }

    #[tokio::test]
    async fn write_checksum_is_deterministic() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("data.bin");
        let cksum1_path = dir.path().join("cksum1");
        let cksum2_path = dir.path().join("cksum2");

        tokio::fs::write(&file_path, b"deterministic content")
            .await
            .unwrap();
        write_checksum(&cksum1_path, &file_path).await.unwrap();
        write_checksum(&cksum2_path, &file_path).await.unwrap();

        let c1 = tokio::fs::read(&cksum1_path).await.unwrap();
        let c2 = tokio::fs::read(&cksum2_path).await.unwrap();
        assert_eq!(c1, c2);
    }

    #[tokio::test]
    async fn different_content_produces_different_checksum() {
        let dir = tempfile::tempdir().unwrap();
        let file_a = dir.path().join("a.bin");
        let file_b = dir.path().join("b.bin");
        let cksum_a = dir.path().join("a.sha512");
        let cksum_b = dir.path().join("b.sha512");

        tokio::fs::write(&file_a, b"content A").await.unwrap();
        tokio::fs::write(&file_b, b"content B").await.unwrap();

        write_checksum(&cksum_a, &file_a).await.unwrap();
        write_checksum(&cksum_b, &file_b).await.unwrap();

        let ca = tokio::fs::read(&cksum_a).await.unwrap();
        let cb = tokio::fs::read(&cksum_b).await.unwrap();
        assert_ne!(ca, cb);
    }

    #[tokio::test]
    async fn write_checksum_to_same_directory_creates_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("image.ext4");
        tokio::fs::write(&file_path, b"disk image data")
            .await
            .unwrap();

        let checksum_path = write_checksum_to_same_directory(&file_path).await.unwrap();

        assert_eq!(checksum_path, dir.path().join("image.ext4.sha512"));
        assert!(checksum_path.exists());
    }

    #[tokio::test]
    async fn write_checksum_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("source.bin");
        let checksum_path = dir.path().join("sub/dir/checksum.sha512");

        tokio::fs::write(&file_path, b"data").await.unwrap();
        write_checksum(&checksum_path, &file_path).await.unwrap();

        assert!(checksum_path.exists());
    }
}
