use std::path::PathBuf;

/// DO NOT MODIFY; this appears to be a hard requirement of jailer.
const BINARY_NAME: &str = "firecracker";

/// The root directory in which all chroot jails will be created. DO NOT MODIFY; this is the default value used by jailer. While it can be customized,
/// this is a sensible default.
pub fn get_jailer_root() -> PathBuf {
    PathBuf::from("/srv/jailer")
}

/// Given a VM ID, construct the jailer path that points to its chroot base directory. This is constructed by jailer.
pub fn get_jailer_vm_path(vm_id: &str) -> PathBuf {
    get_jailer_root().join(BINARY_NAME).join(vm_id).join("root")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jailer_root_is_srv_jailer() {
        assert_eq!(get_jailer_root(), PathBuf::from("/srv/jailer"));
    }

    #[test]
    fn jailer_vm_path_contains_firecracker_and_vm_id() {
        let path = get_jailer_vm_path("abc-123");
        assert_eq!(path, PathBuf::from("/srv/jailer/firecracker/abc-123/root"));
    }

    #[test]
    fn jailer_vm_path_with_uuid_style_id() {
        let path = get_jailer_vm_path("550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(
            path,
            PathBuf::from("/srv/jailer/firecracker/550e8400-e29b-41d4-a716-446655440000/root")
        );
    }
}
