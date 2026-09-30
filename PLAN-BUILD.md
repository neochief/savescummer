# Save Scummer — Build & Release

I want building, packaging and releasing the app to be boring: one command to build, one command to cut a release, and one file per platform for users to download.

**Current implementation (2026-09-28):** Windows builds the UI, host and CLI in an Inno Setup installer. macOS builds the same three programs in an ad hoc signed app bundle and DMG. CI calls `cargo xtask dist` on both platforms; the tag workflow uploads both files to a draft GitHub Release. A tag-triggered release has not yet been verified here. Linux packaging remains planned. Use [docs/building.md](docs/building.md) for current commands.

This plan covers only the machinery around the app: builds, packaging, installers, CI and releases. App behavior lives in PLAN-HOST.md and PLAN-UI.md; the few things this plan needs from the app are listed under WHAT THE APP MUST PROVIDE.

This plan contains both implemented behavior and future platform targets. The status above and [docs/building.md](docs/building.md) identify what works today; the macOS and Linux release descriptions below are acceptance targets.

Windows builds the complete Tauri UI installer; macOS builds the complete Tauri UI disk image. Linux packaging is future work. The shared build machinery keeps platform details in modules:

- shared code stays platform-neutral
- platform logic lives in its own module
- nothing assumes Windows paths, tools or file names

Build order:

1. The shared parts: XTASK, VERSION, DISK, APP PACKAGE, TOOLCHAINS.
2. Windows, end to end.
3. macOS and Linux.

CI gets a job for each platform as it lands. The work is done when every platform's DONE WHEN list passes.


## PRINCIPLES

- **One build tool, in Rust.** All automation is `cargo xtask`: no PowerShell, bash or Python scripts, no just/make. It runs the same on every OS. Besides Rust, it needs only what the build itself needs: the UI frontend's toolchain (see TOOLCHAINS) and `gh` for publishing.
- **CI runs the same build commands developers run.** Workflows install prerequisites and call `cargo xtask` for checks, builds, packaging and publishing.
- **One release version, in `Cargo.toml`.** The Tauri and npm versions are checked against it and updated by `cargo xtask release`.
- **One file per platform per release,** in the friendliest format that platform has. Users never have to pick, and nothing else is uploaded.
- **Fixed executable names.** Platforms change how the programs are packaged, never what they're called.
- **Pin build inputs.** `cargo xtask setup` installs pinned packaging tools. Windows and macOS also need Node.js and the pnpm version declared in `apps/ui/package.json`; `pnpm install --frozen-lockfile` fetches the locked frontend dependencies. The Windows installer may fetch WebView2 during setup if the runtime is absent.
- **User data is sacred.** No install, upgrade, uninstall or clean ever touches the app's data directory or the checkpoint store, wherever the user has moved it: checkpoints are the user's saves.
- **Only a human publishes.** Tooling makes draft releases; I look at them and press publish.
- **Every failure says what to run next,** e.g. a missing Tauri build prerequisite names pnpm or the relevant `cargo xtask setup` command.


## WHAT USERS GET

The app is three programs:

- **Host** — Rust; the app itself and its entry point (SQLite, monitoring, operations, tray, hotkeys)
- **UI** — Tauri/WebView2 on Windows and Tauri/WebKit on macOS, started by the host; Linux support remains to be completed
- **CLI** — Rust command-line client

The Windows build always produces `SaveScummer.exe` (the host), `SaveScummer.UI.exe` and `SaveScummer.CLI.exe`. Other platforms use the same program names where their UI is implemented. Every launcher, shortcut and sign-in entry points at `SaveScummer`.

| Platform | Release file | Why this format |
| --- | --- | --- |
| Windows 10/11 x64 | `SaveScummer-windows-x64-<ver>-setup.exe` | What Windows users expect; installs per-user, no admin |
| macOS 13+, Apple Silicon | `SaveScummer-macos-arm64-<ver>.dmg` | The standard drag-to-Applications install |
| Linux x86_64, glibc 2.35+ (planned) | `SaveScummer-linux-x86_64-<ver>.AppImage` | One file to run without root or installation |

These minimums are recorded here and in xtask platform constants. Future build flags, `Info.plist`, Linux build system, CI runners and test machines must follow them.

What ends up on the user's machine:

