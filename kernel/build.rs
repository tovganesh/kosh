use std::path::PathBuf;

fn main() {
    // Use an absolute path for the linker script. A relative path is resolved
    // against the *linker's* working directory, so `-Tkernel/linker.ld` only
    // worked when cargo happened to be invoked from the workspace root — and
    // failed silently (dropping the multiboot2 header) otherwise.
    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"),
    );
    let linker_script = manifest_dir.join("linker.ld");

    println!("cargo:rustc-link-arg=-T{}", linker_script.display());

    println!("cargo:rerun-if-changed=linker.ld");
    println!("cargo:rerun-if-changed=build.rs");
}
