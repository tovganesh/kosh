fn main() {
    // Use our custom linker script
    println!("cargo:rustc-link-arg=-Tkernel/linker.ld");
    
    // Rerun if the linker script changes
    println!("cargo:rerun-if-changed=linker.ld");
}