**Windows** — `%LOCALAPPDATA%\Programs\SaveScummer\`:

```text
bin\SaveScummer.exe              host (what the Start menu runs)
bin\SaveScummer.UI.exe           UI
bin\SaveScummer.CLI.exe          CLI
```

**macOS** — `SaveScummer.app` in Applications:

```text
Contents/MacOS/SaveScummer       host (the bundle's main executable)
Contents/MacOS/SaveScummer.UI    UI
Contents/MacOS/SaveScummer.CLI   CLI
```

**Linux target** — the AppImage itself is the install and acts as all three programs:

```text
<file>.AppImage                  host
<file>.AppImage ui               UI (what the host runs)
<file>.AppImage cli …            CLI
```

All release files are named `SaveScummer-<os>-<arch>-<version>[-<suffix>].<ext>`, built by one function in xtask. Arch tags follow each OS's habit (`x64` on Windows, `arm64`/`x86_64` elsewhere) and are never unified.


## XTASK

`xtask/` is a workspace crate (`publish = false`), run as `cargo xtask` through the alias in `.cargo/config.toml` (`xtask = "run --package xtask --"`). Platform code lives in `windows.rs`, `macos.rs` and `linux.rs`, each compiled only on its OS; shared modules never contain platform logic.

Commands:

- `check` — the quality gate (see CHECK).
- `build [--release] [--test] [--package]` — build Rust and the UI. `--test` runs Rust and UI tests. `--package` assembles the APP PACKAGE; `--release` always does.
- `run [--demo] [--stop-other-hosts]` — dev build, then the dev host, launched the way a user launches the app, so it shows the UI. `--demo` uses simulated operations.
- `host start [--demo]` / `host stop` — just the dev host, with `--minimized`; used by VS Code debugging.
- `dist` — `build --release --test`, then the platform's release file in `dist/`.
- `clean [--deep]` — stop output processes, remove `build/` and `dist/` (`--deep` also `target/`). Works without Qt or any other tool, so a broken setup can always be cleaned.
- `release <version>` — cut a release (see RELEASING).
- `publish` — upload `dist/` to a draft GitHub release (see RELEASING).
- `setup qt|inno|linux-tools|cargo-about` — install a pinned tool (see TOOLCHAINS).
- `catalog [--check] [--strict]` — the game catalog; owned by PLAN-CATALOG.md.

Every command validates its inputs up front.

`cargo xtask run`:

1. Does a dev build.
2. Starts the dev host with `--data-dir .runtime/dev` (or `.runtime/dev-demo` with `--demo`), and records it in `build/dev/session.json`.
3. Waits for the host's `"ready":true` line. The host shows the UI itself, as it does for a user.

The dev host uses its own data, so development never touches the real app's data. An installed host is left running, and xtask warns that the dev instance won't own the tray icon or global shortcuts. `--stop-other-hosts` stops it instead (gracefully).

The dev host runs from the dev APP PACKAGE and outlives xtask. On macOS, xtask opens the app through Launch Services so the app owns its privacy requests; xtask reads the ready line and pid from `.runtime/dev/host.log` (or the demo data folder's log). On other platforms, startup output goes to `build/dev/logs/host.log` and the host inherits only its own stdio (NUL and the log), so a terminal pipeline or IDE task reading xtask's output does not hang until the host exits. On Windows and macOS, `run` opens the packaged Tauri UI.

xtask honors `CARGO_TARGET_DIR` like Cargo, so it can build next to another checkout's running binaries.

## VERSION

The version lives in `Cargo.toml` → `[workspace.package] version`. The git tag is always `v<version>`. Only `cargo xtask release` changes it.

Everything else reads it:

- **xtask** parses `Cargo.toml` with the `toml` crate — no pattern matching.
- **Tauri UI** has matching versions in `apps/ui/package.json` and `apps/ui/src-tauri/tauri.conf.json`; `cargo xtask release` updates both, and the build checks the Tauri version.
- **CMake** on Qt platforms gets it from xtask as `-DSAVESCUMMER_VERSION=<ver>`.
- **Windows version resources** come from `winresource` in `build.rs` for the host and CLI (using the version Cargo passes to the build).
- **The host's manifest** (also from `build.rs`) declares per-monitor DPI awareness, so the tray icon and its menu are drawn at the screen's real resolution instead of being stretched blurry on scaled displays.
- **macOS `Info.plist`** is filled from it.


## DISK

Four roots, each with one rule, so it's always obvious what's safe to delete:

| Root | Holds | `clean` |
| --- | --- | --- |
| `target/` | Cargo's cache; never written directly | only with `--deep` |
| `build/` | everything regenerable | always |
| `dist/` | release files and nothing else, so CI can upload whatever is there | always |
| `.runtime/` | SDKs, tools, dev data — slow to rebuild, and holds data | never |

```text
savescummer/
|-- .cargo/config.toml   the xtask alias
|-- rust-toolchain.toml  pinned Rust
|-- about.toml           licenses cargo-about accepts
|-- xtask/               the build program
|-- docs/building.md     the how-to
|-- packaging/
|   |-- licenses/        Qt license texts for Qt platform packages
|   |-- windows/         savescummer.iss, README.txt
|   |-- macos/           Info.plist.in, com.savescummer.SaveScummer.host.plist (login agent)
|   `-- linux/           AppRun, SaveScummer.desktop
|-- assets/              icons, sounds, asset tooling
|-- target/
|-- build/
|   |-- dev/             UI build, APP PACKAGE, session.json, logs
|   |-- release/         UI build, APP PACKAGE
|   `-- tmp/             scratch
|-- dist/
`-- .runtime/
    |-- Qt/<ver>/<kit>/  Qt SDK for Qt platforms
    |-- tools/           aqtinstall venv, Inno Setup, linuxdeploy, appimagetool
    `-- dev/             dev app data (the dev host's --data-dir)
```

There is no `scripts/` directory. `.gitignore` covers the four roots.


## APP PACKAGE

The APP PACKAGE is the assembled, runnable app under `build/<mode>/package/`: a folder on Windows, a `.app` on macOS, an AppDir on Linux. It's what the release file wraps, so what I test locally is exactly what ships.

Every current package contains:

- the host and CLI executables, with their platform runtime files; Windows and macOS packages also require the UI executable
- the Qt license texts when a Qt frontend is packaged
- `THIRD-PARTY-LICENSES.html`
- `WEB-THIRD-PARTY-LICENSES.html` for Windows and macOS JavaScript production dependencies
- `.savescummer-package.json` (mode, version, platform, UI toolkit, configuration, creation time), which marks it as generated output
- `SHA256SUMS.txt`

`THIRD-PARTY-LICENSES.html` is generated by cargo-about from the packaged Rust crates, including the Tauri UI, for the platform's own target, and merged into one page listing each license text once. A crate under a license not in `about.toml` fails packaging, so licensing is checked on every build rather than at release time. The app's own crates are `publish = false` and ignored as private, so they need no license of their own.

Each platform module declares what its package must contain, and packaging fails if anything required is missing. Windows and macOS require all three executables.

On Windows and macOS the Tauri UI is required. The frontend build uses `pnpm install --frozen-lockfile`, runs the UI tests under `build --test` or `dist`, embeds the Vite assets and stages `SaveScummer.UI` beside the host. Packaging fails if that executable is absent. Linux may still omit the UI until its frontend is implemented.

Cargo can't put dots in binary names, so Cargo builds `savescummer-host` and `savescummer-cli`, and packaging renames them to the fixed names.

Release packages have no debug symbols; dev packages keep them.

A package is assembled in a fresh `.staging-<uuid>` sibling and swapped into place only when complete, after stopping anything running from the old one. A failed build never damages the existing package.


## STOPPING PROCESSES

A running host may be in the middle of a save, and Windows can't replace a running executable. So xtask stops processes politely, always with the same routine:

1. Ask UIs to close.
2. Ask hosts to shut down through their sibling CLI (`--data-dir <dir> shutdown`, up to 30 s), so an accepted operation finishes.
3. Terminate whatever is still running.

It's used for:

- **Output processes.** Every `build` (whatever its flags, so also `run`, `host start` and `dist`), `check` and `clean` starts by stopping everything running from this checkout's output folders: `target/`, `build/` and `dist/`. That covers hosts and CLIs started straight from `target/`, the dev host, packaged copies, UIs and test binaries. Why: a rebuild must be clean. A process left running either locks its executable (Windows can't replace it, so the build fails with "access denied") or keeps serving old code next to the new build. Exempt are xtask itself and Cargo's build scripts, which belong to a build in progress. Never stopped: anything from `.runtime/` or installed copies (see below).
- **The dev host.** `build/dev/session.json` records its PID, start time and path. It's stopped only if all three still match, so a reused PID is never killed. The session file is deleted last.
- **Other hosts,** only with `run --stop-other-hosts`.

## TOOLCHAINS

The toolchains come in two layers, so the UI frontend can be replaced without touching anything else:

- **Core** — Rust and the packaging tools. The host, CLI, xtask, packaging and releases depend only on these.
- **UI frontend** — Tauri, Node.js and pnpm on Windows. The inactive Qt setup path is retained for an older frontend design.

The packaging tools are installed by setup commands into `.runtime/`. Windows UI builds use Node.js and the version of pnpm pinned in `apps/ui/package.json` from `PATH`; CI installs them explicitly.

### Core

**Rust** is pinned in `rust-toolchain.toml`: channel, `minimal` profile, rustfmt and clippy. rustup applies it everywhere. Bumping it is a deliberate one-line commit.

**Packaging tools:**

- `setup inno` — the pinned Inno Setup 6 installer from jrsoftware's GitHub release, checked by SHA-256, installed silently in portable mode into `.runtime/tools/inno-setup/`, so it registers nothing on the machine. Not winget or Chocolatey: they aren't reliably on CI runners and don't pin versions.
- `setup linux-tools` — planned Linux packaging tools. Select the deployment plugin when the Linux UI is implemented.
- `setup cargo-about` — pinned version, built with `cargo install --locked` into `.runtime/tools/cargo-about/`.

All pins, and the supported-platform minimums, live in `xtask/src/pins.rs`; CI caches are keyed on that file.

### What a UI frontend provides

Whatever the frontend is built with, it plugs into xtask the same way:

- **its prerequisites:** Windows uses Node.js and pnpm from `PATH`; a future platform UI must declare its own requirements
- **a build step** that takes the mode and the version and produces the `SaveScummer.UI` executable, plus the runtime files it needs, for the APP PACKAGE
- **a test step** that runs in CI (`pnpm test` for Windows)
- **its license notices,** shipped in every package

Extending the frontend to macOS and Linux means adding its setup, build, test and runtime packaging steps there. The core, release file naming and executable names remain shared.

### Retired Qt frontend design

No Qt UI project exists in the current tree, and neither macOS nor Linux currently packages a Qt UI. The following notes describe the earlier Qt path; they are not prerequisites for the Windows build or the current macOS CI job. The cross-platform UI gate is now the Tauri validation in [PLAN-UI.md](PLAN-UI.md).

The retired Qt plan called for following the latest minor release, with its exact version pinned in xtask. Its maintenance rules were:

- patches are taken promptly
- a new minor is adopted within about two months, in its own commit, once every supported platform builds and passes tests
- a minor that raises a platform's minimum OS is adopted only as a deliberate decision to drop that OS version
- a Qt UI would use Widgets, Network and Svg

`setup qt` installs the pinned Qt kit via aqtinstall (in a venv under `.runtime/tools/`; needs Python 3.9+) into `.runtime/Qt/<version>/`. Safe to re-run. CI caches it keyed on the pin.

**C++,** needed only for the Qt frontend:

- Windows builds need MSVC from Visual Studio 2022+ for Rust and the Tauri UI; CMake is not used for the Windows UI.
- macOS: Apple Clang from Xcode 15+
- Linux: GCC 11+
- CMake 3.21+, from PATH (Visual Studio, Xcode command-line tools or the distro provide it)

**The former build design.** The inactive xtask Qt path drives CMake if a Qt UI project is present:

- Configures `build/<mode>/ui` with the Qt kit as `CMAKE_PREFIX_PATH`, `RelWithDebInfo` for dev and `Release` for release.
- Builds `savescummer-ui`, and `ui-tests` with `--test`.
- Runs ctest with `SAVESCUMMER_TEST_HOST` set to the freshly built host and `QT_QPA_PLATFORM=offscreen` on every platform, so tests never need a display and run the same locally and in CI.

The CMake project sets C++17, `AUTOMOC`/`AUTORCC`, `find_package(Qt6 REQUIRED COMPONENTS Widgets Network Svg)` (xtask points it at the pinned kit), the `SaveScummer.UI` output name, `install()` rules, and `qt_generate_deploy_app_script` for deploying Qt. Test screenshots go to `build/<mode>/ui/screenshots`, never the source tree.


## CHECK

Tests always build as dev, even under `build --release --test`: they check dev-only behavior too, such as dev hosts refusing to create a sign-in entry.

`cargo xtask check` runs, stopping at the first failure:

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets --locked -- -D warnings`
3. `cargo test --workspace --locked -- --test-threads=1` (desktop focus tests run serially)
4. `cargo build --workspace --exclude xtask --locked` (rebuilding xtask would relink the running `xtask.exe`, which Windows can't replace; clippy and the tests already cover it)
5. `cargo xtask catalog --check`

UI tests are not part of `check`; they run with `build --test`.


## Windows

### App package

`build/<mode>/package/SaveScummer-windows-x64/`, containing:

- `SaveScummer.exe` (the host), `SaveScummer.UI.exe` (the Tauri UI with embedded web assets) and `SaveScummer.CLI.exe` in `bin/`
- the Visual C++ runtime DLLs, copied next to the app from the newest Visual Studio (found with vswhere), so users don't need to install a redistributable
- `README.txt`, Rust `THIRD-PARTY-LICENSES.html` and JavaScript `WEB-THIRD-PARTY-LICENSES.html`
- dev only: the PDBs

The host and CLI get version resources (see VERSION) and `assets/icon.ico`; the Tauri binary uses its own icon and version configuration.

### Installer

`packaging/windows/savescummer.iss`, a per-user Inno Setup installer to `%LOCALAPPDATA%\Programs\SaveScummer`. Per-user means no admin prompt. Its payload is exactly the release APP PACKAGE. Only `cargo xtask dist` compiles it, passing the version, payload and output as `/D` defines.

- **Stable `AppId`** and `PrivilegesRequired=lowest`, so upgrades replace in place without admin.
- **One task, "Launch at sign-in",** with `UsePreviousTasks=yes`: checked on first install; on upgrade the user's previous choice is kept, checked or not, so a user who turned it off is never opted back in. Not `checkedonce`: on an upgrade it unchecks the task, overriding the remembered choice, so an upgrade would silently turn sign-in off.
- **Launch at sign-in** is set by running the installed `SaveScummer.exe --autostart on`, so the host stays the only writer of that entry. The uninstaller runs `--autostart off`, which removes the entry only if it points at this install.
- **Before replacing files,** `PrepareToInstall` runs the installed `SaveScummer.CLI.exe --no-start shutdown`, so an in-flight save finishes. Inno's Restart Manager (`CloseApplications=yes`) closes the UI.
- **User data is never touched:** `%LOCALAPPDATA%\SaveScummer`, and the checkpoint store if the user moved it elsewhere.
- **Upgrades replace `bin\` wholesale,** so no file from an older version lingers. Only the app's own folder is cleared.
- **A Start menu entry and a "Launch SaveScummer" finish-page checkbox,** both running `SaveScummer.exe`, the same as a user launch: the host starts, or the running one is reached, and the UI shows.
- **WebView2 prerequisite:** if the Evergreen Runtime is absent, the installer downloads and installs it before installing the app.
- **Unsigned.** SmartScreen shows "More info → Run anyway"; the README and release notes say so.

`dist` fails if Inno Setup is missing: there's nothing to ship without it.

### Done when

1. `clean` stops recorded and output processes and removes `build/` and `dist/`; `--deep` also removes `target/`; `.runtime/` is untouched.
2. Any build while a host, CLI, UI or test binary runs from `target/`, `build/` or `dist/` first stops it (hosts gracefully), then succeeds; nothing is left running old code.
3. `run` builds and runs against `.runtime/dev`, or `.runtime/dev-demo` with `--demo`, recording `build/dev/session.json`; an installed host keeps running unless `--stop-other-hosts` is passed.
4. `dist` leaves exactly one file in `dist/`, the `-setup.exe`. The release package under `build/release/package/` has the UI, `SHA256SUMS.txt`, Rust and web third-party license files, and no PDBs.
5. Every binary's version resource (file properties → Details) shows the Cargo version.
6. Installing needs no admin, puts the binaries under `%LOCALAPPDATA%\Programs\SaveScummer\bin`.
7. The sign-in task is checked on first install and keeps the user's choice on upgrade. When it's checked, sign-in starts the host in the tray with no window.
8. Installing over a running host shuts it down gracefully first.
9. Uninstalling removes the app and its own sign-in entry, never `%LOCALAPPDATA%\SaveScummer` or a moved checkpoint store.
10. The Start menu entry shows the UI whether or not the host is already running, and never starts a second host.


## macOS release target

`cargo xtask dist` produces the complete app bundle and DMG with the Tauri UI. See [PLAN-MACOS.md](PLAN-MACOS.md) for platform behavior and manual acceptance checks.

Apple Silicon (M1 or later) only, on the minimum macOS or later. Intel Macs aren't supported, and xtask refuses to build on one.

### Build

Built on an Apple Silicon Mac. The whole app agrees on its minimum OS:

- Rust: `MACOSX_DEPLOYMENT_TARGET` (the build machine is Apple Silicon, so the native target is already arm64)
- The Tauri UI: its deployment target and arm64 build must match the Rust bundle
- `Info.plist`: `LSMinimumSystemVersion`

The host, CLI, UI and bundle must agree on the minimum macOS.

### App package

`build/<mode>/package/SaveScummer.app`:

- Bundle ID `com.savescummer.SaveScummer`, fixed forever, because macOS keys permissions and settings on it.
- The bundle has three executables side by side in `Contents/MacOS/`. The host, `SaveScummer`, is the bundle's main executable. The bundle has no Dock icon of its own (`LSUIElement`): the host lives in the menu bar, and the UI manages its window while open.
- `Contents/Info.plist` from `packaging/macos/Info.plist.in`: `CFBundleExecutable` `SaveScummer`, `LSUIElement`, `CFBundleShortVersionString` and `CFBundleVersion` from the Cargo version, the minimum macOS, the icon.
- Licenses, notices, manifest and checksums in `Contents/Resources/`, including the UI's web dependency licenses.
- Ad-hoc signed (`codesign --force --deep --sign -`) as the last step, after all packaged binaries and runtimes have been staged.

### Release file

- A DMG made with `hdiutil create -format UDZO` from a folder holding the app and an `Applications` link, so installing is drag-and-drop.
- Ad hoc signed and not notarized, so macOS blocks the first launch; the user opens it via *System Settings → Privacy & Security → Open Anyway*. The README and release notes show this with screenshots.

### Integration

**Launch at login:** a login agent registered with `SMAppService.agent`, through the shared `--autostart on|off` code. Its plist ships in the bundle (`Contents/Library/LaunchAgents/com.savescummer.SaveScummer.host.plist`) and runs `Contents/MacOS/SaveScummer --minimized`. macOS lists it as SaveScummer in *Login Items*, under *Allow in the Background*. A custom `--data-dir` can't be carried, so `--autostart on` refuses with one on macOS. Details in PLAN-MACOS.md, LAUNCH AT LOGIN.

### Upgrade and removal

The README gives both:

- **Upgrade:** quit (menu-bar icon → Exit), drag the new app over the old one. Replacing a running app is safe on macOS, since running processes keep the old files, and the next launch runs the new version.
- **Remove:** turn off launch at login, quit, drag to Trash. `~/Library/Application Support/SaveScummer` is never touched.

### Done when

1. `dist` on an Apple Silicon Mac leaves exactly `SaveScummer-macos-arm64-<ver>.dmg` in `dist/`; on an Intel Mac it refuses with a clear message.
2. Every binary in the bundle is arm64 (`lipo -archs`), and `codesign --verify --deep --strict` passes.
3. The DMG shows the app and an Applications link; dragging installs it; after *Open Anyway* it runs on a clean machine with the minimum macOS.
4. `Info.plist` has the Cargo version and the minimum macOS.
5. Enabling launch at login registers the agent, it shows as SaveScummer in *Login Items* (*Allow in the Background*), and the host starts at login; disabling unregisters it.
6. Replacing the app while it runs doesn't corrupt data, and the next launch runs the new version.
7. Nothing ever touches `~/Library/Application Support/SaveScummer`.


## Linux release target

This section is an unimplemented packaging concept. Its Qt-specific details belong to the retired frontend design above and need to be revised when the Linux UI is chosen and validated.

x86_64, any mainstream distro with the minimum glibc or newer (Ubuntu, Fedora, Arch, SteamOS…). No `.deb` or `.rpm`: one AppImage runs on all of them, without installing or root.

### Build

Built on the oldest supported Ubuntu, locally or on the matching `ubuntu-*` runner in CI. A binary only runs on a glibc at least as new as the one it was built against, so the oldest supported system has to be the build system.

### App package and release file

The APP PACKAGE is `build/<mode>/package/SaveScummer.AppDir`, made by `linuxdeploy` with its Qt plugin. It contains:

- the three executables
- Qt and every library not guaranteed on a base system (per the AppImage exclude list)
- both the `xcb` and `wayland` Qt platform plugins, so it runs under X11 and Wayland
- `packaging/linux/SaveScummer.desktop` and the icon
- licenses, notices, manifest and checksums

`packaging/linux/AppRun` is the entry point and dispatches on its first argument, so one file serves as all three programs:

- `ui` runs `SaveScummer.UI` (the host runs this to show the window)
- `cli …` runs `SaveScummer.CLI`
- anything else runs the host, `SaveScummer`, with those arguments

`appimagetool` turns the AppDir into `SaveScummer-linux-x86_64-<ver>.AppImage` using the static AppImage runtime, so users don't need `libfuse2`. Unsigned.

### Integration

- **Launch at login:** the host writes and removes `~/.config/autostart/SaveScummer.desktop` with `Exec="<AppImage path>" --minimized --data-dir "<data>"`, through the shared `--autostart on|off` code. The path comes from `$APPIMAGE`, which the AppImage runtime sets, and the host keeps it current (see WHAT THE APP MUST PROVIDE).
- **App menu entry:** adding the app to the menu is left to the user's AppImage tool (Gear Lever, AppImageLauncher…), which reads the embedded `.desktop` file.

### Upgrade and removal

The README gives both:

- **Upgrade:** download the new AppImage, quit the old one, start the new one, delete the old file.
- **Remove:** turn off launch at login, quit, delete the file. `~/.local/share/SaveScummer` is never touched.

### Done when

1. `dist` on the oldest supported Ubuntu leaves exactly `SaveScummer-linux-x86_64-<ver>.AppImage` in `dist/`.
2. After `chmod +x`, it runs on the oldest supported Ubuntu and current Fedora, both stock, without `libfuse2`, under both X11 and Wayland.
3. `….AppImage --version` and `….AppImage cli --version` print the Cargo version.
4. Enabling launch at login writes the XDG entry pointing at the AppImage and the host starts at login. Running a newer AppImage re-points the entry, and disabling removes it.
5. Nothing ever touches `~/.local/share/SaveScummer`.


## CI

Two workflows in `.github/workflows/`. Setup actions install toolchains and caches; the app build and packaging steps call `cargo xtask`.

**ci.yml** — so nothing merges that breaks a platform:

- Runs on pushes to the `release` branch, and by hand (the Actions tab) for any branch. Other pushes, pull requests and tags don't start it. Why: work lands on `main` unchecked by CI to save time, and releasing is when every platform must pass. Merging `main` into `release` runs it; the tag is pushed only once it passes (RELEASING).
- A concurrency group per ref cancels superseded runs.
- A Windows job on `windows-latest`: checkout, Rust cache, Node.js 22, pnpm from `apps/ui/package.json`, packaging tool caches, `setup cargo-about`, `setup inno`, `check`, then `dist`. This compiles and tests the Tauri UI and produces the real installer on every CI run.
- A macOS job on `macos-latest` (Apple Silicon): checkout, Rust cache, Node.js 22, pnpm, tool cache, `setup cargo-about`, `check`, `dist`. The Linux job is still future work. Packaging in CI means an unaccepted license fails the run.

**release.yml** — builds the Windows installer and macOS disk image into a draft GitHub Release:

- Runs on `v*` tags only, with a concurrency group that never cancels, so a release is never half-built.
- The Windows and macOS build jobs: checkout with full history and caches, Node.js/pnpm and packaging tools, `dist`, upload their release files from `dist/` (`if-no-files-found: error`).
- A final publish job (`needs:` both build jobs, `contents: write`) downloads the files into `dist/`, fetches the tag and runs `cargo xtask publish` with `GH_TOKEN`.


## RELEASING

### Cutting a release

`cargo xtask release <version>`:

1. Checks that the version has three parts (a leading `v` is fine), I'm on a branch, the tree is clean, the version differs from the current one, and tag `v<version>` exists neither locally nor on origin.
2. Writes the version into `Cargo.toml`, `apps/ui/package.json` and `apps/ui/src-tauri/tauri.conf.json`, then refreshes `Cargo.lock` (`cargo update --workspace`).
3. Runs `cargo xtask check` (`--skip-checks` for emergencies). If it fails, the bump is undone and the tree is left clean.
4. Commits "Release <version>", creates the annotated tag `v<version>`, and pushes the branch only (`--no-push` prints the commands instead).

It runs on the `release` branch, after merging `main` into it locally. The branch push runs CI on every platform; the tag is pushed by hand once that passes, and starts release.yml. Why the tag waits: the release build and the draft should only exist for a commit every platform passed. Why the branch can't wait too: `release` is the catalog channel (PLAN-CATALOG.md 6), and pushing it is how the release commit reaches CI. The local `check`, which includes `catalog --check`, is the gate before the catalog goes out.

### Publishing

`cargo xtask publish` works the same locally and in CI. A local publish needs the other platform's release file copied into `dist/` after `dist` runs:

1. Checks that `gh` is installed and logged in, `origin` exists, and tag `v<version>` exists and points at `HEAD`.
2. Checks that `dist/` holds exactly one release file for that version per supported platform, and nothing else.
3. If the release is already published, refuses. If a draft exists, re-uploads the files (`--clobber`). Otherwise creates a draft with `--generate-notes`.
4. Prints the draft URL.

Tooling only ever makes drafts; I publish by hand. A published release is never changed — that's what a new version is for. To rebuild a draft from a different commit, delete the tag locally and on origin and push it again.


## WHAT THE APP MUST PROVIDE

[PLAN-HOST.md](PLAN-HOST.md) owns these; they are repeated here because this plan depends on them:

- **`SaveScummer --autostart on|off [--data-dir <dir>]`** sets the launch-at-login preference, writes or removes the platform's entry, and exits:
  - Windows: a `Run` value
  - macOS: the `SMAppService` login agent
  - Linux: the XDG autostart entry

  `off` only removes an entry pointing at this host. The in-app checkbox uses the same code, so the host is the only writer and the entry can't get out of sync.
- **When running as an AppImage with autostart on, the host re-points its entry to its own path on every start,** because each version is a new file. Other platforms install to a fixed path, so the host never rewrites the entry on its own there.
- **Dev hosts never create a launch-at-login entry,** so signing in never starts a debug host and a dev host can't touch the installed app's entry. xtask marks dev builds with a compile-time flag; in them `--autostart on` refuses with a clear message and the in-app checkbox is disabled.
- **Data directories come from the `savescummer-platform` crate,** the single source every component uses:
  - Windows: `%LOCALAPPDATA%\SaveScummer`
  - macOS: `~/Library/Application Support/SaveScummer`
  - Linux: `~/.local/share/SaveScummer`

  The checkpoint store defaults to `checkpoints` inside it and the user can move it; wherever it lives, it's user data.


## DOCS

- `docs/building.md` — the how-to per OS: prerequisites, every `cargo xtask` command and flag, outputs, the release runbook, CI, dev data locations, VS Code tasks.
- `README.md` — the short version, linking to the guide:
  - install, upgrade and removal per platform, including the SmartScreen, *Open Anyway* and `chmod +x` steps
  - where data lives
  - the quickest build commands

Whenever the build changes, this plan and `docs/building.md` change with it.


## NOT DOING

- Code signing on any platform — it costs money and yearly upkeep; the workarounds above are documented instead.
- Update checks or auto-update — the app never checks for its own updates; users install the next release. Its only network use is fetching catalog updates and Steam artwork.
- Intel Mac, 32-bit or ARM Linux builds.
- Distro packages, Flatpak, Microsoft Store, Mac App Store — the three release files cover everyone.
- Explorer, Finder or file-manager integration on any platform — a game's saves can span several folders, so there's no single folder to right-click; the app never touches the file manager.
- Auto-publishing releases — a human always publishes.
- Renaming the executables, or cleaning `.runtime/`.
