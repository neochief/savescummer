# Building on Windows

Run commands from the repository root in PowerShell. `build.ps1` coordinates Cargo
(Rust host and CLI), CMake/MSBuild (C++ Qt desktop), and portable deployment.

```powershell
./build.ps1 dev -Run
./build.ps1 dev -Run -Demo
./build.ps1 release
```

The first command is the closest equivalent of `vite dev`: compile, then open a
development app. There is currently no file watcher or hot module replacement.
After edits, run it again; it closes desktops running from the exact dev output
path, rebuilds changed code, then relaunches. This includes hidden instances and
manual or debugger launches. The desktop gets five seconds to close when it has a
window; hidden or unresponsive desktops are terminated automatically. Packaged
apps and desktops built elsewhere are left running. Recorded development hosts
shut down gracefully so accepted save/restore operations can finish; untracked
hosts are not terminated. Closing the desktop leaves its host running until the
next dev build.

`release` corresponds to `vite build`: produce optimized native executables and a
portable distribution containing the runtime dependencies. Recipients need neither
Rust nor Qt nor the compiler. On Windows this produces both a portable folder/ZIP
and a per-user Inno Setup installer (the primary channel); see
[Building the installer](#building-the-installer).

## Prerequisites

- Rust/rustup with the repository's pinned toolchain (`rust-toolchain.toml`).
- Visual Studio C++ build tools and a Windows SDK. Default generator: VS 2019 x64
  (`build.ps1` falls back to the newest installed Visual Studio when VS 2019 is
  absent).
- Qt 6.5 or newer, MSVC x64 kit, including Widgets, Network, SVG and Test.
  Qt 6.5's headers route two array-iterator helpers through MSVC's
  non-standard `stdext` namespace; VS 2022 17.8 deprecated those helpers and
  later toolsets removed them, so VS 2022 17.8+ and VS 2026 fail with
  `error C2065` / `error C3861: 'stdext'` in `QtCore/qvarlengtharray.h`.
  `apps/desktop/compat/msvc-stdext.h` is force-included for MSVC 19.38+ and
  switches them to Qt's own identity fallback. It can be removed once the
  build moves off Qt 6.5.
- CMake 3.21 or newer on PATH.
- Inno Setup 6.3 or newer for the installer (optional for a portable-only build).

This machine already has these. Scripts also find the local SDK at
`.runtime/Qt/6.5.3/msvc2019_64` and local CMake under `.runtime/qt-tools/cmake/data/bin`.
They do not download SDKs. Rust is also found under the user's `.cargo/bin`.
On a fresh Windows, macOS or Linux machine, `./scripts/setup-qt.ps1` installs the
pinned Qt SDK with aqtinstall (Python 3.8+) into `.runtime/Qt`; the build scripts
then find it without `-QtPrefix`.
For another installation:

```powershell
./build.ps1 dev -Run -QtPrefix C:/Qt/6.8.3/msvc2022_64 -Generator 'Visual Studio 17 2022'
```

Use a compiler compatible with your Qt kit. Changing the CMake generator requires
a fresh CMake build directory. Normal source edits do not require cleaning.

## Development and release

| | dev | release |
|---|---|---|
| Rust | Cargo dev: unoptimized, debugging information | Cargo release: optimized, thin LTO, stripped |
| Qt C++ | RelWithDebInfo: optimized, debugging symbols | Release: optimized, no distributed symbols |
| Qt runtime | Release DLLs | Same release DLLs |
| Main purpose | Fast Rust rebuilds and debugging | Distribution |
| Build trees | `target/debug`, `build/dev/desktop` | `target/release`, `build/release/desktop` |
| Portable package | Optional: add `-Package` | Always produced |
| Installer | — | `release` produces it when Inno Setup is available |

The C++ dev profile deliberately uses release Qt DLLs plus application symbols,
avoiding a dependency on debug-only Qt/Visual C++ runtimes. Symbols are separate
PDB files. Dev packages include host, CLI and desktop PDBs; release packages omit
them. Prebuilt Qt's own debugging symbols are not included in either package.

Both profiles cache compilation. An unchanged Rust crate or C++ translation unit
does not need recompiling. Changes to shared headers or Rust dependencies can
rebuild several dependents. Release optimization/linking usually takes longer;
native code is compiled in dev too. No JavaScript bundling/browser server is involved.
Qt's runtime DLLs provide a substantial fixed size in both packages.

Dev builds allow Cargo to refresh `Cargo.lock` after dependency edits. Release
builds use `--locked` for reproducibility; run a dev build first if dependency
changes require updating the lockfile. Network access is needed for uncached crates.

Cargo's profile defaults are documented in the
[Rust book](https://doc.rust-lang.org/stable/book/ch14-01-release-profiles.html).
This repository adds thin LTO and stripping to release in `Cargo.toml`.

### Measurements on this machine

Observed during build-script verification on September 21, 2026:

| Measurement | Observed |
|---|---|
| Dev build with a Rust source rebuild, first C++ compilation, and ZIP | 17.3 seconds |
| Dev repeat build and ZIP, unchanged source | 5.9 seconds (1.8 seconds compilation checks, 4.1 seconds packaging) |
| Dev repeat build without packaging, including graceful shutdown of the dev session | 2.7 seconds |
| Release Rust recompilation with cached dependencies | 12.4 seconds |
| Release repeat build, Rust tests, rebuilt Qt test, and ZIP | 18.3 seconds |
| Release rebuild after new dependencies were added, with ZIP | 59.2 seconds |
| Latest dev portable folder / ZIP, including PDB symbols | 264.0 / 64.2 MiB |
| Latest release portable folder / ZIP | 40.8 / 18.5 MiB |

These are observed runs, not a controlled benchmark of identical source changes.
Most dependency caches were already populated; a completely clean build and SDK
downloads were not timed. Concurrent Rust development introduced new dependencies
and caused the larger release rebuild. Times and sizes change with source,
dependencies, hardware, antivirus scanning and whether tests/packaging are requested.
Use the generated reports to measure the current version.

## Commands and output

```powershell
./build.ps1 dev           # Just compile both parts
./build.ps1 dev -Test     # Also run Rust workspace and Qt integration tests
./build.ps1 dev -Package  # Portable dev package, including symbols
./build.ps1 release -Test # Optimized package, plus verification
./build.ps1 clean         # Remove regenerable outputs (build/ and dist/)
./build.ps1 clean -Deep   # Also remove the Cargo cache (target/)
```

`-Test` runs Rust workspace tests using Cargo's test profile and Qt tests using the
selected C++ configuration. The Qt integration test exercises the selected profile's
real host with temporary game data and OS integrations/audio disabled. No test is
required for each edit/run; use `-Test` before distributing a change. The additional
formatting and lint checks are available through `scripts/check.ps1`.

`clean` removes `build/` and `dist/` — everything regenerable — and never touches
`.runtime/` (vendored SDKs and development app data survive). `-Deep` also removes
the Cargo cache under `target/`. A recorded development host is stopped
gracefully before its build folder is deleted.

For each mode, intermediates live under `build/<mode>`:

- `desktop`: CMake build tree (not the distribution).
- `build-report.json`: last successful command's timings and executable sizes.
- `build/dev/SaveScummer-windows-x64-<version>-dev.zip`: dev package, including
  PDB symbols.

Release distributables live in `dist/`:

- `dist/SaveScummer-windows-x64/bin/SaveScummer.exe`: portable entry point.
- `dist/SaveScummer-windows-x64-<version>.zip`: complete portable archive whose
  root is a single `SaveScummer-windows-x64/` folder.
- `dist/SaveScummer-windows-x64-<version>-setup.exe`: per-user installer (present
  when Inno Setup is available).

The version is read from the Cargo workspace manifest (`Cargo.toml`); the
release tag must equal `v<version>`.

The folder includes Qt plugins, Qt/Visual C++ DLLs, license notices, and SHA-256
checksums. Keep it together. When replacing a package, the packaging step closes
desktops running from that exact generated folder and asks its background host to
shut down gracefully. The host gets up to 30 seconds to finish accepted work;
hidden, unresponsive, or custom-data-directory processes still running from that
folder are then terminated. Apps running elsewhere are left alone. A compile
failure stops the build before deployment, so an old successful package can remain
on disk; check the command's exit status and report timestamp. Packages are unsigned.

## Building the installer

`./build.ps1 release` builds the Explorer extension, the portable package and,
when Inno Setup is available, the per-user installer. The installer is the
primary Windows channel; the portable ZIP stays as the secondary channel with the
same binaries.

```powershell
./scripts/setup-innosetup.ps1   # once: install Inno Setup 6 with winget
./build.ps1 release             # payload + ZIP + dist/SaveScummer-windows-x64-<version>-setup.exe
```

If `ISCC.exe` is missing, `release` prints a warning and still produces the
portable package. To build only the installer from an existing payload:

```powershell
./scripts/build-installer.ps1
./scripts/build-installer.ps1 -PayloadDirectory dist/SaveScummer-windows-x64
```

The installer is strictly per-user and needs no administrator rights. It installs
to `%LOCALAPPDATA%\Programs\SaveScummer`, registers the Explorer context menu
under `HKCU` (default on, uncheckable) and enables a sign-in entry (task checked
on a first install; the `checkedonce` flag presents it unchecked on upgrades so
a user who disabled startup in-app is not opted back in). When selected, setup
writes the same `HKCU\...\Run\SaveScummer` value the app writes, so autostart
works immediately. The app itself keeps its preference off by default: on its
first start it reads that registry value and adopts its current state, after
which the in-app **Launch on startup** checkbox can change it. Uninstalling
removes the entry only when it still references the installed copy. Before
replacing files the installer asks a running installed host to shut down
gracefully, so an accepted save/restore operation can finish. Silent install and
uninstall:

```powershell
.\SaveScummer-windows-x64-0.1.0-setup.exe /SILENT /SUPPRESSMSGBOXES
# Keep the default-on Explorer registration but skip the sign-in entry:
.\SaveScummer-windows-x64-0.1.0-setup.exe /SILENT /SUPPRESSMSGBOXES /MERGETASKS="!startup"
"%LOCALAPPDATA%\Programs\SaveScummer\unins000.exe" /SILENT
```

Uninstalling removes only the installed application folder and the per-user
registrations. It deliberately leaves `%LOCALAPPDATA%\SaveScummer` (the entire
backup history) in place and never deletes your backups. Release installers are
unsigned; Windows SmartScreen may warn on first run. Code signing is left as a
slot in `packaging/windows/installer/savescummer.iss` (define `SignedBuild` and a
`SignTool`).

Windows cannot overwrite the Explorer extension DLL while a running
`explorer.exe` has it loaded. Updating or uninstalling an installation whose
extension is registered therefore completes the DLL replacement on the next
restart and Inno Setup asks for one; the new or removed context-menu entries
appear only after Explorer restarts. A first install with the extension task
disabled never registers the DLL, so it needs no restart.

## Publishing a GitHub release

Releases are published to GitHub Releases from a `v<version>` tag, always as a
draft for review before it becomes public:

```powershell
git tag v0.1.0
git push origin v0.1.0
./build.ps1 release            # produces dist/SaveScummer-windows-x64-<version>.zip
./scripts/release-github.ps1   # creates a DRAFT release with the installer
```

Or cut the whole release in one command — it bumps `Cargo.toml`, refreshes
`Cargo.lock`, runs `scripts/check.ps1`, commits "Release 0.2.0", tags `v0.2.0`
and pushes both:

```powershell
./scripts/release.ps1 0.2.0
./scripts/release.ps1 0.2.0 -SkipChecks   # skip scripts/check.ps1
./scripts/release.ps1 0.2.0 -NoPush       # commit and tag locally only
```

The release script requires the GitHub CLI (`gh`); run `gh auth login` once.
It verifies that the tag matches the `Cargo.toml` version and points at HEAD,
uploads the installer as the single release asset, and creates a draft with
auto-generated notes. Pass `-IncludePortable` to also attach the portable
archive, and `-IncludeChecksums` to add `.sha256` sidecars for every attached
asset. When Inno Setup was unavailable, `-AllowMissingInstaller` publishes the
portable archive instead (a warning is printed). Review the release page and click
**Publish** to make it public. Rebuilding and rerunning the script updates the existing draft with
`--clobber`; an already-published release is never modified. The release workflow
performs the same steps on a per-OS matrix, so the local and CI paths stay
identical.

GitHub always adds `Source code (zip)` and `Source code (tar.gz)` archives to a
release; they cannot be disabled or deleted. When the release notes name the
installer explicitly, users are less likely to download those archives instead.

## Continuous integration

`.github/workflows/ci.yml` runs on every push, pull request and manual dispatch:

- **Rust checks (Windows)** — `scripts/check.ps1` (fmt, clippy, tests, build).
  Windows is the qualified platform; this job must pass.
- **Qt desktop (Windows)** — builds the Rust host/CLI and the Qt desktop with
  tests: `./build.ps1 dev -Test`. The runner image ships a newer Visual Studio,
  so `build.ps1` picks the newest installed generator when the repository
  default (VS 2019) is absent.
- **Rust checks (ubuntu-latest, macos-latest), non-blocking** — `cargo fmt`,
  `cargo check` and `cargo test` for the portable Rust crates. These measure
  porting progress before those platforms are qualified.

CI installs Qt with `scripts/setup-qt.ps1` and caches `.runtime/Qt`; the cache
key hashes the script, so changing the pinned Qt version or kit invalidates it.

`.github/workflows/release.yml` runs for a `v<version>` tag. The Windows job runs
`./build.ps1 release -Test` and uploads the portable ZIP and the installer; the
publish job downloads every platform's artifacts into `dist/`, so the release
script attaches them all to one draft:

```powershell
git tag v0.1.0
git push origin v0.1.0
```

Review the draft and click **Publish**; nothing is published automatically. A
failed run is retried with GitHub's **Re-run** button; changed content means
releasing a new version. macOS and Linux build jobs join the same draft as those
platforms qualify; code signing and notarization are still open.

## Development data

The dev runner uses `.runtime/dev` for its database/settings/history and connects
the UI to the explicitly started host. Host logs are in `build/dev/host.*.log`.
This separates app state from the normal `%LOCALAPPDATA%/SaveScummer` instance,
but configured game paths still refer to real files. Use `-Run -Demo` for simulated
Save/Load operations without touching games. Portable packages use normal app data
by default, including a dev package launched directly.

Unlike automated tests, the interactive dev runner keeps OS integrations enabled so
the app behaves like the real one: the host owns the tray icon, the global
Ctrl+F5/Ctrl+F9 shortcuts and failure notifications. Because only one host can own
those, `dev -Run` first stops every other SaveScummer host owned by the current user.
Each host is asked to shut down gracefully so accepted save/restore work can finish;
it is terminated only if it refuses, which may leave an operation needing recovery.
Pass `-KeepOtherHosts` to leave them running (the tray and global shortcuts then
belong to whichever host registered first). The dev build also removes a SaveScummer
autostart entry that points at a development build, so toggling **Launch on startup**
in a dev session is cleaned up on the next dev build; an entry pointing at a packaged
or installed host is never touched.

## VS Code

Open the repository folder in VS Code and install the recommended Microsoft C/C++
extension (PowerShell 7, `pwsh`, must be on PATH). In Run and Debug, select
**SaveScummer: app** and press **F5**. **SaveScummer: demo** runs simulated data.
Both compile Rust and Qt first using the existing dev build. **Ctrl+F5** runs
without debugging; **Ctrl+Shift+B** builds the dev binaries.

The app configuration starts a background host with OS integrations disabled and
data in `.runtime/vscode`, then debugs the Qt executable using MSVC symbols.
Stopping the debug session gracefully shuts down that host. After an interrupted
VS Code session, the next pre-launch task cleans up its recorded host; you can
also run **SaveScummer: stop debug host** from Tasks. Stop the current debug
session before launching another configuration. Configured game paths are real;
use the demo for simulated operations. These configurations debug the Qt UI, not
the separate Rust host. Because Qt dev uses RelWithDebInfo, some locals may be
optimized out and stepping may skip source lines.

The configurations use the local Qt 6.5.3 SDK. If you build with another SDK,
update the PATH entries in `.vscode/launch.json` and the build's QtPrefix together.
The **SaveScummer: package release** task runs the standard release command.

## Lower-level entry points

`scripts/build-desktop.ps1` builds/tests Qt only; it accepts `-Mode dev|release`
(default `dev`) and selects `build/<mode>/desktop` with the matching
configuration unless overridden. `scripts/package-windows.ps1` deploys
already-built files; it does not compile them. `scripts/build-installer.ps1`
compiles the Inno Setup definition from a staged release payload.
`scripts/build-explorer.ps1` builds and COM-tests the Explorer extension.
`scripts/setup-qt.ps1` installs the pinned Qt SDK on Windows, macOS and Linux
and is a no-op when that kit is already present.
Prefer root `build.ps1` to avoid mixing profiles or accidentally using an older
host during active Rust development.
