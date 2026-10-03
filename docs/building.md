# Building SaveScummer

All build automation is `cargo xtask`: one Rust program that runs the same on every OS, locally and in CI. There are no PowerShell, bash or Python build scripts. The design and the reasons behind it are in [PLAN-BUILD.md](../PLAN-BUILD.md); this guide is the how-to.

Windows, macOS and Linux build the complete host, CLI and Tauri UI. Windows packages an installer, macOS a disk image and Linux an AppImage for x86_64 or aarch64. Linux has also been exercised with real games.


## Prerequisites

Every platform:

- **Rust via rustup.** `rust-toolchain.toml` pins the version; rustup installs it on first use.
- **Git.**
- **`gh`** (the GitHub CLI), only for `cargo xtask publish`.

Windows:

- **Visual Studio 2022+ or its Build Tools** with "Desktop development with C++". Rust links with it, and the Visual C++ runtime DLLs shipped in the package come from it.
- **Node.js 22 and pnpm 12** on `PATH` to build and test the Tauri UI. `apps/ui/package.json` pins the exact pnpm, which Corepack and CI use; xtask accepts another pnpm 12 with a warning.

macOS (13+, Apple Silicon only):

- **Xcode command-line tools** (`xcode-select --install`): the linker, `codesign`, `iconutil` and `hdiutil`.
- **rsvg-convert** (`brew install librsvg`), which renders the app icon from `assets/icon.svg` for every package.
- **Node.js 22 and pnpm 12** on `PATH` for the Tauri UI, as on Windows.
- Every Rust build targets the oldest supported macOS: `.cargo/config.toml` sets `MACOSX_DEPLOYMENT_TARGET`, kept equal to `pins::MIN_MACOS` by a test.
- The end-to-end tests that switch between windows take the desktop's focus, so they run only with `SAVESCUMMER_DESKTOP_TESTS=1` (CI sets it). Without the variable they report a skip and pass; with it, a locked Mac fails the test instead of silently skipping it.

Linux (x86_64 or aarch64):

