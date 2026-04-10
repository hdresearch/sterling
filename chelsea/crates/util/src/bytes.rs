/// Converts bytes to mebibytes, always rounding up.
pub fn bytes_to_mib_ceil(num_bytes: u64) -> u64 {
    (num_bytes.saturating_add(1024 * 1024 - 1)) / (1024 * 1024)
}

/// Converts kibibytes to mebibytes, always rounding up.
pub fn kib_to_mib_ceil(num_kib: u64) -> u64 {
    (num_kib.saturating_add(1023)) / 1024
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bytes_to_mib_ceil_exact() {
        assert_eq!(bytes_to_mib_ceil(0), 0);
        assert_eq!(bytes_to_mib_ceil(1024 * 1024), 1);
        assert_eq!(bytes_to_mib_ceil(2 * 1024 * 1024), 2);
    }

    #[test]
    fn test_bytes_to_mib_ceil_rounding() {
        assert_eq!(bytes_to_mib_ceil(1), 1);
        assert_eq!(bytes_to_mib_ceil(1024 * 1024 - 1), 1);
        assert_eq!(bytes_to_mib_ceil(1024 * 1024 + 1), 2);
        assert_eq!(bytes_to_mib_ceil(2 * 1024 * 1024 - 1), 2);
    }

    #[test]
    fn test_kib_to_mib_ceil_exact() {
        assert_eq!(kib_to_mib_ceil(0), 0);
        assert_eq!(kib_to_mib_ceil(1024), 1);
        assert_eq!(kib_to_mib_ceil(2048), 2);
    }

    #[test]
    fn test_kib_to_mib_ceil_rounding() {
        assert_eq!(kib_to_mib_ceil(1), 1);
        assert_eq!(kib_to_mib_ceil(1023), 1);
        assert_eq!(kib_to_mib_ceil(1025), 2);
        assert_eq!(kib_to_mib_ceil(2047), 2);
    }
}
