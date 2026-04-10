use std::io;
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecretReadError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("AWS CLI command failed: {0}")]
    AwsCliFailed(String),
    #[error("No secret returned from AWS Secrets Manager")]
    SecretEmpty,
}

/// Retrieve a secret string from AWS Secrets Manager using the AWS CLI.
///
/// # Arguments
/// * `secret_id` - The id of the secret to retrieve.
///
/// # Returns
/// * `Ok(secret_string)` if successful
/// * `Err(SecretReadError)` if there is any problem
pub fn read_secret_string(secret_id: &str) -> Result<String, SecretReadError> {
    let output = Command::new("aws")
        .args([
            "secretsmanager",
            "get-secret-value",
            "--secret-id",
            secret_id,
            "--query",
            "SecretString",
            "--output",
            "text",
        ])
        .output()?;

    if !output.status.success() {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        return Err(SecretReadError::AwsCliFailed(error_msg.to_string()));
    }

    let secret = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if secret.is_empty() {
        return Err(SecretReadError::SecretEmpty);
    }

    Ok(secret)
}
