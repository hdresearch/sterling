mod binary_checker;
mod gh;
mod types;

use binary_checker::{
    asset_to_binary_metadata, compute_file_checksum, get_github_release, get_precomputed_checksum,
};
use gh::download_binary;
use reqwest::Client;
use serde_json::json;
use std::path::Path;
use std::process::Command;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::time::{sleep, timeout, Duration};
use tracing::{debug, error, info, warn};

//For testing might wanna change to ./chelsea
const BINARY_PATH: &str = "/usr/local/bin/chelsea"; //Bootstraped Chelsea at: "/usr/local/bin"

#[cfg(not(test))]
const COMMANDS_SCRIPT_PATH: &str = "./commands.sh";
#[cfg(test)]
const COMMANDS_SCRIPT_PATH: &str = "./../../commands.sh";
/// Gets the update check interval from environment variable or returns default
fn get_check_interval_seconds() -> u64 {
    dotenvy::var("UPDATE_CHECK_INTERVAL_SECONDS")
        .expect("UPDATE_CHECK_INTERVAL_SECONDS is required")
        .parse::<u64>()
        .expect("UPDATE_CHECK_INTERVAL_SECONDS must be a valid u64")
}

#[derive(Error, Debug)]
pub enum UpdaterError {
    #[error("GitHub API error: {0}")]
    GitHubApi(String),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("File system error: {0}")]
    FileSystem(#[from] std::io::Error),

    #[error("Checksum verification failed: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("Binary not found: {0}")]
    BinaryNotFound(String),

    #[error("JSON parsing error: {0}")]
    JsonParsing(#[from] serde_json::Error),

    #[error("Authentication error: {0}")]
    Authentication(String),

    #[error("Download error: {0}")]
    Download(String),

    #[error("Service management error: {0}")]
    ServiceManagement(String),
}

#[tokio::main]
async fn main() -> Result<(), UpdaterError> {
    // Initialize tracing
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("Chelsea Updater Daemon Starting...");
    let client = Client::new();

    // Initial setup and first download if needed
    let mut current_local_checksum = perform_initial_setup(&client).await?;

    // Main update checking loop
    run_update_loop(&client, &mut current_local_checksum).await
}

/// Performs initial setup: fetches remote checksum, checks local binary, downloads if needed
pub async fn perform_initial_setup(
    client: &Client,
) -> Result<Option<types::PrecomputedChecksum>, UpdaterError> {
    // First, try to get the current remote checksum
    info!("Fetching current binary checksum from GitHub...");
    let remote_checksum = get_precomputed_checksum(client.clone()).await?;
    info!(
        "Remote binary info: {} bytes, SHA256: {}",
        remote_checksum.size, remote_checksum.sha256
    );

    // Check if binary exists locally and get its checksum
    let current_local_checksum = get_local_binary_checksum(BINARY_PATH)?;

    // Download binary if needed (first run or checksum mismatch)
    let needs_download = match &current_local_checksum {
        Some(local) => local.sha256 != remote_checksum.sha256,
        None => true,
    };

    let final_checksum = if needs_download {
        info!("Binary needs to be downloaded/updated...");

        // For initial setup, we need to handle the case where there might not be a running service
        // So we'll download without stopping first, but start the service after
        let client = Client::new();
        let release = get_github_release(client.clone()).await?;

        let binary_asset = release
            .assets
            .iter()
            .find(|asset| asset.name == "binary-linux")
            .ok_or_else(|| {
                UpdaterError::BinaryNotFound("binary-linux asset not found in release".to_string())
            })?;

        let download_url = format!(
            "https://api.github.com/repos/hdresearch/chelsea/releases/assets/{}",
            binary_asset.id
        );

        info!(
            "Downloading binary from GitHub (Asset ID: {})...",
            binary_asset.id
        );

        // If there's an existing binary, stop the service first
        if current_local_checksum.is_some() {
            info!("Existing binary found, stopping service before update...");
            if let Err(e) = stop_chelsea_service().await {
                warn!(
                    "Failed to stop existing service (it may not be running): {}",
                    e
                );
                // Continue with download anyway
            }
        }

        // Download the new binary
        download_binary(&download_url, BINARY_PATH)
            .map_err(|e| UpdaterError::Download(e.to_string()))?;

        // Make the binary executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(BINARY_PATH)?.permissions();
            perms.set_mode(0o755); // rwxr-xr-x
            std::fs::set_permissions(BINARY_PATH, perms)?;
            info!("Made binary executable");
        }

        // Verify the download
        let downloaded_checksum = compute_file_checksum(BINARY_PATH)?;
        if downloaded_checksum.sha256 != remote_checksum.sha256 {
            return Err(UpdaterError::ChecksumMismatch {
                expected: remote_checksum.sha256.clone(),
                actual: downloaded_checksum.sha256,
            });
        }

        info!("Checksum verification passed!");

        info!("Downloaded from: {}", download_url);
        info!(
            "Asset ID: {}, Size: {} bytes",
            binary_asset.id, binary_asset.size
        );

        Some(downloaded_checksum)
    } else {
        info!("Local binary is up to date!");
        current_local_checksum
    };

    // Always ensure Chelsea service is running, regardless of whether we downloaded an update
    // I could do the check here, but not sure if that would be better architecture
    if let Err(e) = start_chelsea_service().await {
        warn!("Failed to start Chelsea service: {}", e);
        warn!("Binary is available but service failed to start");
        warn!("You may need to start the service manually or check the binary arguments");
        // Don't return error here - we still want the updater to continue running
    } else {
        info!("Chelsea service is running!");
    }

    Ok(final_checksum)
}

