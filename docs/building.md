# Building SaveScummer

All build automation is `cargo xtask`: one Rust program that runs the same on every OS, locally and in CI. There are no PowerShell, bash or Python build scripts. The design and the reasons behind it are in [PLAN-BUILD.md](../PLAN-BUILD.md); this guide is the how-to.

Windows builds and ships a complete installer with the Tauri UI. macOS host/CLI packaging is implemented but its UI is not yet included in the release bundle, so macOS is tested in CI but not published. Linux packaging isn't yet implemented.


## Prerequisites

Every platform:

- **Rust via rustup.** `rust-toolchain.toml` pins the version; rustup installs it on first use.
- **Git.**
- **`gh`** (the GitHub CLI), only for `cargo xtask publish`.

Windows:

- **Visual Studio 2022+ or its Build Tools** with "Desktop development with C++". Rust links with it, and the Visual C++ runtime DLLs shipped in the package come from it.
- **Node.js 22 and pnpm 10.11.0** on `PATH` to build and test the Tauri UI. `apps/ui/package.json` pins pnpm; CI installs both tools.

macOS (13+, Apple Silicon only):

- **Xcode command-line tools** (`xcode-select --install`): the linker, `codesign`, `iconutil` and `hdiutil`.
- **rsvg-convert** (`brew install librsvg`), which renders the app icon from `assets/icon.svg` for every package.
- Every Rust build targets the oldest supported macOS: `.cargo/config.toml` sets `MACOSX_DEPLOYMENT_TARGET`, kept equal to `pins::MIN_MACOS` by a test.
- The end-to-end tests that switch between windows need an unlocked desktop session; while the screen is locked they say so and pass.

The old Qt frontend setup is dormant because this tree has no Qt UI project. It is not needed for the current Windows or macOS build:

- **CMake 3.21+** on `PATH` (Visual Studio, Xcode command-line tools or the distro provide it).
- **Python 3.9+**, only to install Qt.
- **Qt**, installed with `cargo xtask setup qt`.

Pinned packaging tools are installed into `.runtime/` by their setup commands. Windows UI builds also need Node.js and pnpm on `PATH`; CI sets these up before running xtask. Missing packaging tools name the setup command to run:

| Command | Installs | Needed for |
| --- | --- | --- |
| `cargo xtask setup cargo-about` | cargo-about (`THIRD-PARTY-LICENSES.html`) | any package: `build --package`, `run`, `dist` |
| `cargo xtask setup inno` | Inno Setup, portable, into `.runtime/tools/inno-setup/` | `dist` on Windows |
| `cargo xtask setup qt` | the Qt kit into `.runtime/Qt/<version>/<kit>/` | a Qt UI build |
| `cargo xtask setup linux-tools` | linuxdeploy and appimagetool | `dist` on Linux (not yet) |

Packaging tool versions and checksums are pinned in [`xtask/src/pins.rs`](../xtask/src/pins.rs). Node.js is selected by CI, and pnpm is pinned in `apps/ui/package.json`; frontend dependencies are locked in `apps/ui/pnpm-lock.yaml`. Setup commands are safe to re-run.

A first Windows setup:

```bash
cargo xtask setup cargo-about
```

```bash
cargo xtask setup inno
```


## Commands

| Command | What it does |
| --- | --- |
| `cargo xtask check` | The quality gate: `cargo fmt --check`, clippy with `-D warnings`, serial tests (some need desktop focus), build, `catalog --check`. Stops at the first failure. |
| `cargo xtask build` | Dev build of the host, CLI and Windows Tauri UI. |
| `cargo xtask build --test` | Also runs the Rust tests and the UI's headless tests. |
| `cargo xtask build --package` | Also assembles the APP PACKAGE under `build/dev/package/`. |
| `cargo xtask build --release` | Optimized, stripped release build; always packages, into `build/release/package/`. |
| `cargo xtask run [--demo] [--stop-other-hosts]` | Dev package, then the dev host, started like a user launch, so it shows the UI. |
| `cargo xtask host start [--demo]` | Dev package, then just the dev host, in the tray without the UI. |
| `cargo xtask host stop` | Stops the dev host gracefully. |
| `cargo xtask dist` | `build --release --test`, then the release file in `dist/`. |
| `cargo xtask clean [--deep]` | Stops anything running from `target/`, `build/` or `dist/`, removes `build/` and `dist/`; `--deep` also removes `target/`. Never touches `.runtime/`. |
| `cargo xtask release <version>` | Cuts a release (see RELEASING). |
| `cargo xtask publish` | Uploads `dist/` to a draft GitHub release. |
| `cargo xtask catalog [--check] [--strict]` | Regenerates the game catalog (see [PLAN-CATALOG.md](../PLAN-CATALOG.md)). |

