use std::process::Command;

use nix::unistd::User;

#[derive(Debug)]
pub struct UserInfo {
    pub uid: u32,
    pub gid: u32,
}

#[derive(Debug)]
pub enum UserError {
    CommandFailed(String),
    ParseError(String),
    CreateUserFailed(String),
    NotFound(String),
}

impl std::fmt::Display for UserError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::CommandFailed(msg) => write!(f, "Command failed: {}", msg),
            Self::ParseError(msg) => write!(f, "Parse error: {}", msg),
            Self::CreateUserFailed(msg) => write!(f, "Failed to create user: {}", msg),
            Self::NotFound(user) => write!(f, "User '{user}' not found"),
        }
    }
}

impl std::error::Error for UserError {}

/// Get the uid and gid of a given user, creating it if it doesn't exist. The created user will not have a shell assigned to it.
pub fn get_or_create_system_user(username: &str) -> Result<UserInfo, UserError> {
    // First, try to get existing user info
    match get_user_info(username) {
        Ok(user_info) => return Ok(user_info),
        // if the user is not found, continue
        Err(UserError::NotFound(_)) => (),
        Err(other) => return Err(other),
    }

    // If it doesn't exist, create it
    let output = Command::new("sudo")
        .arg("useradd")
        .arg("-r")
        .arg("-s")
        .arg("/bin/false")
        .arg(username)
        .output()
        .map_err(|e| UserError::CommandFailed(format!("Failed to execute useradd: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(UserError::CreateUserFailed(format!(
            "useradd failed with exit code {}: {}",
            output.status.code().unwrap_or(-1),
            stderr
        )));
    }

    // Get info for the newly-created user
    get_user_info(username)
}

/// Retrieve the UID and GID for a given user if it exists
fn get_user_info(username: &str) -> Result<UserInfo, UserError> {
    match User::from_name(username).map_err(|e| {
        UserError::CommandFailed(format!("Failed to look up user '{username}': {e}"))
    })? {
        Some(user) => Ok(UserInfo {
            uid: user.uid.as_raw() as u32,
            gid: user.gid.as_raw() as u32,
        }),
        None => Err(UserError::NotFound(username.to_string())),
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use nix::unistd::{geteuid, User as NixUser};

    #[test]
    fn returns_current_user_info() {
        let uid = geteuid();
        let user = NixUser::from_uid(uid)
            .expect("failed to resolve current user")
            .expect("current uid has no username entry");

        let info = get_or_create_system_user(&user.name)
            .expect("get_or_create_system_user should succeed");

        assert_eq!(
            info.uid,
            uid.as_raw() as u32,
            "expected uid to match current user"
        );
        assert_eq!(
            info.gid,
            user.gid.as_raw() as u32,
            "expected gid to match current user"
        );
    }
}
