use std::{env::VarError, process::Command};

fn main() {
    let git_hash = get_git_hash();
    println!("cargo:rustc-env=GIT_HASH={}", git_hash.trim());
}

fn get_git_hash() -> String {
    let err = match std::env::var("VERS_GIT_HASH") {
        Ok(value) => return value,
        Err(err) => err,
    };

    match err {
        VarError::NotUnicode(_) => panic!("var 'VERS_GIT_HASH' not unicode"),
        VarError::NotPresent => {
            let output = Command::new("git")
                .args(&["rev-parse", "HEAD"])
                .output()
                .expect("Failed to execute git command");
            println!("cargo:rerun-if-changed=.git/HEAD");

            String::from_utf8(output.stdout).unwrap()
        }
    }
}