Dev builds may refresh `Cargo.lock`; release builds, `check` and packaging use `--locked`.

xtask honors `CARGO_TARGET_DIR` like Cargo does.


## Where things go

| Folder | Holds | Removed by |
| --- | --- | --- |
| `target/` | Cargo's cache | `clean --deep` |
| `build/` | everything regenerable: packages, UI build trees, logs, `session.json` | `clean` |
| `dist/` | the release file and nothing else | `clean`, and emptied by every `dist` |
| `.runtime/` | pinned tools, any Qt SDK, dev data | never |

The APP PACKAGE is the runnable app, exactly what the release file wraps:

```text
build/<mode>/package/SaveScummer-windows-x64/
  bin/SaveScummer.exe             host: the app, what the Start menu runs
  bin/SaveScummer.UI.exe          embedded Tauri/WebView2 UI
  bin/SaveScummer.CLI.exe         CLI
  bin/vcruntime140.dll …          Visual C++ runtime
  bin/*.pdb                       dev only
  README.txt
  THIRD-PARTY-LICENSES.html
  WEB-THIRD-PARTY-LICENSES.html
  .savescummer-package.json       mode, version, platform, UI toolkit, configuration, creation time
  SHA256SUMS.txt
```

On macOS the package is the app bundle itself:

```text
build/<mode>/package/SaveScummer.app/
  Contents/Info.plist             LSUIElement: menu bar only, no Dock icon
  Contents/MacOS/SaveScummer      host: the bundle's executable, what opening the app runs
  Contents/MacOS/SaveScummer.UI   UI (once it exists)
  Contents/MacOS/SaveScummer.CLI  CLI
  Contents/Library/LaunchAgents/com.savescummer.SaveScummer.host.plist   launch at login
  Contents/Resources/SaveScummer.icns, licenses, manifest, SHA256SUMS.txt
```

It's signed ad hoc (`codesign --sign -`) as the last step and verified. The main executable and `_CodeSignature` are left out of `SHA256SUMS.txt`: the signature covers them, and they change whenever the bundle is signed.

