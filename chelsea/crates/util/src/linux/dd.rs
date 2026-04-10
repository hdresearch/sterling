use std::path::Path;

use anyhow::{anyhow, Result};
use tokio::process::Command;

pub struct DDOptions {
    /// Block size for transfer, in bytes
    block_size: u64,
}

impl Default for DDOptions {
    fn default() -> Self {
        Self {
            block_size: 4 * 1024 * 1024, // 4 MB
        }
    }
}

pub async fn dd(
    input_file: impl AsRef<Path>,
    output_file: impl AsRef<Path>,
    options: DDOptions,
) -> Result<()> {
    let input_file = input_file.as_ref();
    let output_file = output_file.as_ref();

    let output = Command::new("dd")
        .arg(format!("if={}", input_file.display()))
        .arg(format!("of={}", output_file.display()))
        .arg(format!("bs={}", options.block_size))
        .output()
        .await?;

    match output.status.success() {
        true => Ok(()),
        false => Err(anyhow!(
            "failed to execute 'dd if={} of={} bs={}': {}",
            input_file.display(),
            output_file.display(),
            options.block_size,
            String::from_utf8_lossy(&output.stderr),
        )),
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dd_copies_file_contents() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.bin");
        let output = dir.path().join("output.bin");

        let data = b"hello dd world";
        tokio::fs::write(&input, data).await.unwrap();

        dd(&input, &output, DDOptions::default()).await.unwrap();

        let result = tokio::fs::read(&output).await.unwrap();
        // dd pads to block size, but the content at the start must match
        assert_eq!(&result[..data.len()], data);
    }

    #[tokio::test]
    async fn dd_copies_larger_file() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.bin");
        let output = dir.path().join("output.bin");

        // Create a 1 MiB file of 0xAA bytes
        let data = vec![0xAAu8; 1024 * 1024];
        tokio::fs::write(&input, &data).await.unwrap();

        dd(&input, &output, DDOptions::default()).await.unwrap();

        let result = tokio::fs::read(&output).await.unwrap();
        assert_eq!(result, data);
    }

    #[tokio::test]
    async fn dd_fails_with_missing_input() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("nonexistent");
        let output = dir.path().join("output.bin");

        let result = dd(&input, &output, DDOptions::default()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn dd_creates_output_file() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.bin");
        let output = dir.path().join("output.bin");

        tokio::fs::write(&input, b"data").await.unwrap();
        assert!(!output.exists());

        dd(&input, &output, DDOptions::default()).await.unwrap();

        assert!(output.exists());
    }
}
