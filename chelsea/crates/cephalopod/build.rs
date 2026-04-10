fn main() {
    println!("cargo:rustc-link-lib=rados");
    println!("cargo:rustc-link-lib=rbd");
}
