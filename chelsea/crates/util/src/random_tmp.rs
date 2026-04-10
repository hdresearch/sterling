use rand::Rng;
use std::{env, fs, io, path::PathBuf, process};

pub fn random_tmp_dir<P: AsRef<str>>(prefix: P) -> io::Result<PathBuf> {
    let mut rng = rand::rng();
    let safe_prefix = prefix.as_ref().replace(['/', '\\'], "_");

    loop {
        let suffix: u64 = rng.random();

        let dir =
            env::temp_dir().join(format!("{}_{}_{:016x}", safe_prefix, process::id(), suffix));

        match fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(ref e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}

pub fn random_tmp_file<P: AsRef<str>, Q: AsRef<str>>(
    prefix: P,
    file_name_only: Q,
) -> io::Result<PathBuf> {
    let dir = random_tmp_dir(prefix)?;
    let safe_file_name_only = file_name_only.as_ref().replace(['/', '\\'], "_");
    Ok(dir.join(safe_file_name_only))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_tmp_dir_creates_directory() {
        let dir = random_tmp_dir("test").unwrap();
        assert!(dir.is_dir());
        fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn random_tmp_dir_unique_paths() {
        let a = random_tmp_dir("test").unwrap();
        let b = random_tmp_dir("test").unwrap();
        assert_ne!(a, b);
        fs::remove_dir(&a).unwrap();
        fs::remove_dir(&b).unwrap();
    }

    #[test]
    fn random_tmp_dir_sanitizes_slashes() {
        let dir = random_tmp_dir("my/prefix\\here").unwrap();
        let name = dir.file_name().unwrap().to_string_lossy();
        assert!(!name.contains('/'));
        assert!(!name.contains('\\'));
        assert!(name.contains("my_prefix_here"));
        fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn random_tmp_file_returns_path_inside_created_dir() {
        let path = random_tmp_file("test", "myfile.txt").unwrap();
        assert_eq!(path.file_name().unwrap(), "myfile.txt");
        // The parent directory should exist
        assert!(path.parent().unwrap().is_dir());
        fs::remove_dir(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn random_tmp_file_sanitizes_filename() {
        let path = random_tmp_file("test", "some/bad\\name.txt").unwrap();
        let name = path.file_name().unwrap().to_string_lossy();
        assert_eq!(name, "some_bad_name.txt");
        fs::remove_dir(path.parent().unwrap()).unwrap();
    }
}
