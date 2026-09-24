# PLAN-INFRA.md — Build, Distribution & Release Infrastructure

Status: **target design** — the final state. Where the code disagrees, the
code changes.
Date: 2026-09-24

## 0. How to read this

This document covers everything between "source code" and "a person has the
app installed": builds, packaging, installers, CI, and releases. `PLAN.md`
is authoritative for the application itself; this document never changes
application behavior, only the machinery around it. The few things this
infrastructure needs from the application are listed in §11.

Every decision here is final. There are no open questions: where a choice
had alternatives, the alternative was weighed and the choice is written
down with its reason.

Build order when implementing: the shared foundations (§1–§4), then Windows
(§5) end to end, then macOS (§6) and Linux (§7). CI (§8) grows a job per
platform as it lands. The work is done when all three acceptance checklists
pass.

Whenever the infrastructure changes, this document and `docs/building.md`
change with it.

---

## 1. The system at a glance

SaveScummer is three cooperating executables plus, on Windows, a shell
extension. Their names are part of the application contract and never
change (Windows adds `.exe`):

| Executable | What it is |
|---|---|
| `SaveScummer` | C++/Qt 6 desktop client |
| `SaveScummer.Host` | Rust background host (SQLite, monitoring, operations) |
| `SaveScummer.CLI` | Rust command-line client |
| `savescummer-explorer.dll` | Windows Explorer context-menu extension (Windows only) |

It ships on three platforms, one release file each:

| Platform | Release file | Made with |
|---|---|---|
| Windows 10/11 x64 | `SaveScummer-windows-x64-<ver>-setup.exe` | Inno Setup |
| macOS 13+ Apple Silicon | `SaveScummer-macos-arm64-<ver>.dmg` | `hdiutil` |
| Linux x86_64 (glibc ≥ 2.35) | `SaveScummer-linux-x86_64-<ver>.AppImage` | `linuxdeploy` + `appimagetool` |

All build, package, and release automation is **one Rust program**,
`cargo xtask`, living in the workspace. It runs the same on every OS and
needs nothing beyond the Rust toolchain the project already requires; it
calls out to CMake, Qt's deploy tools, and each platform's packaging tool.

```
                ┌──────────────── cargo xtask ────────────────┐
   repo ──────▶ │ version · cargo build/test · cmake build/test│
                │ app package (build/<mode>/package/)          │
                └──────┬───────────────┬───────────────┬──────┘
                Windows│          macOS│          Linux│
             Qt DLLs + VC runtime  macdeployqt     linuxdeploy
             Explorer DLL          ad-hoc sign     AppRun dispatch
             Inno Setup            hdiutil         appimagetool
                       ▼               ▼               ▼
                dist/…-setup.exe   dist/….dmg    dist/….AppImage
                       └───────────────┼───────────────┘
                                       ▼
                     one DRAFT GitHub release ──▶ a human publishes
```

### The rules that don't bend

1. **Canonical names are fixed.** The executables above keep their exact
   names everywhere.
2. **One version source.** `Cargo.toml` → `[workspace.package] version`.
   Everything reads it; the git tag must equal `v<version>`.
3. **Draft-first, always.** Tooling may create or update a *draft* release;
   only a human publishes.
4. **Four roots.** `target/` is Cargo's; `build/` is regenerable; `dist/`
   holds only release files; `.runtime/` is long-lived machine state and is
   never cleaned.
5. **CI runs what developers run.** Workflows call `cargo xtask`; they never
   reimplement build logic.
6. **One build language: Rust.** No PowerShell, bash, or Python build
   scripts, and no task runners (just/make). Platform-specific code lives in
   platform modules of `xtask`, compiled only on their OS.
7. **One file per platform per release.** The most user-friendly format the
   platform has; nothing else is uploaded.
8. **No silent downloads.** Anything fetched from the internet (Qt, Inno
   Setup, AppImage tools, cargo-about) is fetched only by an explicit
   `cargo xtask setup …`, at a pinned version.
9. **User data is sacred.** No install, upgrade, uninstall, or clean ever
   touches the application's data directory.
