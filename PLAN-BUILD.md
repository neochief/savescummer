# Save Scummer — Build & Release

I want building, packaging and releasing the app to be boring: one command to build, one command to cut a release, and one file per platform for users to download.

This plan covers only the machinery around the app: builds, packaging, installers, CI and releases. App behavior lives in PLAN-HOST.md and PLAN-UI.md; the few things this plan needs from the app are listed under WHAT THE APP MUST PROVIDE.

This is the target design: where the code disagrees, the code changes.

Windows should already work. macOS and Linux aren't implemented yet, but everything is built with them in mind, so adding a platform means adding a module, not reworking the shared parts:

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
- **CI runs what developers run.** Workflows only call `cargo xtask`, so a CI failure can always be reproduced locally.
- **One version, in `Cargo.toml`.** Every binary, the installer, the bundle and the git tag read it, so they can't drift apart.
- **One file per platform per release,** in the friendliest format that platform has. Users never have to pick, and nothing else is uploaded.
- **Fixed executable names.** Platforms change how the programs are packaged, never what they're called.
- **Nothing is downloaded silently.** Tools come only from `cargo xtask setup …`, at pinned versions, so developers and CI build with the same tools.
- **User data is sacred.** No install, upgrade, uninstall or clean ever touches the app's data directory or the checkpoint store, wherever the user has moved it: checkpoints are the user's saves.
- **Only a human publishes.** Tooling makes draft releases; I look at them and press publish.
- **Every failure says what to run next,** e.g. "Qt kit not found — run `cargo xtask setup qt`".


## WHAT USERS GET

The app is three programs:

- **Host** — Rust; the app itself and its entry point (SQLite, monitoring, operations, tray, hotkeys)
- **UI** — C++/Qt 6 window, started by the host
- **CLI** — Rust command-line client

The build always produces `SaveScummer` (the host), `SaveScummer.UI` and `SaveScummer.CLI` (`.exe` on Windows). Every launcher, shortcut and sign-in entry points at `SaveScummer`.

| Platform | Release file | Why this format |
| --- | --- | --- |
| Windows 10/11 x64 | `SaveScummer-windows-x64-<ver>-setup.exe` | What Windows users expect; installs per-user, no admin |
| macOS 13+, Apple Silicon | `SaveScummer-macos-arm64-<ver>.dmg` | The standard drag-to-Applications install |
| Linux x86_64, glibc 2.35+ (Ubuntu 22.04) | `SaveScummer-linux-x86_64-<ver>.AppImage` | One file runs on every distro, no root, nothing to install |

These minimums are written down only here and as xtask constants next to the Qt pin. Everything else follows them: build flags, `Info.plist`, the Linux build system, CI runners and test machines. The rest of this plan says "the minimum macOS" and "the oldest supported Ubuntu" instead of repeating numbers.

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

**Linux** — the AppImage itself is the install and acts as all three programs:

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
2. Starts the dev host with `--data-dir .runtime/dev`, and records it in `build/dev/session.json`.
3. Waits for the host's `"ready":true` line. The host shows the UI itself, as it does for a user.

The dev host uses its own data, so development never touches the real app's data. An installed host is left running, and xtask warns that the dev instance won't own the tray icon or global shortcuts. `--stop-other-hosts` stops it instead (gracefully).

The dev host runs from the dev APP PACKAGE, logs to `build/dev/logs/host.log`, and outlives xtask. It inherits only its own stdio (NUL and the log): a plain spawn would also hand it the pipes xtask's output goes to, and a terminal pipeline, VS Code task or CI step reading that output would hang until the host exits. Until the UI exists, the host has no window to show, and `run` prints the CLI command for driving the dev host.

xtask honors `CARGO_TARGET_DIR` like Cargo, so it can build next to another checkout's running binaries.

## VERSION

The version lives in `Cargo.toml` → `[workspace.package] version`. The git tag is always `v<version>`. Only `cargo xtask release` changes it.

Everything else reads it:

