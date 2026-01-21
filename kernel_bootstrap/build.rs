fn main() {
    // Linker configuration for bare-metal binary
    // Only applies to the binary target, not the library or tests
    #[cfg(not(test))]
    {
        // Check if we're building for the bare-metal target
        let target = std::env::var("TARGET").unwrap_or_default();
        if target == "x86_64-unknown-none" {
            println!("cargo:rustc-link-arg=-nostdlib");
            println!("cargo:rustc-link-arg=-static");
            println!("cargo:rerun-if-changed=linker.ld");
        }
    }
}