10. **Every failure says what to run next.**

---

## 2. Repository layout and disk tiers

```
savescummer/
├── .cargo/config.toml   alias: xtask = "run --package xtask --"
├── rust-toolchain.toml  pinned Rust (§4)
├── about.toml           accepted licenses for cargo-about (§4)
├── xtask/               the build program (§3)
├── packaging/
│   ├── licenses/        Qt license texts, shipped on every platform
│   ├── windows/         savescummer.iss
│   ├── macos/           Info.plist.in
│   └── linux/           SaveScummer.desktop, AppRun
├── assets/              icons, sounds (+ generate-sounds.mjs, asset tooling)
├── target/              Cargo's. Never written directly.
├── build/               Everything regenerable:
│   ├── dev/               desktop tree, app package, session.json, logs,
│   │                      build-report.json
│   ├── release/           desktop + explorer trees, app package,
│   │                      build-report.json
│   └── tmp/               scratch; the test temp is emptied each run
├── dist/                The release file(s), nothing else
└── .runtime/            Never cleaned:
    ├── Qt/<version>/<kit>   Qt SDK
    ├── tools/               aqtinstall venv, linuxdeploy, appimagetool
    └── dev/                 dev app data (the dev host's --data-dir)
```

There is no `scripts/` directory.

| Root | Contents | Removed by `cargo xtask clean`? |
|---|---|---|
| `target/` | Cargo cache | only with `--deep` |
| `build/` | trees, app packages, logs, reports, scratch | yes |
| `dist/` | release files | yes |
| `.runtime/` | SDKs, tools, dev data | **never** |

`.gitignore` covers the four roots plus `*.db`, `*.db-shm`, `*.db-wal`.

**Artifact names** are always
`SaveScummer-<os>-<arch>-<version>[-<suffix>].<ext>`. Arch tags follow each
OS's idiom — `x64` on Windows, `arm64`/`x86_64` elsewhere — and are never
normalized across platforms.

