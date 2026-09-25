//! Windows version resource and icon (PLAN-BUILD.md VERSION): file
//! properties show the Cargo version, which winresource reads itself.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("../../assets/icon.ico")
            .set("ProductName", "SaveScummer")
            .set("FileDescription", "SaveScummer command-line client")
            .set("InternalName", "SaveScummer.CLI")
            .set("OriginalFilename", "SaveScummer.CLI.exe")
            .compile()
            .expect("compiling the Windows resources");
    }
}
