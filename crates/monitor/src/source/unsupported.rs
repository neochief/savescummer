//! No process list on this OS yet (PLAN-MACOS.md, PROCESS MONITORING): no
//! game is ever seen running.

use crate::{Proc, ProcessSource};

struct Unsupported;

impl ProcessSource for Unsupported {
    fn list(&mut self) -> Vec<Proc> {
        Vec::new()
    }

    fn foreground(&mut self) -> Option<u32> {
        None
    }
}

pub fn system_source() -> Box<dyn ProcessSource> {
    Box::new(Unsupported)
}