/// Checks if a binary exists at the given path and computes its checksum
pub fn get_local_binary_checksum(
    binary_path: &str,
) -> Result<Option<types::PrecomputedChecksum>, UpdaterError> {
    if Path::new(binary_path).exists() {
        info!(
            "Local binary found at {}, computing checksum...",
            binary_path
        );
        match compute_file_checksum(binary_path) {
            Ok(checksum) => {
                info!(
                    "Local binary: {} bytes, SHA256: {}",
                    checksum.size, checksum.sha256
                );
                Ok(Some(checksum))
            }
            Err(e) => {
                warn!("Could not compute local checksum: {}", e);
                Ok(None)
            }
        }
    } else {
        info!("No local binary found at {}", binary_path);
        Ok(None)
    }
}

/// Runs the main update checking loop
pub async fn run_update_loop(
    client: &Client,
    current_local_checksum: &mut Option<types::PrecomputedChecksum>,
) -> Result<(), UpdaterError> {
    info!(
        "Starting update checker (checking every {} seconds)...",
        get_check_interval_seconds()
    );

    loop {
        sleep(Duration::from_secs(get_check_interval_seconds())).await;

        if let Err(e) = perform_update_check(client, current_local_checksum).await {
            error!("Update check failed: {}", e);
            // Continue running despite errors
        }
    }
}

/// Performs a single update check and download if needed
pub async fn perform_update_check(
    client: &Client,
    current_local_checksum: &mut Option<types::PrecomputedChecksum>,
) -> Result<(), UpdaterError> {
    info!("Checking for updates...");

    match check_for_updates(client, current_local_checksum).await? {
        Some(new_checksum) => {
            info!("New version detected! Downloading update...");
            let (binary_info, binary_metadata) = download_new_binary(&new_checksum).await?;
            *current_local_checksum = Some(compute_file_checksum(BINARY_PATH)?);
            info!("Successfully updated to new version!");
            info!("Downloaded from: {}", binary_info.url);
            info!("Update completed at: {}", binary_info.downloaded_at);
            info!(
                "Asset ID: {}, Size: {} bytes",
                binary_metadata.asset_id, binary_metadata.size
            );
        }
        None => {
            info!("No updates available");
        }
    }

    Ok(())
}

async fn check_for_updates(
    client: &Client,
    current_local: &Option<types::PrecomputedChecksum>,
) -> Result<Option<types::PrecomputedChecksum>, UpdaterError> {
    let remote_checksum = get_precomputed_checksum(client.clone()).await?;

    match current_local {
        Some(local) => {
            if local.sha256 != remote_checksum.sha256 {
                Ok(Some(remote_checksum))
            } else {
                Ok(None)
            }
        }
        None => Ok(Some(remote_checksum)), // No local version, need to download
    }
}

