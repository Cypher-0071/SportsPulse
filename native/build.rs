fn main() {
    println!("cargo:rerun-if-changed=assets/icon.res");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    // Raw `.res` via `rustc-link-arg` only works with the MSVC linker; GNU
    // needs windres preprocessing, so gate on both.
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os == "windows" && target_env == "msvc" {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let res_path = std::path::Path::new(&manifest_dir)
            .join("assets")
            .join("icon.res");
        if res_path.exists() {
            println!("cargo:rustc-link-arg={}", res_path.display());
        } else {
            println!("cargo:warning=assets/icon.res not found, skipping icon embed");
        }
    }
}
