fn main() {
    #[cfg(not(test))]
    {
        println!("cargo:rustc-link-arg=-nostdlib");
        println!("cargo:rustc-link-arg=-static");
        println!("cargo:rerun-if-changed=linker.ld");
    }
}