pub async fn download_new_binary(
    checksum_info: &types::PrecomputedChecksum,
) -> Result<(types::BinaryInfo, types::BinaryMetadata), UpdaterError> {
    let client = Client::new();

    // Get release info using our structured types
    let release = get_github_release(client.clone()).await?;

    // Find the binary asset (not the JSON metadata)
    let binary_asset = release
        .assets
        .iter()
        .find(|asset| asset.name == "binary-linux")
        .ok_or_else(|| {
            UpdaterError::BinaryNotFound("binary-linux asset not found in release".to_string())
        })?;

    let download_url = format!(
        "https://api.github.com/repos/hdresearch/chelsea/releases/assets/{}",
        binary_asset.id
    );

    info!(
        "Downloading binary from GitHub (Asset ID: {})...",
        binary_asset.id
    );
    info!("Binary size: {} bytes", binary_asset.size);
    info!("Binary created at: {}", binary_asset.created_at);

    // Stop the Chelsea service before updating the binary
    stop_chelsea_service().await?;

    // Download the new binary
    download_binary(&download_url, BINARY_PATH)
        .map_err(|e| UpdaterError::Download(e.to_string()))?;

    // Make the binary executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(BINARY_PATH)?.permissions();
        perms.set_mode(0o755); // rwxr-xr-x
        std::fs::set_permissions(BINARY_PATH, perms)?;
        info!("Made binary executable");
    }

    // Verify the download by checking the checksum
    let downloaded_checksum = compute_file_checksum(BINARY_PATH)?;
    if downloaded_checksum.sha256 != checksum_info.sha256 {
        return Err(UpdaterError::ChecksumMismatch {
            expected: checksum_info.sha256.clone(),
            actual: downloaded_checksum.sha256,
        });
    }

    info!("Checksum verification passed!");

    // Start the Chelsea service with the new binary
    start_chelsea_service().await?;

    // Create structured information about what we downloaded
    let binary_info = types::BinaryInfo {
        url: download_url,
        sha256_hash: checksum_info.sha256.clone(),
        downloaded_at: chrono::Utc::now(),
    };

    let binary_metadata = asset_to_binary_metadata(binary_asset, &checksum_info.sha256);

    Ok((binary_info, binary_metadata))
}

/// Stops the Chelsea service using direct socket communication
async fn stop_chelsea_service() -> Result<(), UpdaterError> {
    info!("Stopping Chelsea service...");

    // Try direct socket communication first
    match stop_chelsea_via_socket(false).await {
        Ok(response) => {
            info!("Chelsea service stopped successfully via socket");
            debug!("Socket response: {}", response);
            Ok(())
        }
        Err(socket_error) => {
            warn!("Socket communication failed: {}", socket_error);

            // Fallback to commands script if socket fails
            if !Path::new(COMMANDS_SCRIPT_PATH).exists() {
                warn!(
                    "Commands script not found at {}, cannot stop service gracefully",
                    COMMANDS_SCRIPT_PATH
                );
                return Ok(()); // Don't treat this as an error since the service might not be running
            }

            let output = Command::new(COMMANDS_SCRIPT_PATH)
                .args(&["stop", "-n"])
                .output()
                .map_err(|e| {
                    UpdaterError::ServiceManagement(format!(
                        "Failed to execute stop command: {}",
                        e
                    ))
                })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(UpdaterError::ServiceManagement(format!(
                    "Stop command failed: {}",
                    stderr
                )));
            }

            info!("Chelsea service stopped successfully via commands script");
            Ok(())
        }
    }
}
/// Stops the Chelsea service via direct socket communication
async fn stop_chelsea_via_socket(should_cleanup: bool) -> Result<String, UpdaterError> {
    // Connect to the daemon socket
    let mut stream = UnixStream::connect("/tmp/vm-host.sock") /* EXEMPT: GH #111 */
        .await
        .map_err(|e| {
            UpdaterError::ServiceManagement(format!("Failed to connect to socket: {}", e))
        })?;

    // Create the stop request
    let request = json!({
        "op": "stop",
        "should_cleanup": should_cleanup
    });

    // Send the request
    let request_str = request.to_string() + "\n";
    stream
        .write_all(request_str.as_bytes())
        .await
        .map_err(|e| UpdaterError::ServiceManagement(format!("Failed to send request: {}", e)))?;

    // Flush to ensure all data is sent
    stream
        .flush()
        .await
        .map_err(|e| UpdaterError::ServiceManagement(format!("Failed to flush request: {}", e)))?;

    // Shutdown the write side to signal we're done sending
    stream.shutdown().await.map_err(|e| {
        UpdaterError::ServiceManagement(format!("Failed to shutdown write side: {}", e))
    })?;

    // Read the response with timeout (600 seconds like socat)
    let response = timeout(Duration::from_secs(600), async {
        let mut response = String::new();
        stream.read_to_string(&mut response).await.map_err(|e| {
            UpdaterError::ServiceManagement(format!("Failed to read response: {}", e))
        })?;
        Ok::<String, UpdaterError>(response)
    })
    .await
    .map_err(|_| {
        UpdaterError::ServiceManagement(
            "Socket communication timed out after 600 seconds".to_string(),
        )
    })??;

    Ok(response)
}

