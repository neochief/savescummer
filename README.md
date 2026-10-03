# SaveScummer

[![CI](https://github.com/neochief/savescummer/actions/workflows/ci.yml/badge.svg)](https://github.com/neochief/savescummer/actions/workflows/ci.yml)

SaveScummer helps you save and restore progress in games where that isn't possible by design. It helps you learn difficult games faster and spend less time replaying what you already know. Roguelikes, permadeath, Ironman modes — experience them with less pain and more fun. Checkpoint before risky moments, experiment, fail, learn, and keep going.

If your time is limited, it helps you reach interesting stories, builds, and decisions without losing hours of progress before you _got gud_.

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
| Linux ARM64 (glibc 2.35+), experimental | `SaveScummer-linux-aarch64-<version>.AppImage` (coming) |

Get the Windows installer or macOS disk image from [Releases](https://github.com/neochief/savescummer/releases). Linux AppImages will be published once they pass on CI. Windows is unsigned; macOS uses an ad hoc signature, so each OS may ask before first launch:

- **Windows:** run the installer. SmartScreen may say "Windows protected your PC": choose **More info → Run anyway**. It installs for your user only (no admin), into `%LOCALAPPDATA%\Programs\SaveScummer`, and offers to launch at sign-in.
  - **Upgrade:** run the new installer; it closes the running app safely and keeps your settings.
  - **Remove:** *Settings → Apps → SaveScummer → Uninstall*.
- **macOS:** open the DMG and drag SaveScummer to Applications. The first launch is blocked: open *System Settings → Privacy & Security* and choose **Open Anyway**. It lives in the menu bar; open the app from Finder, Launchpad or Spotlight (the programs inside the bundle aren't meant to be double-clicked).
  - **Hotkeys:** **⌥F5** saves and **⌥F9** loads (Ctrl can't be used: macOS keeps ⌃F5 for itself). On most Mac keyboards the top row is brightness and media keys, so press **fn+⌥+F5**, unless *Keyboard settings → Use F1, F2, etc. keys as standard function keys* is on. Careful with ⌘F5 next to it: it turns VoiceOver on (press it again to turn it off).
  - **Permissions:** macOS asks before an app reads some places: Documents, Desktop, Downloads, iCloud Drive, external disks, other apps' data. SaveScummer asks only when you do something (the first launch, Scan for games, adding a game, *Allow access*); a game whose saves are somewhere it may not read yet waits, marked in the app, and you get one notification per place. If you chose *Don't Allow*, turn it on in *System Settings → Privacy & Security*.
  - **Upgrade:** quit it (menu-bar icon → Exit) and drag the new app over the old one. The app isn't signed with a Developer ID, so macOS treats each version as a new app: allow access again, once, when it asks.
  - **Remove:** turn off launch at login (*System Settings → General → Login Items*, under *Allow in the Background*), quit, drag it to the Trash.
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

You need [rustup](https://rustup.rs) and Git. On Windows, also Visual Studio 2022+ (or its Build Tools) with "Desktop development with C++"; on macOS, the Xcode command-line tools; on Linux, the packages in [Linux setup](#linux-setup). rustup installs the pinned Rust version on first use.

```bash
git clone https://github.com/neochief/savescummer.git
cd savescummer
cargo xtask check
```

`check` runs Rust format, clippy and tests plus the catalog gate. To build and test the full installer or disk image, run `cargo xtask dist` on its platform.

### Run it

The host runs in the Windows tray or macOS menu bar and opens the Tauri UI; the CLI is also included. Development uses its own data in `.runtime/dev`, or disposable `.runtime/dev-demo` for `run --demo`, never your real checkpoints. Install Node.js 22 and pnpm 12 to build the UI on every platform.

The dev package runs like the installed app, with the UI and hotkeys (Ctrl+F5 / Ctrl+F9 on Windows; ⌥F5 / ⌥F9 on macOS). macOS packaging also needs `rsvg-convert` for the app icon (`brew install librsvg`).

```bash
cargo xtask setup cargo-about
cargo xtask run
```

The dev host keeps running after `run` returns; `cargo xtask host stop` stops it.

On Linux, follow [Linux setup](#linux-setup) first; it includes `setup linux-tools`.

Without the UI, build and use the binaries directly. The CLI starts the host itself when it isn't running:

```bash
cargo build
./target/debug/savescummer-cli --data-dir .runtime/dev status
```

### Linux setup

Everything a clean Ubuntu 22.04 or newer needs before `cargo xtask run`. Other distros need the same packages under their own names.

The build tools and the WebKitGTK and GTK libraries Tauri builds against:

```bash
sudo apt install build-essential pkg-config curl git file libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libsoup-3.0-dev patchelf gstreamer1.0-plugins-base gstreamer1.0-plugins-good
```

Rust through rustup, not apt or snap, so `rust-toolchain.toml` picks the version:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Node.js 22 through [nvm](https://github.com/nvm-sh/nvm), since Ubuntu's own Node is older:

```bash
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.3/install.sh | bash
```

Open a new terminal so `cargo` and `nvm` are on `PATH`, or run `source ~/.cargo/env` and `source ~/.nvm/nvm.sh` in this one. Then install Node and let Corepack provide the pnpm version `apps/ui/package.json` pins:

```bash
nvm install 22
corepack enable
```

Fetch that pnpm once, answering yes when Corepack asks to download it:

```bash
(cd apps/ui && pnpm --version)
```

From the repo, the pinned packaging tools, once:

```bash
cargo xtask setup linux-tools
cargo xtask setup cargo-about
```

`cargo xtask run` now builds and starts the app. If a shell says `cargo` or `pnpm` isn't found, it started before the installers changed `~/.bashrc`: open a new terminal or `source` the files above.

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
