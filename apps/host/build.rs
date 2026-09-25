//! Windows version resource, icon and manifest (PLAN-BUILD.md VERSION): file
//! properties show the Cargo version, which winresource reads itself.

/// Per-monitor DPI awareness, so the tray icon and its menu render at the
/// screen's real resolution instead of being bitmap-stretched from 96 DPI.
/// `dpiAware` is the fallback for Windows before 10 1607.
const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2, PerMonitor</dpiAwareness>
    </windowsSettings>
  </application>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>
"#;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("../../assets/icon.ico")
            .set_manifest(MANIFEST)
            .set("ProductName", "SaveScummer")
            .set("FileDescription", "SaveScummer background host")
            .set("InternalName", "SaveScummer")
            .set("OriginalFilename", "SaveScummer.exe")
            .compile()
            .expect("compiling the Windows resources");
    }
}