**The app package** is the assembled, runnable app under
`build/<mode>/package/`: a folder on Windows (the installer's payload), a
`.app` bundle on macOS, an AppDir on Linux. Release packages carry no debug
symbols; dev packages keep them.

---

## 3. `cargo xtask`

`xtask/` is a binary crate in the workspace (`publish = false`), invoked as
`cargo xtask <command>` through the alias in `.cargo/config.toml`. It is
organized as shared modules plus `windows.rs`, `macos.rs`, `linux.rs`
compiled under `cfg(target_os = …)`. Shared code never contains
platform-specific logic.

### Commands

| Command | What it does |
|---|---|
| `check` | The quality gate (§4). |
| `build [--release] [--test] [--package]` | Build the Rust workspace and the desktop. `--test` runs Rust and desktop tests. `--package` assembles the app package (always on with `--release`). |
| `run [--demo] [--stop-other-hosts]` | Dev build, then start the dev host against `.runtime/dev` and the desktop connected to it (§3.2). `--demo` uses simulated operations. |
| `host start [--demo]` / `host stop` | Start or stop just the dev host; used by `run` and by VS Code debugging tasks. |
| `dist` | `build --release --test`, then the platform's release file into `dist/`. |
| `clean [--deep]` | Stop dev/output processes, remove `build/` and `dist/` (and `target/` with `--deep`). Never needs a toolchain. |
| `release <version>` | Cut a release (§9). |
| `publish` | Create or update the draft GitHub release from `dist/` (§9). |
| `setup <qt\|inno\|linux-tools\|cargo-about>` | Explicit, pinned bootstraps (§4). |
| `explorer <build\|register\|unregister\|check> [--dev]` | Windows only: the Explorer extension (§5.3). |
| `catalog [--check] [--strict]` | The game catalog build; its behavior is owned by `PLAN-CATALOG.md`. |

Every command validates its inputs up front and fails with a plain message
naming the command to run next (e.g. "Qt 6.11.2 not found — run `cargo xtask
setup qt`").

### 3.1 Shared building blocks

- **Version:** read by parsing `Cargo.toml` with the `toml` crate and taking
  `workspace.package.version` — no pattern matching.
- **Artifact names:** one function builds every name from os, arch,
  version, suffix, extension.
- **Staging and atomic swap:** a package is assembled in a fresh
  `.staging-<uuid>` sibling, then swapped into place after stopping anything
  running from the old one. A failed run never damages the existing package.
- **Package manifest:** `.savescummer-package.json` in every package (mode,
  version, platform, Qt version, configuration, creation time) marks it as
  generated output.
- **Checksums:** `SHA256SUMS.txt` in every package.
- **Third-party notices:** `THIRD-PARTY-LICENSES.html` in every package,
  generated by `cargo about` from the crates the host and CLI link; a
  dependency under a license not in `about.toml` fails packaging.
- **Build report:** `build/<mode>/build-report.json` — steps, timings,
  binary paths and sizes.

### 3.2 Process handling

A running host may be mid-save, and Windows cannot replace a running
executable, so xtask stops processes politely, in this order: desktops are
asked to close; hosts are asked to shut down through their sibling CLI
(`--data-dir <dir> shutdown`, up to 30 s) so an accepted operation
finishes; only then are stragglers terminated. This one routine is used
everywhere:

- **The recorded dev host.** `run`/`host start` record the host in
  `build/dev/session.json` (PID, start time, path). Stopping verifies all
  three still match before acting, so a reused PID is never killed. The
  session file is deleted last.
- **Output processes.** Before rebuilding or cleaning, anything running
  from `build/` or `dist/` is stopped — never anything under `target/` or
  `.runtime/`.
- **Other hosts.** A dev `run` leaves the user's installed host alone; if
  one is running, xtask warns that the dev instance won't own the tray icon
  or global shortcuts. `--stop-other-hosts` stops it (gracefully) instead.
- **Stale dev autostart.** Any launch-at-login entry that points into
  `target/` or `build/` is removed on every dev build, so sign-in never
  starts a debug host. Entries pointing elsewhere are the user's and stay.

`run` starts the host hidden, waits for its `"ready":true` line (30 s),
then launches the desktop with the host's endpoint.

### 3.3 The desktop build

xtask drives CMake: configure `build/<mode>/desktop` with
`-DCMAKE_PREFIX_PATH=<Qt kit>` and the configuration `RelWithDebInfo` (dev)
or `Release` (release); build `savescummer-desktop` and, with `--test`,
`desktop-tests`; run ctest with `SAVESCUMMER_TEST_HOST` set to the freshly
built host (Linux CI adds `QT_QPA_PLATFORM=offscreen`). On failure it prints
the JUnit XML. On Windows no generator is passed, so CMake picks the newest
installed Visual Studio (2022 or later).

The CMake project reads the version from `Cargo.toml` before `project()`,
matching only a line that starts with `version = "x.y.z"`:

```cmake
file(READ "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml" _savescummer_manifest)
# "\n" pins the match to a line start (CMake's ^ means start-of-file), so an
# inline dependency such as foo = { version = "1.2.3" } never matches.
string(REGEX MATCH "\n[ \t]*version[ \t]*=[ \t]*\"([0-9]+\\.[0-9]+\\.[0-9]+)\"" _v "${_savescummer_manifest}")
if(NOT _v)
    message(FATAL_ERROR "Cannot read the SaveScummer version from Cargo.toml.")
endif()
project(SaveScummer VERSION ${CMAKE_MATCH_1} LANGUAGES CXX)
```

This requires `[workspace.package]` to stay above any dependency written as
its own table (`[dependencies.foo]` with `version = …` on its own line). It
also sets C++17, `AUTOMOC`/`AUTORCC`,
`find_package(Qt6 6.11 REQUIRED COMPONENTS Widgets Network Svg)`, the
`SaveScummer` output name, `install()` rules, and
`qt_generate_deploy_app_script` for the Qt deploy step. Tests write
screenshots to `build/<mode>/desktop/screenshots`, never the source tree.

---

## 4. Toolchains

**Rust** is pinned in `rust-toolchain.toml` (channel, `minimal` profile,
`rustfmt` + `clippy`, and the `aarch64-apple-darwin` target). rustup applies
it automatically everywhere. Bumping Rust is a deliberate one-line commit.

**Qt tracks the latest minor release**, currently **6.11**, with the exact
patch pinned in one constant in `xtask`. Open-source Qt gets patch releases
only for the newest minor, so staying current is the only way to get fixes:
patch releases are taken promptly, and a new minor is adopted within about
two months of its release, in its own commit, once all three platforms
build and pass tests. The app uses only Widgets, Network, and Svg — the
most stable parts of Qt — so a minor bump is normally a rebuild.
`cargo xtask setup qt` installs the pinned kit with aqtinstall (in a venv
under `.runtime/tools/`; it needs Python 3.9+) to `.runtime/Qt/<version>/`.
It's idempotent, and CI caches `.runtime/Qt` keyed on the pin.

**C++ compilers:** MSVC from Visual Studio 2022 or later on Windows, Apple
Clang from Xcode 15+ on macOS, GCC 11+ on Linux. **CMake** ≥ 3.21 from
PATH (Visual Studio, Xcode's command-line tools, or the distro provide it).

**Platform tools**, each installed only by its explicit setup command:
Inno Setup 6 (`setup inno`, via winget), linuxdeploy with its Qt plugin and
appimagetool (`setup linux-tools`, pinned release + SHA-256, into
`.runtime/tools/`), and cargo-about (`setup cargo-about`, pinned version).

### The quality gate — `cargo xtask check`

`cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked
-- -D warnings`, `cargo test --workspace --locked`, `cargo build --workspace
--locked`, then `cargo xtask catalog --check`. First failure stops it.
Desktop tests are not part of `check`; they run with `build --test`.

---

## 5. Windows

### 5.1 The app package

`build/<mode>/package/SaveScummer-windows-x64/`: the `cmake --install`
deploy (Qt DLLs into `bin/`), `SaveScummer.Host.exe` and
`SaveScummer.CLI.exe`, the VC runtime DLLs copied app-locally from the
newest installed Visual Studio (found with vswhere), the Explorer DLL
(release only), the Qt license texts, `README.txt` (LGPL attribution,
source links, and the note that the Qt DLLs may be replaced with
interface-compatible builds), `THIRD-PARTY-LICENSES.html`, the package
manifest, and `SHA256SUMS.txt`. Dev packages add the PDBs.

### 5.2 Identity

Every binary carries a version resource from the single version: host and
CLI via `winresource` in `build.rs` (which **must** print
`cargo:rerun-if-changed=Cargo.toml`, or a version bump ships a stale
resource), desktop and Explorer DLL via `.rc.in` templates filled by CMake.
The icon is `assets/icon.ico`.

### 5.3 The Explorer extension

A classic COM `IContextMenu` handler, registered per-user under
`HKCU\Software\Classes` (no admin). On Windows 11 it appears under *Show
more options*; the modern `IExplorerCommand` menu requires package identity
(MSIX) and code signing, which this project doesn't have, so the classic
handler is the design.

Its identity lives in exactly **one** file,
`integrations/windows-explorer/identity.h`. xtask parses it and passes the
values to Inno Setup (`/D` defines) and to its own registration code:

| | Release | Development |
|---|---|---|
| CLSID | `{3F8F42CE-463F-41B6-98D1-8C8D16B88931}` | `{43BFBA41-D0AB-44D3-A5D6-600EB5C74D18}` |
| Handler name | `SaveScummer` | `SaveScummerDev` |

The dev identity means a dev registration never shadows the installed one.
`cargo xtask explorer build` builds the DLL and runs its COM tests;
`register`/`unregister [--dev]` write or remove the per-user registration
(`InprocServer32` with `ThreadingModel=Apartment`, plus
`Directory\shellex\ContextMenuHandlers\<name>`); `check` exercises the real
handler without registering anything.

The extension's Rust bridge finds the host's data directory through the
`savescummer-platform` crate's default-data-directory function — the same
code the host uses — never by joining a folder name itself.

`explorer.exe` keeps the DLL loaded, so it can't be overwritten in place;
the installer replaces it on restart, and a fresh Explorer process picks up
changes.

### 5.4 The installer

`packaging/windows/savescummer.iss`, compiled by `cargo xtask dist` with
`/D` defines for version, payload, output, and the Explorer identity — the
`.iss` is never compiled by hand. It is a **per-user** installer to
`%LOCALAPPDATA%\Programs\SaveScummer` (no admin), and its payload is exactly
the release app package.

- **Stable `AppId`**, so upgrades replace in place; `PrivilegesRequired=lowest`.
- **Two tasks — Explorer menu and launch at sign-in — both `checkedonce`
  with `UsePreviousTasks=yes`:** checked on a first install, unchecked on
  upgrades, so a user who turned either off is never opted back in.
- **Launch at sign-in** is set by running the installed
  `SaveScummer.Host.exe --autostart on` (§11) — the host is the only writer
  of that entry. The uninstaller runs `--autostart off`, which removes the
  entry only if it points at this installation.
- **The Explorer DLL** is its own `[Files]` entry with `restartreplace`.
- **Registry writes are HKCU-only**, and the CLSID subtree uses
  `uninsdeletekey`.
- **Before replacing files**, `PrepareToInstall` runs the installed
  `SaveScummer.CLI.exe --no-start shutdown`, so an in-flight save finishes;
  the desktop is closed by Inno's Restart Manager (`CloseApplications=yes`).
- **Data safety:** `%LOCALAPPDATA%\SaveScummer` is never touched.
- **Unsigned.** SmartScreen shows "More info → Run anyway" on first run;
  the README and release notes say so. The script has an off-by-default
  `SignedBuild` block (`SignTool`, `SignedUninstaller`) so signing can be
  switched on without other changes.

`dist` fails if Inno Setup is missing (there's nothing to ship without it)
or if the payload lacks `bin/SaveScummer.exe` or the Explorer DLL.

### 5.5 Windows acceptance checklist

1. `cargo xtask clean` stops recorded and output processes, removes `build/`
   and `dist/`; `--deep` also `target/`; `.runtime/` untouched.
2. `cargo xtask run` builds and runs against `.runtime/dev`, recording
   `build/dev/session.json`; an installed host keeps running unless
   `--stop-other-hosts`.
3. `cargo xtask dist` leaves exactly one file in `dist/`, the `-setup.exe`,
   and a release package under `build/release/package/` with
   `SHA256SUMS.txt`, `THIRD-PARTY-LICENSES.html`, and no PDBs.
4. CMake configure shows the Cargo version; screenshots land in
   `build/<mode>/desktop/screenshots`.
5. Installing needs no admin, puts the binaries under
   `%LOCALAPPDATA%\Programs\SaveScummer\bin`, and the Explorer task adds the
   menu (under *Show more options* on Windows 11).
6. The sign-in task is checked on first install and unchecked on upgrade;
   when checked, sign-in starts the host minimized.
7. Installing over a running host shuts it down gracefully first; a DLL
   update completes after restart.
8. Uninstalling removes the app, its registrations, and its own sign-in
   entry — never `%LOCALAPPDATA%\SaveScummer`.

---

## 6. macOS

Apple Silicon only (M1 or later), macOS 13 or newer (Qt 6.11's minimum).
Intel Macs are not supported.

### 6.1 Build

Built on an Apple Silicon Mac. Rust builds with
`--target aarch64-apple-darwin`; CMake with
`-DCMAKE_OSX_ARCHITECTURES=arm64` and
`-DCMAKE_OSX_DEPLOYMENT_TARGET=13.0`, and Rust with
`MACOSX_DEPLOYMENT_TARGET=13.0`, so the whole app agrees on its minimum OS.
xtask refuses to run the macOS build on an Intel Mac.

### 6.2 The app package

`build/<mode>/package/SaveScummer.app`, bundle identifier
`com.savescummer.SaveScummer` (fixed forever — macOS keys permissions and
settings on it):

- `Contents/MacOS/SaveScummer` (desktop), `SaveScummer.Host`,
  `SaveScummer.CLI` side by side.
- `Contents/Info.plist` from `packaging/macos/Info.plist.in`:
  `CFBundleShortVersionString`/`CFBundleVersion` from the Cargo version,
  `LSMinimumSystemVersion` 13.0, the icon.
- Qt frameworks deployed by `macdeployqt`; licenses, notices, manifest, and
  checksums in `Contents/Resources/`.
- **Ad-hoc signed** (`codesign --force --deep --sign -`) as the last step:
  Apple Silicon refuses to run binaries without a valid signature, and
  `macdeployqt` invalidates the linker's ad-hoc signatures when it rewrites
  library paths.

### 6.3 The release file

A DMG made with `hdiutil create -format UDZO` from a folder holding the
`.app` and an `Applications` link, so installing is drag-and-drop. Never a
zip: PowerShell- or .NET-style zips break the symlinks inside Qt's
frameworks.

**Unsigned and not notarized.** On first launch macOS blocks the app; the
user opens it via *System Settings → Privacy & Security → Open Anyway*. The
README and release notes show this step with screenshots.

### 6.4 Launch at login

The host writes and removes `~/Library/LaunchAgents/com.savescummer.host.plist`
(pointing at the host inside the bundle, with `--minimized` and the data
directory) and loads or unloads it with `launchctl`. It's driven by the
in-app setting through the same `--autostart on|off` code as every
platform (§11). macOS shows its standard "Background item added" notice.

### 6.5 Integration, upgrade, removal

- **No Finder integration.** The only sanctioned route is a FinderSync app
  extension, which requires proper code signing.
- **Upgrade:** quit SaveScummer (menu-bar icon → Quit), drag the new app
  over the old one. Replacing a running app is safe on macOS — running
  processes keep the old files — and the next launch runs the new version.
  The README gives these two steps.
- **Removal:** turn off *Launch at login*, quit, drag the app to the Trash.
  The README says this and gives the data location
  (`~/Library/Application Support/SaveScummer`), which removal never
  touches.

### 6.6 macOS acceptance checklist

1. `cargo xtask dist` on an Apple Silicon Mac leaves exactly
   `SaveScummer-macos-arm64-<ver>.dmg` in `dist/`; on an Intel Mac it
   refuses with a clear message.
2. Every binary in the bundle is arm64 (`lipo -archs`) and
   `codesign --verify --deep --strict` passes on the ad-hoc signature.
3. The DMG shows the app and an Applications link; dragging installs it;
   after *Open Anyway* it runs on a clean macOS 13+ machine.
4. `Info.plist` carries the Cargo version and minimum OS 13.0.
5. Enabling launch at login writes the LaunchAgent and the host starts at
   login; disabling removes it.
6. Replacing the app while it runs does not corrupt data; the next launch
   runs the new version.
7. Nothing ever touches `~/Library/Application Support/SaveScummer`.

---

## 7. Linux

x86_64, any mainstream distribution with glibc 2.35 or newer (Ubuntu 22.04
and later, Fedora, Arch, SteamOS, …). No distro packages (`.deb`/`.rpm`):
one AppImage runs on all of them without installation or root.

### 7.1 Build

Built on **Ubuntu 22.04** (locally or `ubuntu-22.04` in CI). A binary only
runs on systems whose glibc is at least as new as the one it was built
against, so the oldest supported base is the build base.

### 7.2 The app package and release file

The app package is an AppDir, `build/<mode>/package/SaveScummer.AppDir`,
produced by `linuxdeploy` with its Qt plugin. It contains the three
executables, Qt and every library not guaranteed on a base system (per the
AppImage exclude list), both the `xcb` and `wayland` Qt platform plugins,
`packaging/linux/SaveScummer.desktop` and the icon, licenses, notices,
manifest, and checksums.

`packaging/linux/AppRun` is the entry point and dispatches on its first
argument: `host …` runs `SaveScummer.Host`, `cli …` runs `SaveScummer.CLI`,
anything else runs the desktop. So the single file serves as all three
executables.

`appimagetool` turns the AppDir into
`SaveScummer-linux-x86_64-<ver>.AppImage`, using the static AppImage
runtime so it doesn't need `libfuse2` on the user's system. **Unsigned.**

### 7.3 Launch at login

The XDG convention: the host writes and removes
`~/.config/autostart/SaveScummer.desktop` with
`Exec="<AppImage path>" host --minimized --data-dir "<data>"`, through the
same `--autostart on|off` code (§11). The AppImage path comes from the
`APPIMAGE` variable the AppImage runtime sets. Because users download each
version under a new file name, the host refreshes the entry to its own
current path whenever it starts with autostart enabled (§11).

### 7.4 Integration, upgrade, removal

- **No file-manager integration.** There's no cross-desktop mechanism for
  it on Linux. Adding the app to the application menu is left to the
  user's AppImage tool (e.g. Gear Lever, AppImageLauncher), which reads the
  embedded `.desktop` file.
- **Upgrade:** download the new AppImage, quit the old one, start the new
  one; delete the old file. The README gives these steps.
- **Removal:** turn off *Launch at login*, quit, delete the file. The README
  gives the data location (`~/.local/share/SaveScummer`), which removal
  never touches.

### 7.5 Linux acceptance checklist

1. `cargo xtask dist` on Ubuntu 22.04 leaves exactly
   `SaveScummer-linux-x86_64-<ver>.AppImage` in `dist/`.
2. The AppImage runs after `chmod +x` on a stock Ubuntu 22.04 desktop and
   on current Fedora, without `libfuse2`, under both X11 and Wayland.
3. `SaveScummer-….AppImage host --version` and `… cli --version` print the
   Cargo version.
4. Enabling launch at login writes the XDG entry pointing at the AppImage
   and the host starts at login; running a newer AppImage re-points it;
   disabling removes it.
5. Nothing ever touches `~/.local/share/SaveScummer`.

---

## 8. Continuous integration

Two workflows in `.github/workflows/`. Every step is a `cargo xtask`
command.

**`ci.yml`** — branch pushes, pull requests, manual dispatch; not tags. A
concurrency group cancels superseded runs per ref. One required job per
platform — `windows-latest`, `macos-latest` (Apple Silicon),
`ubuntu-22.04` — each: checkout, Rust cache, Qt cache → `cargo xtask setup
qt` (Linux also `setup linux-tools`), `cargo xtask check`,
`cargo xtask build --test`.

**`release.yml`** — `v*` tags only, with a concurrency group that never
cancels. One build job per platform: checkout with full history, caches,
setup commands (Windows `choco install innosetup`; all
`setup cargo-about`), `cargo xtask dist`, upload the one file from `dist/`
(`if-no-files-found: error`). Then one **publish** job (`needs:` all three,
`contents: write`) downloads all three files into `dist/`, fetches the tag,
and runs `cargo xtask publish` with `GH_TOKEN`: one draft release carrying
all platforms.

---

## 9. Versioning and releases

**Cutting a release** — `cargo xtask release <version>` is the only writer
of the version:

1. Validates: three-part version (a leading `v` is accepted), on a branch,
   clean working tree, version differs from the current one, tag
   `v<version>` exists neither locally nor on origin.
2. Rewrites `workspace.package.version` with `toml_edit` (formatting
   preserved) and refreshes `Cargo.lock` (`cargo update --workspace`).
3. Runs `cargo xtask check` (skippable with `--skip-checks` for
   emergencies). On failure the bump is reverted and the tree ends clean.
4. Commits `Release <version>`, creates the annotated tag `v<version>`, and
   pushes branch and tag (`--no-push` stops and prints the two commands).

The tag triggers `release.yml` (§8).

**Publishing** — `cargo xtask publish`, identical locally and in CI. It
checks that `gh` is installed and authenticated, `origin` exists, and tag
`v<version>` exists and points at `HEAD`. It uploads exactly the three
release files for that version from `dist/` and fails if any is missing.
If the release is already published it refuses; if a draft exists it
re-uploads with `--clobber`; otherwise it creates the draft with
`--generate-notes`. It prints the draft URL. A human publishes.

To rebuild a draft from a different commit, delete the tag locally and on
origin and push it again. A published release is never changed; that's
what a new version is for.

---

## 10. Documentation

- **`docs/building.md`** — the how-to, organized per OS: prerequisites, every
  `cargo xtask` command and flag, outputs, the release runbook, CI, dev
  data locations, VS Code tasks. It's the only place flags are listed.
- **`README.md`** — the short version: install instructions per platform
  (including the SmartScreen, *Open Anyway*, and `chmod +x` steps, upgrade
  and removal, and where data lives), the quickest build commands, and a
  link to the guide.
- **This document** — decisions and reasons, not flag lists.

---

## 11. What this plan needs from the application

These behaviors belong to `PLAN.md` and must be specified there; the
infrastructure above depends on them.

1. **`SaveScummer.Host --autostart on|off [--data-dir <dir>]`** sets the
   launch-at-login preference, writes or removes the platform's entry
   (Windows `Run` value, macOS LaunchAgent, Linux XDG autostart), and exits.
   `off` removes the entry only if it points at this host. The in-app
   setting uses the same code, so there is exactly one writer.
2. **With autostart enabled, the host refreshes its entry to its own
   current path on every start** (needed for AppImages, harmless
   elsewhere).
3. **Data directories:** Windows `%LOCALAPPDATA%\SaveScummer`, macOS
   `~/Library/Application Support/SaveScummer`, Linux
   `~/.local/share/SaveScummer`, exposed by the `savescummer-platform`
   crate as the single source every component uses.

---

## 12. Non-goals

- **No code signing** on any platform (§5.4, §6.3, §7.2).
- **No update checks or auto-update.** The app never phones home; users
  install the next release.
- **No Intel Mac, 32-bit, or ARM Linux builds.**
- **No distro packages, Flatpak, Microsoft Store, or Mac App Store.**
- **No shell integration outside Windows.**
- **Never auto-publish; never rename the canonical executables; never clean
  `.runtime/`.**

---

## Appendix — decision index

| # | Decision | Where |
|---|---|---|
| D1 | Four disk roots; `dist/` holds only release files; `.runtime/` never cleaned. | §2 |
| D2 | Version only in `Cargo.toml`; tag = `v<version>`. | §3.1, §9 |
| D3 | All build automation is `cargo xtask`; no shell scripts, no task runners. | §3 |
| D4 | One release file per platform: Inno installer / DMG / AppImage. | §1, §9 |
| D5 | App package assembled under `build/`, staged, swapped atomically. | §2, §3.1 |
| D6 | Processes stopped politely; PID reuse guarded; other hosts left alone by default. | §3.2 |
| D7 | Qt tracks the latest minor (6.11 now); Rust pinned in `rust-toolchain.toml`. | §4 |
| D8 | Windows: CMake picks the newest Visual Studio; no generator logic, no MSVC shim. | §3.3 |
| D9 | Explorer identity only in `identity.h`; classic menu; bridge uses the platform crate. | §5.3 |
| D10 | Per-user Inno installer; integrations checked on first install only. | §5.4 |
| D11 | The host is the only writer of launch-at-login entries on every OS. | §5.4, §11 |
| D12 | macOS: Apple Silicon, macOS 13+, ad-hoc signed, DMG, LaunchAgent, no Finder integration. | §6 |
| D13 | Linux: AppImage built on Ubuntu 22.04, AppRun dispatch, XDG autostart, no file-manager integration. | §7 |
| D14 | Unsigned everywhere; Inno keeps an off-by-default signing block. | §12 |
| D15 | Third-party notices via cargo-about; unlisted licenses fail packaging. | §3.1 |
| D16 | CI: one required job per platform; release builds three files into one draft. | §8 |
| D17 | Draft-first publishing; one `publish` implementation for local and CI. | §9 |
| D18 | User data directories are never touched by install, upgrade, uninstall, or clean. | §1, §11 |
| D19 | No update checks or auto-update. | §12 |