- **A C toolchain and the WebKitGTK and GTK development packages** Tauri builds against. On Ubuntu or Debian:

  ```bash
  sudo apt install build-essential pkg-config curl git file libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libsoup-3.0-dev patchelf gstreamer1.0-plugins-base gstreamer1.0-plugins-good
  ```

  The README has a short [Linux setup summary](../README.md#linux-setup).

- **Node.js 22 and pnpm 12** on `PATH` for the Tauri UI, as on Windows and macOS.
- `cargo xtask setup linux-tools` before packaging (`run`, `host start`, `dist`); plain builds and tests don't need it.
- Build release AppImages on Ubuntu 22.04 (glibc 2.35, the minimum). One built on a newer system runs only on systems as new as it.
- On Wayland, focus tracking sees only X11 windows (games run through XWayland, so they're seen). Global hotkeys use the desktop's shortcuts portal where there is one (GNOME 48+, KDE Plasma 6), which asks once to allow them, and otherwise see only X11 windows too; see PLAN-HOST.md.

Pinned packaging tools are installed into `.runtime/` by their setup commands. UI builds on every platform also need Node.js and pnpm on `PATH`; CI sets these up before running xtask. Missing packaging tools name the setup command to run:

| Command | Installs | Needed for |
| --- | --- | --- |
| `cargo xtask setup cargo-about` | cargo-about (`THIRD-PARTY-LICENSES.html`) | any package: `run`, `host start`, `dist` |
| `cargo xtask setup inno` | Inno Setup, portable, into `.runtime/tools/inno-setup/` | `dist` on Windows |
| `cargo xtask setup linux-tools` | Tauri's AppImage tools (linuxdeploy, AppRun, the AppImage plugin), appimagetool and the AppImage runtime | any Linux package: `run`, `host start`, `dist` |

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
| `cargo xtask test` | The quality gate: `cargo fmt --check`, clippy with `-D warnings`, serial Rust tests (some need desktop focus), workspace build, `catalog --check`, UI typecheck and tests. Stops at the first failure. |
| `cargo xtask test crates` | Rust workspace package tests, excluding the end-to-end harness. |
| `cargo xtask test e2e` | Host and CLI end-to-end tests, run serially. Focus tests need `SAVESCUMMER_DESKTOP_TESTS=1`. |
| `cargo xtask test ui` | UI typecheck and headless React tests. |
| `cargo xtask build` | Dev build of the host, CLI and Tauri UI on Windows, macOS and Linux. |
| `cargo xtask run [--demo] [--no-integrations] [--stop-other-hosts]` | Dev package, then the dev host, started like a user launch, so it shows the UI. Demo uses disposable `.runtime/dev-demo` data; `--no-integrations` disables tray, hotkeys and sounds. |
| `cargo xtask host start [--demo]` | Dev package, then just the dev host, in the tray without the UI. Demo uses `.runtime/dev-demo`. |
| `cargo xtask host stop` | Stops the dev host gracefully. |
| `cargo xtask dist` | Builds the optimized release package and creates this platform's release file in `dist/`. Run `test` first when preparing a release. |
| `cargo xtask clean [--deep]` | Stops anything running from `target/`, `build/` or `dist/`, removes `build/` and `dist/`; `--deep` also removes `target/`. Never touches `.runtime/`. |
| `cargo xtask release <version>` | Cuts a release (see RELEASING). |
| `cargo xtask publish` | Uploads `dist/` to a draft GitHub release. |
| `cargo xtask catalog [--check] [--strict]` | Regenerates the game catalog (see [PLAN-CATALOG.md](../PLAN-CATALOG.md)). |

Dev builds may refresh `Cargo.lock`; `test` and `dist` use `--locked`.

xtask honors `CARGO_TARGET_DIR` like Cargo does.

For a narrower development run, use Cargo or Vitest directly: `cargo test -p savescummer-host --locked`, `cargo test -p savescummer-e2e --test checkpoints --locked -- --test-threads=1`, or `pnpm exec vitest run src/App.test.tsx` from `apps/ui`. The latter runs only that UI test file; `cargo xtask test ui` also typechecks.

The full command runs all normally enabled tests. Two very large scale tests, audible sound playback, and the manual Windows tray test remain marked `#[ignore]`; Cargo excludes them unless run explicitly.


## Where things go

| Folder | Holds | Removed by |
| --- | --- | --- |
| `target/` | Cargo's cache | `clean --deep` |
| `build/` | everything regenerable: packages, UI build trees, logs, `session.json` | `clean` |
| `dist/` | the release file and nothing else | `clean`, and emptied by every `dist` |
| `.runtime/` | pinned tools, dev data | never |

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
  Contents/MacOS/SaveScummer.UI   embedded Tauri/WebKit UI
  Contents/MacOS/SaveScummer.CLI  CLI
  Contents/Library/LaunchAgents/com.savescummer.SaveScummer.host.plist   launch at login
  Contents/Resources/SaveScummer.icns, Rust and web licenses, manifest, SHA256SUMS.txt
```

Packaging signs and verifies the bundle as its last step. The main executable and `_CodeSignature` are left out of `SHA256SUMS.txt` because signing changes them.

On Linux the package is an AppDir assembled from Tauri's AppImage build:

```text
build/<mode>/package/SaveScummer.AppDir/
  AppRun                         dispatches to the host, UI or CLI
  usr/bin/SaveScummer            host
  usr/bin/SaveScummer.UI         wrapper for bundled GTK and WebKitGTK
  usr/bin/savescummer-ui         Tauri UI
  usr/bin/SaveScummer.CLI        CLI
  usr/share/savescummer/         Rust and web licenses, manifest, SHA256SUMS.txt
```

`cargo xtask dist` wraps it as `dist/SaveScummer-linux-<arch>-<version>.AppImage` on the build machine's architecture. Build release files on Ubuntu 22.04 for the stated glibc minimum. The AppImage uses a pinned type 2 runtime, so it does not require `libfuse2`.

Cargo builds `savescummer-host` and `savescummer-cli` (Cargo names can't contain dots); packaging renames them to the fixed names above (`SaveScummer` and `SaveScummer.CLI`).

A package is assembled in a `.staging-<uuid>` folder and swapped in only when complete, after stopping anything running from the old one, so a failed build never breaks the existing package.


## Licenses

`THIRD-PARTY-LICENSES.html` lists every crate the host, CLI and Tauri UI link, generated by cargo-about. `WEB-THIRD-PARTY-LICENSES.html` lists production JavaScript dependencies and their license texts on every platform. [`about.toml`](../about.toml) lists the accepted Rust licenses: a dependency under any other license fails packaging, so adding a license is a deliberate edit there. The app's own crates are `publish = false` and are left out.

## Dev session

`cargo xtask run` and `host start`:

1. Build the dev package.
2. Start its host with `--data-dir .runtime/dev` (or `.runtime/dev-demo` with `--demo`). On macOS the packaged app is opened through Launch Services, so privacy requests belong to SaveScummer even when RustRover runs the task; its log is `.runtime/dev/host.log` (or `.runtime/dev-demo/data/host.log`). Other platforms log startup output to `build/dev/logs/host.log`. `host start` adds `--minimized`; `run` doesn't, so the host shows the UI itself, as it does for a user.
3. Wait for its `"ready":true` line and record it in `build/dev/session.json`.

On Windows, macOS and Linux, `run` opens the Tauri UI from the dev package.

The dev host uses its own data, so development never touches the real app's data. It keeps running after `run` returns; `cargo xtask host stop` stops it (only if the recorded pid, start time and path all still match). Any build stops it too.

Demo mode resets `.runtime/dev-demo` each time it starts. Its protocol data and CLI socket are in `.runtime/dev-demo/data`.

If the installed app's host is running, it keeps the tray icon and global shortcuts, and xtask says so. `--stop-other-hosts` shuts it down gracefully first.

Dev builds never create a launch-at-sign-in entry: only release builds (`SAVESCUMMER_RELEASE_BUILD=1`, set by `dist`) may.

Data locations:

| | App data | Dev data |
| --- | --- | --- |
| Windows | `%LOCALAPPDATA%\SaveScummer` | `.runtime\dev` |
| macOS | `~/Library/Application Support/SaveScummer` | `.runtime/dev` |
| Linux | `~/.local/share/SaveScummer` | `.runtime/dev` |


## Stopping processes

Every build (so also `run`, `host start` and `dist`), `test` and `clean` first stops everything running from `target/`, `build/` and `dist/`: hosts and CLIs started from `target/`, the dev host, packaged copies, UIs, test binaries. A rebuild is always clean: no locked executable ("access denied") and no host left serving old code. Only xtask itself and Cargo's build scripts are exempt; nothing from `.runtime/` or an installed copy is ever stopped. It's always done the same way:

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

Version resources: `apps/host/build.rs` and `apps/cli/build.rs` embed the Cargo version and `assets/icon.ico` (winresource).


## macOS disk image

`cargo xtask dist` puts the release bundle and an `Applications` link into `dist/SaveScummer-macos-arm64-<version>.dmg` (`hdiutil`, UDZO). The user drags the app over; there's no installer.

- Launch at login is an `SMAppService` agent whose plist is inside the bundle; macOS lists it as SaveScummer in *System Settings → General → Login Items*, under *Allow in the Background* (not *Open at Login*).
- The version is in `Info.plist`; the `winresource` build scripts do nothing on macOS.


## RELEASING

Releases are cut on the `release` branch. It is also the catalog channel: installed hosts download catalog updates from `release/catalog/catalog.json` on GitHub, so every push to `release` reaches users at once. Its catalog must stay readable by the installed versions. A tag alone does not move the catalog channel.

1. Merge `main` into `release` locally. Don't push the merge by hand: step 2 pushes it after checking it.
2. `cargo xtask release 1.2.3` on `release`, with a clean tree: checks the version is new and the tag free (locally and on origin), writes it into `Cargo.toml`, the Tauri config and the UI package manifest, refreshes `Cargo.lock`, checks the catalog (the last gate before the catalog goes out), commits "Release 1.2.3", tags `v1.2.3` locally and pushes the branch only. `--no-push` prints the push commands instead. If anything fails before the commit, the bump is undone.
3. Push the tag (`git push origin v1.2.3`; the release command prints it). This starts `.github/workflows/release.yml`: Windows, macOS and both Linux architectures run `cargo xtask test` once, then `cargo xtask dist` to package and upload their files. A final job runs `cargo xtask publish` only if all four platform jobs pass.
4. `publish` checks that `gh` is logged in, the tag points at `HEAD`, and `dist/` holds exactly the Windows installer, macOS disk image and both Linux AppImages. It creates a draft release (or re-uploads to an existing draft), and refuses if the release is already published.
5. Review the draft on GitHub and publish it by hand.

A published release never changes; fixes ship as a new version. To rebuild a draft from another commit, delete the tag locally and on origin and push it again.

`publish` also works locally: run `cargo xtask dist` on the tagged commit, put the other platform's release file for that same tag in `dist/`, then run `cargo xtask publish`.


## CI

[`release.yml`](../.github/workflows/release.yml) is the only GitHub Actions workflow. A `v*` tag runs the same `test` then `dist` commands on Windows, macOS, Linux x86_64 and Linux aarch64. Each job runs the Rust and UI suites once, then builds its platform package. All four jobs must pass before the workflow drafts a release. Runs are never cancelled.

The Windows pnpm store is cached using `apps/ui/pnpm-lock.yaml`; the pinned packaging tools are cached using `xtask/src/pins.rs`. Run the listed `cargo xtask` commands locally to reproduce build failures.


## VS Code

Tasks (`.vscode/tasks.json`) wrap the commands above: build dev (default build), test (default test), start/stop dev host, dist. The launch configurations debug the UI from the dev package against the dev host: they start the host before (in the tray) and stop it after.