- **xtask** parses `Cargo.toml` with the `toml` crate — no pattern matching.
- **CMake** gets it from xtask as `-DSAVESCUMMER_VERSION=<ver>` and fails to configure without it. It never parses `Cargo.toml` itself, so there's only one parser.
- **Windows version resources** come from `winresource` in `build.rs` for the host and CLI (using the version Cargo passes to the build), and from CMake-filled `.rc.in` templates for the UI.
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
|   |-- licenses/        Qt license texts, shipped on every platform
|   |-- windows/         savescummer.iss, README.txt (+ README-qt.txt once the UI ships)
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
    |-- Qt/<ver>/<kit>/  Qt SDK
    |-- tools/           aqtinstall venv, Inno Setup, linuxdeploy, appimagetool
    `-- dev/             dev app data (the dev host's --data-dir)
```

There is no `scripts/` directory. `.gitignore` covers the four roots.


## APP PACKAGE

The APP PACKAGE is the assembled, runnable app under `build/<mode>/package/`: a folder on Windows, a `.app` on macOS, an AppDir on Linux. It's what the release file wraps, so what I test locally is exactly what ships.

Every package contains:

- the three executables and the Qt libraries they need
- the Qt license texts from `packaging/licenses/`
- `THIRD-PARTY-LICENSES.html`
- `.savescummer-package.json` (mode, version, platform, Qt version, configuration, creation time), which marks it as generated output
- `SHA256SUMS.txt`

`THIRD-PARTY-LICENSES.html` is generated by cargo-about from the crates the host and CLI link, for the platform's own target, and merged into one page listing each license text once. A crate under a license not in `about.toml` fails packaging, so licensing is checked on every build rather than at release time. The app's own crates are `publish = false` and ignored as private, so they need no license of their own.

Each platform module declares what its package must contain (at least the three executables), and packaging fails if anything is missing.

**Until the UI exists** (`apps/ui/CMakeLists.txt`), the frontend step is skipped with a note, and packages and installers carry only the host and CLI. Everything else, from licensing to the installer, already runs for real. Once the UI exists it's required: packaging fails without `SaveScummer.UI`.

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
- **UI frontend** — whatever the UI is built with. Today that's Qt and C++; nothing outside the frontend assumes either.

Every tool, in either layer, is installed only by its setup command, into `.runtime/`, and xtask uses it from there.

### Core

**Rust** is pinned in `rust-toolchain.toml`: channel, `minimal` profile, rustfmt and clippy. rustup applies it everywhere. Bumping it is a deliberate one-line commit.

**Packaging tools:**

- `setup inno` — the pinned Inno Setup 6 installer from jrsoftware's GitHub release, checked by SHA-256, installed silently in portable mode into `.runtime/tools/inno-setup/`, so it registers nothing on the machine. Not winget or Chocolatey: they aren't reliably on CI runners and don't pin versions.
- `setup linux-tools` — linuxdeploy and appimagetool, plus the linuxdeploy plugin for the current frontend (today its Qt plugin); pinned release, checked by SHA-256.
- `setup cargo-about` — pinned version, built with `cargo install --locked` into `.runtime/tools/cargo-about/`.

All pins, and the supported-platform minimums, live in `xtask/src/pins.rs`; CI caches are keyed on that file.

### What a UI frontend provides

Whatever the frontend is built with, it plugs into xtask the same way:

- **a setup command** for its pinned SDK, following the same rules as every other tool
- **a build step** that takes the mode and the version and produces the `SaveScummer.UI` executable, plus the runtime files it needs, for the APP PACKAGE
- **a test step** that runs headless against the freshly built host, so tests run the same locally and in CI
- **its license texts,** shipped in every package

