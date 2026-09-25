//! Command-line options. They never change a rule; they point the host at
//! other folders for development and tests, or turn OS integrations off.

use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Clone, Parser)]
#[command(
    name = "SaveScummer",
    version,
    about = "SaveScummer: the host, which runs in the tray and shows the UI",
    args_override_self = true
)]
pub struct Options {
    /// Start in the tray without showing the UI (the sign-in entry, and
    /// clients that start a host).
    #[arg(long)]
    pub minimized: bool,
    /// Use another data folder (development and tests).
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
    /// Write or remove the sign-in entry, then exit.
    #[arg(long, value_parser = ["on", "off"])]
    pub autostart: Option<String>,
    /// Simulated games and operations, for UI development: a generated
    /// machine in `<data folder>\demo` (or `--data-dir`), wiped at start.
    #[arg(long)]
    pub demo: bool,
    /// Never fetch a newer catalog.
    #[arg(long)]
    pub no_catalog_update: bool,
    /// No hotkeys, tray, sounds or sign-in changes (automated tests).
    #[arg(long)]
    pub no_integrations: bool,

    /// Use this catalog bundle instead of the built-in one (tests).
    #[arg(long, hide = true)]
    pub catalog: Option<PathBuf>,
    /// Fetch catalog updates from this URL instead of the repository (tests).
    #[arg(long, hide = true)]
    pub catalog_url: Option<String>,
    /// Fetch Steam art from this base URL instead of Steam's CDN (tests).
    #[arg(long, hide = true)]
    pub artwork_url: Option<String>,
    /// Read the machine's folders and stores from this file instead of the
    /// OS, so tests never touch real game libraries.
    #[arg(long, hide = true)]
    pub env: Option<PathBuf>,
    /// How often the monitor looks at processes.
    #[arg(long, hide = true, default_value_t = 250)]
    pub poll_ms: u64,
    /// The periodic full scan's interval.
    #[arg(long, hide = true, default_value_t = 900)]
    pub scan_interval_secs: u64,
    /// The minimum time between scans caused by the window gaining focus.
    #[arg(long, hide = true, default_value_t = 20)]
    pub focus_scan_cooldown_secs: u64,
    /// The Delete countdown.
    #[arg(long, hide = true, default_value_t = 5000)]
    pub delete_countdown_ms: u64,
    /// How long Save or Load may make no file progress before it's reported
    /// failed (a read stuck on a permission prompt).
    #[arg(long, hide = true, default_value_t = 30)]
    pub stall_secs: u64,
    /// Watch store locations even with integrations off (tests).
    #[arg(long, hide = true)]
    pub watch: bool,
}
