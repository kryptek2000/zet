fn main() {
    #[cfg(feature = "ghostty-vt")]
    {
        let lib_dir = std::env::var("GHOSTTY_VT_LIB_DIR").unwrap_or_else(|_| {
            "/home/bandit/zet-ghostty-a2/ghostty-0fa3d577a5614150ff8e2c152de5fb9d5c6d2dec/zig-out/lib"
                .to_string()
        });
        println!("cargo:rustc-link-search=native={lib_dir}");
        println!("cargo:rustc-link-lib=dylib=ghostty-vt");
        println!("cargo:rustc-link-arg=-Wl,-rpath,{lib_dir}");
    }
}
