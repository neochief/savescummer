fn main() {
    // The version resource reads [package.metadata.winresource] from this
    // manifest; without an explicit rerun path a metadata edit would not
    // regenerate it (winresource does not emit one itself).
    println!("cargo:rerun-if-changed=Cargo.toml");
    let icon = std::path::Path::new("../../assets/icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    if std::env::var_os("CARGO_CFG_TARGET_OS").as_deref() == Some(std::ffi::OsStr::new("windows")) {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon_with_id(icon.to_str().expect("UTF-8 icon path"), "1")
            .set("CompanyName", "SaveScummer contributors")
            .compile()
            .expect("compile Windows application resources");
    }
}
