#![cfg_attr(windows, windows_subsystem = "windows")]
use clap::Parser;
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    savescummer_platform::desktop::attach_parent_console(
        !std::env::args_os().any(|arg| arg == "--minimized"),
    );
    savescummer_host::serve(savescummer_host::HostOptions::parse()).await
}
