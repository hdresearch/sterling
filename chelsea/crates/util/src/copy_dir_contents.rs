use anyhow::{anyhow, Result};
use std::path::Path;

pub async fn copy_dir_contents(src: &Path, dest: &Path) -> Result<()> {
    let mut entries = tokio::fs::read_dir(src).await?;

    while let Ok(Some(entry)) = entries.next_entry().await {
        let src_path = entry.path();
        let dest_path = dest.join(
            src_path
                .file_name()
                .ok_or_else(|| anyhow!("Invalid file name in source directory"))?,
        );

        let metadata = tokio::fs::symlink_metadata(&src_path).await?;
        let file_type = metadata.file_type();

        if file_type.is_symlink() {
            let target = tokio::fs::read_link(&src_path).await?;
            tokio::fs::symlink(&target, &dest_path).await?;
        } else if file_type.is_dir() {
            tokio::fs::create_dir_all(&dest_path).await?;
            Box::pin(copy_dir_contents(&src_path, &dest_path)).await?;
        } else if file_type.is_file() {
            tokio::fs::copy(&src_path, &dest_path).await?;
        } else {
            // Skip other file types (sockets, devices, etc.)
            eprintln!("Skipping special file: {:?}", src_path);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn copies_files() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();

        tokio::fs::write(src.path().join("a.txt"), b"alpha")
            .await
            .unwrap();
        tokio::fs::write(src.path().join("b.txt"), b"beta")
            .await
            .unwrap();

        copy_dir_contents(src.path(), dest.path()).await.unwrap();

        assert_eq!(
            tokio::fs::read(dest.path().join("a.txt")).await.unwrap(),
            b"alpha"
        );
        assert_eq!(
            tokio::fs::read(dest.path().join("b.txt")).await.unwrap(),
            b"beta"
        );
    }

    #[tokio::test]
    async fn copies_subdirectories_recursively() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();

        tokio::fs::create_dir(src.path().join("sub")).await.unwrap();
        tokio::fs::write(src.path().join("sub/nested.txt"), b"deep")
            .await
            .unwrap();

        copy_dir_contents(src.path(), dest.path()).await.unwrap();

        assert!(dest.path().join("sub").is_dir());
        assert_eq!(
            tokio::fs::read(dest.path().join("sub/nested.txt"))
                .await
                .unwrap(),
            b"deep"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn copies_symlinks_as_symlinks() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();

        tokio::fs::write(src.path().join("real.txt"), b"content")
            .await
            .unwrap();
        tokio::fs::symlink("real.txt", src.path().join("link.txt"))
            .await
            .unwrap();

        copy_dir_contents(src.path(), dest.path()).await.unwrap();

        let dest_link = dest.path().join("link.txt");
        let metadata = tokio::fs::symlink_metadata(&dest_link).await.unwrap();
        assert!(metadata.file_type().is_symlink());

        let target = tokio::fs::read_link(&dest_link).await.unwrap();
        assert_eq!(target, std::path::PathBuf::from("real.txt"));
    }

    #[tokio::test]
    async fn empty_source_directory() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();

        copy_dir_contents(src.path(), dest.path()).await.unwrap();

        let mut entries = tokio::fs::read_dir(dest.path()).await.unwrap();
        assert!(entries.next_entry().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn fails_when_source_does_not_exist() {
        let dest = tempfile::tempdir().unwrap();
        let result = copy_dir_contents(Path::new("/no/such/dir"), dest.path()).await;
        assert!(result.is_err());
    }
}