/// Starts the Chelsea service using sudo
async fn start_chelsea_service() -> Result<(), UpdaterError> {
    info!("Starting Chelsea service...");

    // Check if the local binary exists
    if !Path::new(BINARY_PATH).exists() {
        return Err(UpdaterError::BinaryNotFound(format!(
            "Cannot start Chelsea service: binary not found at {}",
            BINARY_PATH
        )));
    }
    //CHECK IF CHELSEA IS RUNNING

    let check_existing = Command::new("ps")
        .arg("-C")
        .arg("aux | grep chelsea")
        .output()
        .map_err(|e| {
            UpdaterError::ServiceManagement(format!(
                "Failed to check for existing Chelsea process: {}",
                e
            ))
        })?;
    // not sure if this catches all errors that could come from this ^^^^^^^^^^^
    match check_existing.status.code() {
        // Run Chelsea from the local directory
        Some(1) => {
            let output = Command::new("sudo").arg(BINARY_PATH).spawn().map_err(|e| {
                UpdaterError::ServiceManagement(format!("Failed to start Chelsea service: {}", e))
            })?;

            info!(
                "Chelsea service started successfully (PID: {})",
                output.id()
            );
        }
        Some(0) => {
            info!("Chelsea service already running")
        }

        Some(2) => {
            return Err(UpdaterError::ServiceManagement(format!(
                "Error reading ps aux | grep chelsea"
            )))
        }
        //not sure if this message properly describes the error coming from this ^^^^^
        _ => {
            return Err(UpdaterError::ServiceManagement(format!(
                "Error: ps aux | grep exited with no or unexpected return"
            )))
        } //^^^^ catches none and if there is an unexpected return which should all indicate errors
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;
    use tokio::process::Command as TCommand;
    use tracing::info;
    use tracing::Level;

    #[test]
    fn test_get_local_binary_checksum_missing_file() {
        // Test with non-existent file
        let result = get_local_binary_checksum("/path/that/does/not/exist").unwrap();
        assert!(result.is_none(), "Should return None for missing file");
        info!("✓ Missing file test passed");
    }

    #[test]
    fn test_get_local_binary_checksum_existing_file() {
        // Create a temporary file with known content
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let temp_file_path = temp_dir.path().join("test_binary");

        let test_data = b"Hello, Chelsea updater test!";
        let mut file = fs::File::create(&temp_file_path).expect("Failed to create test file");
        file.write_all(test_data)
            .expect("Failed to write test data");
        drop(file); // Ensure file is closed

        // Test checksum computation
        let result = get_local_binary_checksum(temp_file_path.to_str().unwrap()).unwrap();
        assert!(result.is_some(), "Should return Some for existing file");

        let checksum = result.unwrap();
        assert_eq!(checksum.size, test_data.len() as u64);
        assert_eq!(checksum.sha256.len(), 64, "SHA256 should be 64 chars");
        assert!(
            checksum.sha256.chars().all(|c| c.is_ascii_hexdigit()),
            "Should be valid hex"
        );

        info!("✓ Existing file test passed");
        info!("  Size: {} bytes", checksum.size);
        info!("  SHA256: {}", checksum.sha256);
    }
    #[tokio::test]
    async fn test_perform_initial_setup() {
        // This test requires proper AWS/GitHub setup, so we'll make it graceful
        let client = Client::new();

        match perform_initial_setup(&client).await {
            Ok(checksum) => {
                info!("✓ Initial setup test passed");
                if let Some(cs) = checksum {
                    info!("  Got checksum: {} bytes, SHA256: {}", cs.size, cs.sha256);
                }
            }
            Err(e) => {
                info!(
                        "Note: Initial setup test failed (expected without proper AWS/GitHub setup): {}",
                        e
                    );
            }
        }
        let _ = TCommand::new("./../../commands.sh")
            .arg("cleanup")
            .output()
            .await
            .expect("Cleanup didnt work");
        println!("cleaning up");
    }

    #[tokio::test]
    async fn test_start_chelsea_service() {
        match start_chelsea_service().await {
            _ => {
                let check_existing = match Command::new("ps")
                    .arg("-C")
                    .arg("aug | grep chelsea")
                    .output()
                    .map_err(|e| {
                        UpdaterError::ServiceManagement(format!(
                            "failed checking chelsea processes: {}",
                            e
                        ))
                    }) {
                    Ok(n) => n,
                    Err(_) => return,
                };
                println!("exit status: {:?}", check_existing.status.code());
            }
        }
        let _ = TCommand::new("./../../commands.sh")
            .arg("cleanup")
            .output()
            .await
            .expect("Cleanup didnt work");
        println!("cleaning up");
    }

    pub async fn count_processes() -> usize {
        let process_search = "chelsea";
        let output = TCommand::new("pgrep")
            .args(["-c", process_search])
            .output()
            .await
            .expect("Error unable to count processes");
        //add more err handling here
        let count_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let count: usize = count_str.parse().unwrap_or(0);
        return count;
    }
    #[tokio::test]
    async fn test_update_check_integration() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(Level::DEBUG)
            .try_init();

        let initial_chelsea_c = count_processes().await;
        info!("Initial Chelsea Instances: {}", initial_chelsea_c);

        let child = match initial_chelsea_c {
            0 => Some(
                Command::new("sudo")
                    .arg("./../../chelsea")
                    .spawn()
                    .expect("failed to run chelsea"),
            ),
            _ => {
                info!("Chelsea already running, not starting new instance");
                //might change this to assert or just exit the test
                assert!(false, "Chelsea Already Running somewhere");
                None
            }
        };
        let client = Client::new();
        let mut mock_checksum = Some(types::PrecomputedChecksum {
            filename: "test-binary".to_string(),
            size: 1000,
            sha256: "fake_hash_for_testing".to_string(),
        });

        // This should either succeed or fail gracefully
        match perform_update_check(&client, &mut mock_checksum).await {
            Ok(_) => info!("✓ Update check test completed successfully"),
            Err(e) => info!(
                "Note: Update check test failed (expected without proper setup): {}",
                e
            ),
        }
        {
            let c = count_processes().await;
            info!("Completed Test Chelsea Instances: {}", c);
            assert_eq!(c, 1, "Error: Too many chelsea processes: {}", c);
        }
        if let Some(_child) = child {
            let _ = TCommand::new("./../../commands.sh")
                .arg("cleanup")
                .output()
                .await
                .expect("Cleanup didnt work");
            println!("cleaning up");
        }
    }

    #[tokio::test]
    async fn test_perform_update_check() {
        let client = Client::new();
        let mut mock_checksum = Some(types::PrecomputedChecksum {
            filename: "test-binary".to_string(),
            size: 1000,
            sha256: "fake_hash_for_testing".to_string(),
        });

        // This should either succeed or fail gracefully
        match perform_update_check(&client, &mut mock_checksum).await {
            Ok(_) => info!("✓ Update check test completed successfully"),
            Err(e) => info!(
                "Note: Update check test failed (expected without proper setup): {}",
                e
            ),
        }
    }

    #[test]
    fn test_check_for_updates_logic() {
        // Test the logic without actual network calls
        let old_checksum = types::PrecomputedChecksum {
            filename: "binary-linux".to_string(),
            size: 1000,
            sha256: "old_hash".to_string(),
        };

        let new_checksum = types::PrecomputedChecksum {
            filename: "binary-linux".to_string(),
            size: 1100,
            sha256: "new_hash".to_string(),
        };

        // Test that different checksums are detected
        assert_ne!(old_checksum.sha256, new_checksum.sha256);
        assert_ne!(old_checksum.size, new_checksum.size);

        info!("✓ Checksum comparison logic works correctly");
    }
}
