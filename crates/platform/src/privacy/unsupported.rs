//! No OS-guarded locations here.

use std::io;
use std::path::Path;

use super::Table;

pub fn table() -> Table {
    Table::default()
}

pub fn is_mount_point(_path: &Path) -> bool {
    false
}

pub fn is_privacy_refusal(_e: &io::Error) -> bool {
    false
}

pub fn code_identity() -> Option<String> {
    None
}
