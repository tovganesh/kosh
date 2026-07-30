use std::path::PathBuf;

fn main() {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    println!("cargo:rustc-link-arg=-T{}", dir.join("user.ld").display());
    println!("cargo:rerun-if-changed=user.ld");
    println!("cargo:rerun-if-changed=build.rs");
}
