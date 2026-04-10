mod docker;
mod docker_dump;
pub mod error;
mod types;

pub use docker_dump::*;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tokio::fs::create_dir;
    use tracing::{debug, level_filters::LevelFilter, warn};
    use tracing_subscriber::EnvFilter;
    use util::{random_tmp_dir, random_tmp_file};

    use crate::{error::Error, TempDockerDump};

    /// A temporary file that is automatically removed when dropped.
    pub struct TempFileGuard {
        pub path: PathBuf,
    }

    impl TempFileGuard {
        pub fn new(path: PathBuf) -> Self {
            debug!(?path, "Creating temporary file guard");
            Self { path }
        }
    }

    impl Drop for TempFileGuard {
        fn drop(&mut self) {
            debug!(path=?self.path, "Removing temporary file");
            if let Err(e) = std::fs::remove_file(&self.path) {
                let path_str = self
                    .path
                    .to_str()
                    .unwrap_or("(failed to convert path to str)");
                warn!("Error removing temporary file {path_str}: {e}");
            } else {
                debug!(path=?self.path, "Successfully removed temporary file");
            }
        }
    }

    /// A temporary directory that is automatically removed when dropped.
    pub struct TempDirGuard {
        pub path: PathBuf,
        pub recursive: bool,
    }
    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            debug!(path=?self.path, "Removing temporary directory");
            if self.recursive {
                if let Err(e) = std::fs::remove_dir_all(&self.path) {
                    let path_str = self
                        .path
                        .to_str()
                        .unwrap_or("(failed to convert path to str)");
                    warn!("Error removing temporary directory {path_str} recursively: {e}");
                } else {
                    debug!(path=?self.path, "Successfully removed temporary directory recursively");
                }
            } else {
                if let Err(e) = std::fs::remove_dir(&self.path) {
                    let path_str = self
                        .path
                        .to_str()
                        .unwrap_or("(failed to convert path to str)");
                    warn!("Error removing temporary directory {path_str}: {e}");
                } else {
                    debug!(path=?self.path, "Successfully removed temporary directory");
                }
            }
        }
    }

    pub async fn create_dir_temp(path: PathBuf, recursive: bool) -> Result<TempDirGuard, Error> {
        debug!(?path, "Creating temporary directory");
        create_dir(&path)
            .await
            .map_err(|e| Error::IoError(e.to_string()))?;
        debug!(?path, "Successfully created temporary directory");
        Ok(TempDirGuard { path, recursive })
    }

    #[tokio::test]
    async fn test_docker_dump() -> Result<(), Error> {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env().add_directive(LevelFilter::DEBUG.into()))
            .init();

        use tokio::fs::read_to_string;

        // Create a temporary Dockerfile
        let dockerfile =
            TempFileGuard::new(random_tmp_file("boatswain", "Dockerfile.test").unwrap());
        tokio::fs::write(
            &dockerfile.path,
            r#"FROM alpine:latest
RUN mkdir /test-folder
RUN echo "Hello from the container!" > /test-folder/test.txt
RUN apk add python3
RUN python3 -c "print('Python was here')" > /test-folder/python-output.txt"#,
        )
        .await
        .map_err(|e| Error::IoError(e.to_string()))?;

        // Create the Docker dump
        let temp_dir = random_tmp_dir("boatswain").unwrap();
        let output_file = temp_dir.join("image.tar");
        let dump = TempDockerDump::new(&dockerfile.path, output_file).await?;

        // Extract tar to temp dir
        let extract_dir = create_dir_temp(temp_dir.join("extract"), true).await?;
        dump.extract(&extract_dir.path)
            .await
            .expect("failed to extract TempDockerDump contents");

        // Verify image contents
        let test_txt = read_to_string(extract_dir.path.join("test-folder/test.txt"))
            .await
            .map_err(|e| Error::IoError(e.to_string()))?;
        assert_eq!(test_txt.trim(), "Hello from the container!");

        let python_txt = read_to_string(extract_dir.path.join("test-folder/python-output.txt"))
            .await
            .map_err(|e| Error::IoError(e.to_string()))?;
        assert_eq!(python_txt.trim(), "Python was here");

        // The guards will clean everything up when they go out of scope
        Ok(())
    }
}
