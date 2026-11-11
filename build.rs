fn main() {
    println!("cargo:rerun-if-changed=native/nvenc_shim.c");
    println!("cargo:rerun-if-changed=native/nvEncodeAPI.h");
    cc::Build::new()
        .file("native/nvenc_shim.c")
        .include("native")
        .opt_level(2)
        .compile("nvenc_shim");
}
