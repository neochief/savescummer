//! Volumes need no special handling here: the OS's file watching holds
//! nothing open, so a removable drive can always be ejected.

use std::path::Path;
use std::sync::mpsc::Sender;

use super::Msg;

pub fn volume_of(path: &Path) -> Option<String> {
    let _ = path;
    None
}

pub struct Volumes {
    remote: Remote,
}

#[derive(Clone, Copy)]
pub struct Remote;

impl Volumes {
    pub fn start(watch: Sender<Msg>) -> Option<Volumes> {
        let _ = watch;
        Some(Volumes { remote: Remote })
    }

    pub fn remote(&self) -> Remote {
        self.remote
    }
}

impl Remote {
    pub fn set_volumes(&self, volumes: Vec<String>) {
        let _ = volumes;
    }

    pub fn simulate(&self, event: u32, volume: &str) {
        let _ = (event, volume);
    }
}
