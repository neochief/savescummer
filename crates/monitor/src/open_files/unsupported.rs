//! No open-file inspection on this OS for now: on Windows a handle opened
//! without delete sharing makes Load stage 2 fail instead.

use super::{FileId, Unreadable};

pub const SUPPORTED: bool = false;

pub struct Open;

pub fn open_files(_pid: u32) -> Result<Vec<Open>, Unreadable> {
    Err(Unreadable::Refused("not supported on this OS".into()))
}

pub fn same(_open: &Open, _file: &FileId) -> bool {
    false
}
