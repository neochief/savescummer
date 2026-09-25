//! A stand-in for the UI (PLAN-HOST, PROCESSES), so tests can see when the
//! host starts a UI and when it asks the open one to come to the front.
//!
//! It connects to the host of `--data-dir` the way the UI does (a focus
//! report, then a watch) and appends one line per event to the file named
//! by `FAKE_UI_LOG`: `started`, `connected`, `show`. It exits when the host
//! shuts down or goes away.

use std::io::Write;
use std::path::{Path, PathBuf};

use savescummer_ipc::{Client, Command, EventBody};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let data_dir = args.windows(2).find(|w| w[0] == "--data-dir").map(|w| PathBuf::from(&w[1])).expect("--data-dir");
    let log = PathBuf::from(std::env::var_os("FAKE_UI_LOG").expect("FAKE_UI_LOG"));
    append(&log, "started");
    let mut client = Client::connect(&savescummer_ipc::endpoint(&data_dir)).expect("connect to the host");
    client.request(None, Command::UiReport { focused: false, visible: true, selected: None }).expect("report");
    client.request(None, Command::Watch).expect("watch");
    append(&log, "connected");
    while let Ok(Some(event)) = client.next_event() {
        match event.body {
            EventBody::ShowWindow => append(&log, "show"),
            EventBody::Shutdown => break,
            _ => {}
        }
    }
}

fn append(log: &Path, line: &str) {
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(log).expect("the log");
    writeln!(file, "{line}").expect("write the log");
}
