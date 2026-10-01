fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let res_path = std::path::Path::new(&manifest_dir).join("assets").join("icon.res");
        if res_path.exists() {
            println!("cargo:rustc-link-arg={}", res_path.display());
        }
    }
}
