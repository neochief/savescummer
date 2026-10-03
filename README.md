# SaveScummer

[![Release](https://github.com/neochief/savescummer/actions/workflows/release.yml/badge.svg)](https://github.com/neochief/savescummer/actions/workflows/release.yml)

SaveScummer keeps checkpoints of a game's on-disk saves so you can return to earlier progress. It is useful in games that do not offer saving progression on demand or present it in a way that is inconvenient for your play style.

It is not a cheat engine; it does not modify the game or its memory.

## How to use it

1. Open SaveScummer and select a detected game (about 100 games are already supported out of the box).

    Use **Scan for games** or **Add custom game** if it is missing; you may need to point SaveScummer to the game's save files if the game is not supported.

2. Play, then **quit the game normally** so it writes its progress to disk.

3. In SaveScummer, press **Save** to make a checkpoint. Return to the game and keep playing.

4. Later on, if you want to return to that checkpoint, quit the game, and in SaveScummer, press **Load** on that checkpoint in history (or main Load button if that was the most recent checkpoint). Then launch the game again.

Saving and loading while a game is running is disabled by default. Many games keep progress in memory, so changing their files mid-game may not have the intended effect. A per-game **Expert mode** allows it for games you have confirmed can handle it, but use it at your own risk. SaveScummer does not guarantee that a game will work correctly after loading a checkpoint.

## Install

SaveScummer supports Windows, macOS and Linux. Download the package for your platform from [Releases](https://github.com/neochief/savescummer/releases):

| Platform | File |
| --- | --- |
| Windows 10/11 x64 | `SaveScummer-windows-x64-<version>-setup.exe` |
| macOS 13+, Apple Silicon | `SaveScummer-macos-arm64-<version>.dmg` |
| Linux x86_64, glibc 2.35+ | `SaveScummer-linux-x86_64-<version>.AppImage` |
| Linux ARM64, glibc 2.35+ | `SaveScummer-linux-aarch64-<version>.AppImage` |

On Windows, run the installer. It installs for your user without admin access.

On macOS, drag SaveScummer from the disk image to Applications. SaveScummer lives in the menu bar and may ask for access to a game's save folder.

On Linux, make the AppImage executable (`chmod +x SaveScummer-linux-*.AppImage`) and run it. The AppImage includes the host, UI and CLI; no installation or root access is needed.

## Your checkpoints

Checkpoints are stored in the `checkpoints` folder under SaveScummer's app data directory unless you move the checkpoint store in Settings:

| Platform | App data directory |
| --- | --- |
| Windows | `%LOCALAPPDATA%\SaveScummer` |
| macOS | `~/Library/Application Support/SaveScummer` |
| Linux | `~/.local/share/SaveScummer` |

Installing, upgrading, or uninstalling SaveScummer does not delete your checkpoints.

## Develop

Install [Rust through rustup](https://rustup.rs), Git, Node.js 22, and pnpm 12. Windows also needs Visual Studio 2022 or its Build Tools with **Desktop development with C++**. macOS needs the Xcode command-line tools and `rsvg-convert` (`brew install librsvg`). See [building and packaging](docs/building.md) for the full setup and checks.

```bash
cargo xtask setup cargo-about
cargo xtask run
```

`run` builds and opens the app with separate development data in `.runtime/dev`. The host stays running afterward; stop it with `cargo xtask host stop`. Use `cargo xtask test` for the full format, lint, Rust and UI tests, build, and catalog checks; `test crates`, `test e2e` and `test ui` run development sections. Use `cargo xtask dist` to package a release for this platform.

### Linux setup

On Ubuntu 22.04 or newer, install the build dependencies, then [Rust](https://rustup.rs) and Node.js 22 with pnpm 12 through Corepack. Before `cargo xtask run`, install the pinned packaging tools:

```bash
sudo apt install build-essential pkg-config curl git file libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libsoup-3.0-dev patchelf gstreamer1.0-plugins-base gstreamer1.0-plugins-good
cargo xtask setup linux-tools
cargo xtask setup cargo-about
```

## CLI

The included CLI drives the same app host. Save and Load still require the game to be closed by default. For a development build, use the game ID shown by `games` (for example, `steam-212680`):

```bash
./target/debug/savescummer-cli --data-dir .runtime/dev status
./target/debug/savescummer-cli --data-dir .runtime/dev games
./target/debug/savescummer-cli --data-dir .runtime/dev save <game>
./target/debug/savescummer-cli --data-dir .runtime/dev history <game>
./target/debug/savescummer-cli --data-dir .runtime/dev load <game>
```

`load` uses the latest saved checkpoint by default. Use `--help` for more commands and options.
