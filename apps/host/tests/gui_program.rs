//! The host is a GUI program (PLAN-HOST, PROCESSES): Windows never gives it
//! a console window, and its output reaches only the log and callers that
//! captured it.

use std::process::{Command, Stdio};

const HOST: &str = env!("CARGO_BIN_EXE_savescummer-host");

/// The subsystem field of a PE executable's optional header.
#[cfg(windows)]
fn subsystem(exe: &str) -> u16 {
    let bytes = std::fs::read(exe).unwrap();
    let pe = u32::from_le_bytes(bytes[0x3c..0x40].try_into().unwrap()) as usize;
    assert_eq!(&bytes[pe..pe + 4], b"PE\0\0");
    // Signature (4) + file header (20) + subsystem's offset in the optional header (68).
    u16::from_le_bytes(bytes[pe + 24 + 68..pe + 24 + 70].try_into().unwrap())
}

#[test]
#[cfg(windows)]
fn the_host_is_a_windows_gui_program() {
    const IMAGE_SUBSYSTEM_WINDOWS_GUI: u16 = 2;
    assert_eq!(subsystem(HOST), IMAGE_SUBSYSTEM_WINDOWS_GUI);
}

#[test]
fn captured_output_still_arrives() {
    let out = Command::new(HOST).arg("--version").stdin(Stdio::null()).output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn a_bad_option_is_in_the_log_and_the_ready_line() {
    let data = tempfile::tempdir().unwrap();
    let out = Command::new(HOST)
        .args(["--data-dir", data.path().to_str().unwrap(), "--no-such-option"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let line = String::from_utf8_lossy(&out.stdout);
    assert!(line.contains("\"ready\":false") && line.contains("no-such-option"), "{line}");
    let log = std::fs::read_to_string(data.path().join("host.log")).unwrap();
    assert!(log.contains("no-such-option"), "{log}");
}
