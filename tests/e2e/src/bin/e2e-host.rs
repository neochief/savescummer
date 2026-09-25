// A GUI program: Windows never gives it a console window (PLAN-HOST, PROCESSES).
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    savescummer_host::main()
}