Cargo builds `savescummer-host` and `savescummer-cli` (Cargo names can't contain dots); packaging renames them to the fixed names above (`SaveScummer` and `SaveScummer.CLI`).

A package is assembled in a `.staging-<uuid>` folder and swapped in only when complete, after stopping anything running from the old one, so a failed build never breaks the existing package.


## Licenses

`THIRD-PARTY-LICENSES.html` lists every crate the host, CLI and Windows UI link, generated by cargo-about. `WEB-THIRD-PARTY-LICENSES.html` lists production JavaScript dependencies and their license texts. [`about.toml`](../about.toml) lists the accepted Rust licenses: a dependency under any other license fails packaging, so adding a license is a deliberate edit there. The app's own crates are `publish = false` and are left out.

Qt license texts from `packaging/licenses/` ship with a Qt frontend build.


## Dev session

`cargo xtask run` and `host start`:

1. Build the dev package.
2. Start its host with `--data-dir .runtime/dev`, logging to `build/dev/logs/host.log`. `host start` adds `--minimized`; `run` doesn't, so the host shows the UI itself, as it does for a user.
3. Wait for its `"ready":true` line and record it in `build/dev/session.json`.

On Windows, `run` opens the Tauri UI from the dev package.

The dev host uses its own data, so development never touches the real app's data. It keeps running after `run` returns; `cargo xtask host stop` stops it (only if the recorded pid, start time and path all still match). Any build stops it too.

If the installed app's host is running, it keeps the tray icon and global shortcuts, and xtask says so. `--stop-other-hosts` shuts it down gracefully first.

Dev builds never create a launch-at-sign-in entry: only release builds (`SAVESCUMMER_RELEASE_BUILD=1`, set by `build --release`) may.

Data locations:

| | App data | Dev data |
| --- | --- | --- |
| Windows | `%LOCALAPPDATA%\SaveScummer` | `.runtime\dev` |
| macOS | `~/Library/Application Support/SaveScummer` | `.runtime/dev` |
| Linux | `~/.local/share/SaveScummer` | `.runtime/dev` |


## Stopping processes

Every build (so also `run`, `host start` and `dist`), `check` and `clean` first stops everything running from `target/`, `build/` and `dist/`: hosts and CLIs started from `target/`, the dev host, packaged copies, UIs, test binaries. A rebuild is always clean: no locked executable ("access denied") and no host left serving old code. Only xtask itself and Cargo's build scripts are exempt; nothing from `.runtime/` or an installed copy is ever stopped. It's always done the same way:

1. Ask UIs to close.
2. Ask hosts to shut down through the CLI next to them (`--no-start [--data-dir <dir>] shutdown`), waiting up to 30 s so an accepted operation finishes.
3. Terminate whatever is still running.


## Windows installer

`cargo xtask dist` compiles [`packaging/windows/savescummer.iss`](../packaging/windows/savescummer.iss) with Inno Setup into `dist/SaveScummer-windows-x64-<version>-setup.exe`. The payload is exactly the release package.

- Per-user install into `%LOCALAPPDATA%\Programs\SaveScummer`, no admin prompt; upgrades replace in place (stable `AppId`).
- "Launch at sign-in" task: checked on first install, the user's choice kept on upgrade. It runs `SaveScummer.exe --autostart on|off`, so the host stays the only writer of the sign-in entry. Uninstalling runs `--autostart off`.
- Before replacing or removing files, it runs the installed `SaveScummer.CLI.exe --no-start shutdown`, so an in-flight save finishes.
- If WebView2 is absent, the installer downloads and installs Microsoft's Evergreen Runtime before installing the app. This requires an internet connection on those machines.
- The Start menu entry and the last page's "Launch SaveScummer" run `SaveScummer.exe`: the host starts, or the running one is reached, and shows the UI.
- User data (`%LOCALAPPDATA%\SaveScummer` and a moved checkpoint store) is never touched.
- Unsigned: SmartScreen shows "More info → Run anyway".

Version resources: `apps/host/build.rs` and `apps/cli/build.rs` embed the Cargo version and `assets/icon.ico` (winresource).


## macOS disk image

`cargo xtask dist` puts the release bundle and an `Applications` link into `dist/SaveScummer-macos-arm64-<version>.dmg` (`hdiutil`, UDZO). The user drags the app over; there's no installer.

- Signed ad hoc, not with a Developer ID: the first launch needs *Open Anyway*, and each new build is a new identity to macOS, so it asks again for any privacy permission (PLAN-MACOS.md, SIGNING).
- Launch at login is an `SMAppService` agent whose plist is inside the bundle; macOS lists it as SaveScummer in *System Settings → General → Login Items*, under *Allow in the Background* (not *Open at Login*).
- The version is in `Info.plist`; the `winresource` build scripts do nothing on macOS.


## RELEASING

1. `cargo xtask release 1.2.3` on a clean branch: checks the version is new and the tag free (locally and on origin), writes it into `Cargo.toml`, the Tauri config and the UI package manifest, refreshes `Cargo.lock`, runs `check` (`--skip-checks` for emergencies), commits "Release 1.2.3", tags `v1.2.3` and pushes both. `--no-push` prints the two push commands instead. If anything fails before the commit, the bump is undone.
2. The tag starts `.github/workflows/release.yml`: the Windows job runs `cargo xtask dist` and uploads its installer; a final job runs `cargo xtask publish`. macOS remains in CI until its UI can be packaged.
3. `publish` checks that `gh` is logged in, the tag points at `HEAD`, and `dist/` holds exactly the Windows installer. It creates a draft release (or re-uploads to an existing draft), and refuses if the release is already published.
4. Review the draft on GitHub and publish it by hand.

A published release never changes; fixes ship as a new version. To rebuild a draft from another commit, delete the tag locally and on origin and push it again.

`publish` also works locally: run `cargo xtask dist` on the tagged commit, then `cargo xtask publish`.


## CI

- [`ci.yml`](../.github/workflows/ci.yml): branch pushes, pull requests and manual runs. Windows installs Node.js and pnpm, runs `check`, then `dist` to build and test the full Tauri UI package and Inno Setup installer. macOS runs `check` and `build --test --package`; it installs Qt only if a Qt UI project is present, plus librsvg for the icon. Superseded runs are cancelled.
- [`release.yml`](../.github/workflows/release.yml): `v*` tags build and draft the Windows installer. Never cancelled.

The Windows pnpm store is cached using `apps/ui/pnpm-lock.yaml`; the pinned packaging tools are cached using `xtask/src/pins.rs`. Run the listed `cargo xtask` commands locally to reproduce build failures.


## VS Code

Tasks (`.vscode/tasks.json`) wrap the commands above: build dev (default build), test (default test), check, start/stop dev host, dist. The launch configurations debug the UI from the dev package against the dev host: they start the host before (in the tray) and stop it after.
