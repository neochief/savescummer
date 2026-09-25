//! macOS and Linux: no registry, so no uninstall entries (the tests that
//! need them are Windows only), and folder links are symbolic links.

/// No registry off Windows: uninstall entries are never seen there, so the
/// tests that rely on them are Windows-only.
pub mod registry {
    pub struct Scratch {
        pub path: String,
    }

    impl Scratch {
        pub fn new() -> Scratch {
            Scratch { path: String::new() }
        }
    }

    pub fn set_value(_path: &str, _name: &str, _value: &str) {}

    pub fn delete_tree(_path: &str) {}
}

/// Processes are cleaned up by each test's own guards.
pub fn kill_children_on_exit() {}

/// A folder link: a symbolic link.
pub fn link_dir(link: &std::path::Path, target: &std::path::Path) {
    std::os::unix::fs::symlink(target, link).expect("symlink");
}

/// Removes a folder link made by [`link_dir`], not what it points at.
pub fn unlink_dir(link: &std::path::Path) {
    std::fs::remove_file(link).unwrap();
}
