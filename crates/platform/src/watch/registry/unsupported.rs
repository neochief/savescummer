//! No registry on this OS: installs are found through store folders only.

use std::sync::mpsc::Sender;

use super::{Msg, RegistryKey};

pub struct KeyWatcher;

impl KeyWatcher {
    pub fn new(changed: Sender<Msg>) -> Option<KeyWatcher> {
        let _ = changed;
        Some(KeyWatcher)
    }

    pub fn set_keys(&self, keys: Vec<RegistryKey>) {
        let _ = keys;
    }
}