Replacing the frontend means replacing this layer: its setup command, its build step, and the platform packaging steps that deploy its runtime (e.g. `macdeployqt`, linuxdeploy's Qt plugin). The core, the release files and the executable names stay the same.

### The current frontend: Qt

**Qt follows the latest minor release,** with the exact version pinned in one xtask constant, the only place it's written down. Open-source Qt only patches its newest minor, so staying current is the only way to get fixes:

- patches are taken promptly
- a new minor is adopted within about two months, in its own commit, once every supported platform builds and passes tests
- a minor that raises a platform's minimum OS is adopted only as a deliberate decision to drop that OS version
- the app uses only Widgets, Network and Svg, the most stable parts of Qt, so a bump is normally just a rebuild

`setup qt` installs the pinned Qt kit via aqtinstall (in a venv under `.runtime/tools/`; needs Python 3.9+) into `.runtime/Qt/<version>/`. Safe to re-run. CI caches it keyed on the pin.

**C++,** needed only for the Qt frontend:

- Windows: MSVC from Visual Studio 2022+. No CMake generator is passed, so CMake picks the newest Visual Studio.
- macOS: Apple Clang from Xcode 15+
- Linux: GCC 11+
- CMake 3.21+, from PATH (Visual Studio, Xcode command-line tools or the distro provide it)

**The build.** xtask drives CMake:

- Configures `build/<mode>/ui` with the Qt kit as `CMAKE_PREFIX_PATH`, `RelWithDebInfo` for dev and `Release` for release.
- Builds `savescummer-ui`, and `ui-tests` with `--test`.
- Runs ctest with `SAVESCUMMER_TEST_HOST` set to the freshly built host and `QT_QPA_PLATFORM=offscreen` on every platform, so tests never need a display and run the same locally and in CI.

The CMake project sets C++17, `AUTOMOC`/`AUTORCC`, `find_package(Qt6 REQUIRED COMPONENTS Widgets Network Svg)` (xtask points it at the pinned kit), the `SaveScummer.UI` output name, `install()` rules, and `qt_generate_deploy_app_script` for deploying Qt. Test screenshots go to `build/<mode>/ui/screenshots`, never the source tree.


## CHECK

Tests always build as dev, even under `build --release --test`: they check dev-only behavior too, such as dev hosts refusing to create a sign-in entry.

`cargo xtask check` runs, stopping at the first failure:

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets --locked -- -D warnings`
3. `cargo test --workspace --locked`
4. `cargo build --workspace --exclude xtask --locked` (rebuilding xtask would relink the running `xtask.exe`, which Windows can't replace; clippy and the tests already cover it)
5. `cargo xtask catalog --check`

UI tests are not part of `check`; they run with `build --test`.


## Windows

### App package

`build/<mode>/package/SaveScummer-windows-x64/`, containing:

- the `cmake --install` deploy, with Qt DLLs in `bin/`
- `SaveScummer.exe` (the host) and `SaveScummer.CLI.exe`
- the Visual C++ runtime DLLs, copied next to the app from the newest Visual Studio (found with vswhere), so users don't need to install a redistributable
- `README.txt` with the LGPL attribution, source links, and the note that the Qt DLLs may be replaced with interface-compatible builds
- dev only: the PDBs

Every binary gets a version resource (see VERSION) and `assets/icon.ico`.

### Installer

`packaging/windows/savescummer.iss`, a per-user Inno Setup installer to `%LOCALAPPDATA%\Programs\SaveScummer`. Per-user means no admin prompt. Its payload is exactly the release APP PACKAGE. Only `cargo xtask dist` compiles it, passing the version, payload and output as `/D` defines.

- **Stable `AppId`** and `PrivilegesRequired=lowest`, so upgrades replace in place without admin.
- **One task, "Launch at sign-in",** with `UsePreviousTasks=yes`: checked on first install; on upgrade the user's previous choice is kept, checked or not, so a user who turned it off is never opted back in. Not `checkedonce`: on an upgrade it unchecks the task, overriding the remembered choice, so an upgrade would silently turn sign-in off.
- **Launch at sign-in** is set by running the installed `SaveScummer.exe --autostart on`, so the host stays the only writer of that entry. The uninstaller runs `--autostart off`, which removes the entry only if it points at this install.
- **Before replacing files,** `PrepareToInstall` runs the installed `SaveScummer.CLI.exe --no-start shutdown`, so an in-flight save finishes. Inno's Restart Manager (`CloseApplications=yes`) closes the UI.
- **User data is never touched:** `%LOCALAPPDATA%\SaveScummer`, and the checkpoint store if the user moved it elsewhere.
- **Upgrades replace `bin\` wholesale,** so no file from an older version lingers. Only the app's own folder is cleared.
- **A Start menu entry and a "Launch SaveScummer" finish-page checkbox,** both running `SaveScummer.exe`, the same as a user launch: the host starts, or the running one is reached, and the UI shows. Until the UI exists, that starts the host in the tray.
- **Unsigned.** SmartScreen shows "More info → Run anyway"; the README and release notes say so.

`dist` fails if Inno Setup is missing: there's nothing to ship without it.

### Done when

1. `clean` stops recorded and output processes and removes `build/` and `dist/`; `--deep` also removes `target/`; `.runtime/` is untouched.
2. Any build while a host, CLI, UI or test binary runs from `target/`, `build/` or `dist/` first stops it (hosts gracefully), then succeeds; nothing is left running old code.
3. `run` builds and runs against `.runtime/dev`, recording `build/dev/session.json`; an installed host keeps running unless `--stop-other-hosts` is passed.
4. `dist` leaves exactly one file in `dist/`, the `-setup.exe`. The release package under `build/release/package/` has `SHA256SUMS.txt` and `THIRD-PARTY-LICENSES.html`, and no PDBs.
5. Every binary's version resource (file properties → Details) shows the Cargo version.
6. Installing needs no admin, puts the binaries under `%LOCALAPPDATA%\Programs\SaveScummer\bin`.
7. The sign-in task is checked on first install and keeps the user's choice on upgrade. When it's checked, sign-in starts the host in the tray with no window.
10. The Start menu entry shows the UI whether or not the host is already running, and never starts a second host.
8. Installing over a running host shuts it down gracefully first.
9. Uninstalling removes the app and its own sign-in entry, never `%LOCALAPPDATA%\SaveScummer` or a moved checkpoint store.


## macOS

Apple Silicon (M1 or later) only, on the minimum macOS or later, which must never be below what the pinned Qt supports. Intel Macs aren't supported, and xtask refuses to build on one.

### Build

Built on an Apple Silicon Mac. The whole app agrees on its minimum OS:

- Rust: `MACOSX_DEPLOYMENT_TARGET` (the build machine is Apple Silicon, so the native target is already arm64)
- CMake: `CMAKE_OSX_ARCHITECTURES=arm64`, `CMAKE_OSX_DEPLOYMENT_TARGET`
- `Info.plist`: `LSMinimumSystemVersion`

All three are set to the minimum macOS.

### App package

`build/<mode>/package/SaveScummer.app`:

- Bundle ID `com.savescummer.SaveScummer`, fixed forever, because macOS keys permissions and settings on it.
- The three executables side by side in `Contents/MacOS/`. The host, `SaveScummer`, is the bundle's main executable, so opening the app starts the host, and opening it again while it runs reaches the running host. The bundle has no Dock icon of its own (`LSUIElement`): the host lives in the menu bar, and the UI turns its Dock icon on while its window is open.
- `Contents/Info.plist` from `packaging/macos/Info.plist.in`: `CFBundleExecutable` `SaveScummer`, `LSUIElement`, `CFBundleShortVersionString` and `CFBundleVersion` from the Cargo version, the minimum macOS, the icon.
- Qt frameworks deployed by `macdeployqt` for the UI only (the host and CLI don't link Qt); licenses, notices, manifest and checksums in `Contents/Resources/`.
- Ad-hoc signed (`codesign --force --deep --sign -`) as the last step, because Apple Silicon won't run binaries without a valid signature and `macdeployqt` breaks the linker's signatures when it rewrites library paths.

### Release file

- A DMG made with `hdiutil create -format UDZO` from a folder holding the app and an `Applications` link, so installing is drag-and-drop.
- Unsigned and not notarized, so macOS blocks the first launch; the user opens it via *System Settings → Privacy & Security → Open Anyway*. The README and release notes show this with screenshots.

### Integration

**Launch at login:** a login agent registered with `SMAppService.agent`, through the shared `--autostart on|off` code. Its plist ships in the bundle (`Contents/Library/LaunchAgents/com.savescummer.SaveScummer.host.plist`) and runs `Contents/MacOS/SaveScummer --minimized`. macOS lists it as SaveScummer in *Login Items*. A custom `--data-dir` can't be carried, so `--autostart on` refuses with one on macOS. Details in PLAN-MACOS.md, LAUNCH AT LOGIN.

### Upgrade and removal

The README gives both:

- **Upgrade:** quit (menu-bar icon → Exit), drag the new app over the old one. Replacing a running app is safe on macOS, since running processes keep the old files, and the next launch runs the new version.
- **Remove:** turn off launch at login, quit, drag to Trash. `~/Library/Application Support/SaveScummer` is never touched.

### Done when

1. `dist` on an Apple Silicon Mac leaves exactly `SaveScummer-macos-arm64-<ver>.dmg` in `dist/`; on an Intel Mac it refuses with a clear message.
2. Every binary in the bundle is arm64 (`lipo -archs`), and `codesign --verify --deep --strict` passes.
3. The DMG shows the app and an Applications link; dragging installs it; after *Open Anyway* it runs on a clean machine with the minimum macOS.
4. `Info.plist` has the Cargo version and the minimum macOS.
5. Enabling launch at login registers the agent, it shows as SaveScummer in *Login Items*, and the host starts at login; disabling unregisters it.
6. Replacing the app while it runs doesn't corrupt data, and the next launch runs the new version.
7. Nothing ever touches `~/Library/Application Support/SaveScummer`.


## Linux

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

Two workflows in `.github/workflows/`, every step a `cargo xtask` command.

**ci.yml** — so nothing merges that breaks a platform:

- Runs on branch pushes, pull requests and manual runs; not tags.
- A concurrency group per ref cancels superseded runs.
- One required job each on `windows-latest`, `macos-latest` (Apple Silicon) and the oldest supported Ubuntu: checkout, Rust cache, Qt and tool caches, `setup qt` (only once the UI exists; Linux also `setup linux-tools`), `setup cargo-about`, `check`, `build --test --package`. Packaging in CI means a dependency with an unaccepted license fails the pull request, not the release.

**release.yml** — builds every platform's file into one draft release:

- Runs on `v*` tags only, with a concurrency group that never cancels, so a release is never half-built.
- One build job per platform: checkout with full history, caches, `setup qt` and `setup cargo-about` (plus `setup inno` on Windows, `setup linux-tools` on Linux), `dist`, upload its one file from `dist/` (`if-no-files-found: error`).
- A final publish job (`needs:` every build job, `contents: write`) downloads their files into `dist/`, fetches the tag and runs `cargo xtask publish` with `GH_TOKEN`.


## RELEASING

### Cutting a release

`cargo xtask release <version>`:

1. Checks that the version has three parts (a leading `v` is fine), I'm on a branch, the tree is clean, the version differs from the current one, and tag `v<version>` exists neither locally nor on origin.
2. Writes the version into `Cargo.toml` with `toml_edit` (formatting preserved) and refreshes `Cargo.lock` (`cargo update --workspace`).
3. Runs `cargo xtask check` (`--skip-checks` for emergencies). If it fails, the bump is undone and the tree is left clean.
4. Commits "Release <version>", creates the annotated tag `v<version>`, and pushes both (`--no-push` prints the two commands instead).

The tag starts release.yml.

### Publishing

`cargo xtask publish` works the same locally and in CI:

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
