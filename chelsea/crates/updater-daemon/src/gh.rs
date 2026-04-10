use crate::UpdaterError;
use std::io::{self, ErrorKind};
use std::process::Command;

/// Get the GitHub token from AWS Secrets Manager using AWS CLI
pub fn get_github_token() -> Result<String, io::Error> {
    // First try to get from environment variable
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if !token.trim().is_empty() {
            return Ok(token);
        }
    }

    // Fallback to AWS Secrets Manager
    let output = Command::new("aws")
        .args([
            "secretsmanager",
            "get-secret-value",
            "--secret-id",
            "github-token-chelsea",
            "--query",
            "SecretString",
            "--output",
            "text",
        ])
        .output()?;

    if !output.status.success() {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::new(
            ErrorKind::Other,
            format!("AWS CLI command failed: {}", error_msg),
        ));
    }

    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if token.is_empty() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            "No token returned from AWS Secrets Manager",
        ));
    }

    Ok(token)
}

pub fn download_binary(url: &str, output_path: &str) -> Result<(), UpdaterError> {
    let token = get_github_token()
        .map_err(|e| UpdaterError::Authentication(format!("Failed to get GitHub token: {}", e)))?;

    let output = Command::new("curl")
        .args([
            "-L", // Follow redirects
            "-H",
            &format!("Authorization: token {}", token),
            "-H",
            "Accept: application/octet-stream",
            "-H",
            "User-Agent: chelsea-updater",
            "-o",
            output_path,
            url,
        ])
        .output()
        .map_err(|e| UpdaterError::Download(format!("Failed to execute curl: {}", e)))?;

    if !output.status.success() {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        return Err(UpdaterError::Download(format!(
            "curl command failed: {}",
            error_msg
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tracing::info;

    #[test]
    fn test_aws_cli_available() {
        // Test that AWS CLI is available on the system
        let result = Command::new("aws").arg("--version").output();
        assert!(
            result.is_ok(),
            "AWS CLI should be available for integration tests"
        );
    }

    #[test]
    fn test_get_github_token_integration() {
        // This is an integration test that requires:
        // 1. AWS CLI to be installed and configured
        // 2. Proper AWS credentials/permissions
        // 3. The secret 'github-token-chelsea' to exist in AWS Secrets Manager

        match get_github_token() {
            Ok(token) => {
                // If successful, verify the token looks reasonable
                assert!(!token.is_empty(), "Token should not be empty");
                assert!(token.len() > 10, "Token should be reasonably long");
                // GitHub tokens typically start with 'ghp_' for personal access tokens
                // or other prefixes, but we'll just check it's not obviously invalid
                assert!(!token.contains('\n'), "Token should not contain newlines");
                info!(
                    "Successfully retrieved GitHub token (length: {})",
                    token.len()
                );
            }
            Err(e) => {
                // Print the error for debugging but don't fail the test
                // since this might fail in CI/CD or local environments without proper AWS setup
                info!("Warning: Failed to retrieve GitHub token: {}", e);
                info!("This is expected if AWS CLI is not configured or the secret doesn't exist");

                // We can still test that we get the expected error types
                match e.kind() {
                    ErrorKind::NotFound => {
                        assert!(
                            e.to_string().contains("aws")
                                || e.to_string().contains("command not found")
                                || e.to_string().contains("No token returned"),
                            "Should be a command not found or no token error"
                        );
                    }
                    ErrorKind::Other => {
                        assert!(
                            e.to_string().contains("AWS CLI command failed"),
                            "Should be an AWS CLI command failure"
                        );
                    }
                    _ => {
                        // Other IO errors are also acceptable (permission denied, etc.)
                        info!("Got IO error: {}", e);
                    }
                }
            }
        }
    }

    #[test]
    fn test_error_handling() {
        // Test that our function properly handles command execution
        // We can test this by trying to run a command that will definitely fail
        let output = Command::new("aws")
            .args([
                "secretsmanager",
                "get-secret-value",
                "--secret-id",
                "non-existent-secret-that-should-not-exist-12345",
                "--query",
                "SecretString",
                "--output",
                "text",
            ])
            .output();

        match output {
            Ok(cmd_output) => {
                if !cmd_output.status.success() {
                    let error_msg = String::from_utf8_lossy(&cmd_output.stderr);
                    // This should contain some AWS error about the secret not existing
                    assert!(
                        error_msg.contains("ResourceNotFoundException")
                            || error_msg.contains("does not exist")
                            || error_msg.contains("not found")
                            || error_msg.contains("NoCredentialsError")
                            || error_msg.contains("Unable to locate credentials"),
                        "Should get a proper AWS error message, got: {}",
                        error_msg
                    );
                }
            }
            Err(_) => {
                // AWS CLI not available, which is fine for this test
                info!("AWS CLI not available - skipping error handling test");
            }
        }
    }
}
