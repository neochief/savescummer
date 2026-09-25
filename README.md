# SaveScummer

[![CI](https://github.com/neochief/savescummer/actions/workflows/ci.yml/badge.svg)](https://github.com/neochief/savescummer/actions/workflows/ci.yml)

SaveScummer helps you save and restore progress in games where that isn't possible by design. It helps you learn difficult games faster and spend less time replaying what you already know. Roguelikes, permadeath, Ironman modes — experience them with less pain and more fun. Checkpoint before risky moments, experiment, fail, learn, and keep going.

If your time is limited, it helps you reach interesting stories, builds, and decisions without losing hours of progress before you got gud.

If you or your child is an anxious player, it lets you keep playing, experimenting, learning, and having fun without being punished for every mistake.

## How it works

Most games persist progress on disk in one way or another. SaveScummer gives you keyboard shortcuts you can use in-game to checkpoint that progress and restore it later.

Depending on the game, restoring progress may require returning to the main menu or even relaunching the game. It may not be perfectly convenient, but it's still much faster than repeating an evening-long run after one stupid mistake or non-optimal choice.


---

## Install

| Platform | Download |
| --- | --- |
| Windows 10/11 x64 | `SaveScummer-windows-x64-<version>-setup.exe` |
| macOS 13+, Apple Silicon | `SaveScummer-macos-arm64-<version>.dmg` |
| Linux x86_64 (glibc 2.35+) | `SaveScummer-linux-x86_64-<version>.AppImage` (coming) |

Get it from [Releases](https://github.com/neochief/savescummer/releases). The downloads aren't code-signed, so each OS asks once:

- **Windows:** run the installer. SmartScreen may say "Windows protected your PC": choose **More info → Run anyway**. It installs for your user only (no admin), into `%LOCALAPPDATA%\Programs\SaveScummer`, and offers to launch at sign-in.
  - **Upgrade:** run the new installer; it closes the running app safely and keeps your settings.
  - **Remove:** *Settings → Apps → SaveScummer → Uninstall*.
- **macOS:** open the DMG and drag SaveScummer to Applications. The first launch is blocked: open *System Settings → Privacy & Security* and choose **Open Anyway**. It lives in the menu bar; open the app from Finder, Launchpad or Spotlight (the programs inside the bundle aren't meant to be double-clicked).
  - **Hotkeys:** **⌥F5** saves and **⌥F9** loads (Ctrl can't be used: macOS keeps ⌃F5 for itself). On most Mac keyboards the top row is brightness and media keys, so press **fn+⌥+F5**, unless *Keyboard settings → Use F1, F2, etc. keys as standard function keys* is on. Careful with ⌘F5 next to it: it turns VoiceOver on (press it again to turn it off).
  - **Permissions:** macOS asks before an app reads some places: Documents, Desktop, Downloads, iCloud Drive, external disks, other apps' data. SaveScummer asks only when you do something (the first launch, Scan games, adding a game, *Allow access*); a game whose saves are somewhere it may not read yet waits, marked in the app, and you get one notification per place. If you chose *Don't Allow*, turn it on in *System Settings → Privacy & Security*.
  - **Upgrade:** quit it (menu-bar icon → Exit) and drag the new app over the old one. The app isn't signed with a Developer ID, so macOS treats each version as a new app: allow access again, once, when it asks.
  - **Remove:** turn off launch at login, quit, drag it to the Trash.
- **Linux:** make the AppImage executable (`chmod +x SaveScummer-*.AppImage`) and run it. Tools like Gear Lever or AppImageLauncher can add it to your app menu.
  - **Upgrade:** download the new AppImage, quit the old one, start the new one, delete the old file.
  - **Remove:** turn off launch at login, quit, delete the file.

## Your data

Checkpoints are your saves, so installing, upgrading and removing the app never touch them:

- Windows: `%LOCALAPPDATA%\SaveScummer`
- macOS: `~/Library/Application Support/SaveScummer`
- Linux: `~/.local/share/SaveScummer`

Checkpoints go in its `checkpoints` folder unless you move them.

## Development

You need [rustup](https://rustup.rs) and Git. On Windows, also Visual Studio 2022+ (or its Build Tools) with "Desktop development with C++"; on macOS, the Xcode command-line tools. rustup installs the pinned Rust version on first use.

```bash
git clone https://github.com/neochief/savescummer.git
cd savescummer
cargo xtask check
```

`check` is the full quality gate (format, clippy, tests, catalog). If it passes, everything builds.

### Run it

The window (the UI) isn't written yet, so for now SaveScummer is the host, which runs in the tray, plus the CLI to talk to it. Development always uses its own data in `.runtime/dev`, never your real checkpoints.

On Windows and macOS, the dev package runs just like the installed app, tray (menu-bar) icon and hotkeys included (Ctrl+F5 / Ctrl+F9, or ⌥F5 / ⌥F9 on a Mac). On macOS, packaging also needs `rsvg-convert` for the app icon (`brew install librsvg`).

```bash
cargo xtask setup cargo-about
cargo xtask run
```

The dev host keeps running after `run` returns; `cargo xtask host stop` stops it.

On Linux, packaging isn't done yet, so build and use the binaries directly. The CLI starts the host itself when it isn't running:

```bash
cargo build
./target/debug/savescummer-cli --data-dir .runtime/dev status
```

### Drive it

The same commands work on every OS:

```bash
./target/debug/savescummer-cli --data-dir .runtime/dev games
./target/debug/savescummer-cli --data-dir .runtime/dev save <game>
./target/debug/savescummer-cli --data-dir .runtime/dev history <game>
./target/debug/savescummer-cli --data-dir .runtime/dev load <game>
./target/debug/savescummer-cli --data-dir .runtime/dev shutdown
```

`<game>` is the id `games` prints, such as `steam-212680`. `--help` lists the rest, and `--json` gives machine-readable output.

### More

- [docs/building.md](docs/building.md): every `cargo xtask` command, packaging, installers, releases, CI.
- [PLAN.md](PLAN.md): how the app fits together, and the plan for each part.
