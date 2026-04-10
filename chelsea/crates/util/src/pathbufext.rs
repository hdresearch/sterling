use std::path::{Path, PathBuf};

pub trait PathBufExt {
    fn append<P: AsRef<Path>>(&self, path: P) -> PathBuf;
    fn with_added_extension(&self, extension: &str) -> PathBuf;
}

impl PathBufExt for PathBuf {
    /// Returns a new PathBuf with the path appended, even if the second is absolute; this is in contrast to the behavior
    /// of `PathBuf.join(other)`, which will entirely replace the original path if `other` is absolute.
    fn append<P: AsRef<Path>>(&self, path: P) -> PathBuf {
        let path = path.as_ref();
        // Strip leading slash if present
        let path_str = path.to_string_lossy();
        let clean_path = path_str.strip_prefix('/').unwrap_or(&path_str);
        let mut new_path = self.clone();
        new_path.push(clean_path);
        new_path
    }

    /// Workaround while with_added_extension is experimental
    fn with_added_extension(&self, extension: &str) -> PathBuf {
        match self.extension() {
            Some(existing_ext) => {
                let mut new_ext = existing_ext.to_os_string();
                new_ext.push(".");
                new_ext.push(extension);
                self.with_extension(new_ext)
            }
            None => self.with_extension(extension),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── append ──

    #[test]
    fn append_relative_path() {
        let base = PathBuf::from("/srv/jailer");
        assert_eq!(base.append("vm/root"), PathBuf::from("/srv/jailer/vm/root"));
    }

    #[test]
    fn append_absolute_path_strips_leading_slash() {
        let base = PathBuf::from("/srv/jailer");
        assert_eq!(
            base.append("/etc/passwd"),
            PathBuf::from("/srv/jailer/etc/passwd")
        );
    }

    #[test]
    fn append_empty_path() {
        let base = PathBuf::from("/srv");
        assert_eq!(base.append(""), PathBuf::from("/srv"));
    }

    #[test]
    fn append_preserves_original() {
        let base = PathBuf::from("/a");
        let result = base.append("b");
        // original unchanged
        assert_eq!(base, PathBuf::from("/a"));
        assert_eq!(result, PathBuf::from("/a/b"));
    }

    // ── with_added_extension ──

    #[test]
    fn added_extension_when_none_exists() {
        let p = PathBuf::from("/tmp/myfile");
        assert_eq!(
            p.with_added_extension("sha512"),
            PathBuf::from("/tmp/myfile.sha512")
        );
    }

    #[test]
    fn added_extension_when_one_exists() {
        let p = PathBuf::from("/tmp/image.ext4");
        assert_eq!(
            p.with_added_extension("sha512"),
            PathBuf::from("/tmp/image.ext4.sha512")
        );
    }

    #[test]
    fn added_extension_when_multiple_exist() {
        let p = PathBuf::from("/tmp/archive.tar.gz");
        // existing extension is "gz", so we get "tar.gz.bak"
        assert_eq!(
            p.with_added_extension("bak"),
            PathBuf::from("/tmp/archive.tar.gz.bak")
        );
    }
}
