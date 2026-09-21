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
Rust nor Qt nor the compiler. This is a folder/ZIP distribution, not an installer.

## Prerequisites

- Rust/rustup with the repository's pinned toolchain (`rust-toolchain.toml`).
- Visual Studio C++ build tools and a Windows SDK. Default generator: VS 2019 x64.
- Qt 6.5 or newer, MSVC x64 kit, including Widgets, Network, SVG and Test.
- CMake 3.21 or newer on PATH.

This machine already has these. Scripts also find the local SDK at
`.runtime/Qt/6.5.3/msvc2019_64` and local CMake under `.runtime/qt-tools/cmake/data/bin`.
They do not download SDKs. Rust is also found under the user's `.cargo/bin`.
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
```

`-Test` runs Rust workspace tests using Cargo's test profile and Qt tests using the
selected C++ configuration. The Qt integration test exercises the selected profile's
real host with temporary game data and OS integrations/audio disabled. No test is
required for each edit/run; use `-Test` before distributing a change. The additional
formatting and lint checks are available through `scripts/check.ps1`.

For each mode, output lives under `build/<mode>`:

- `SaveScummer/bin/savescummer-desktop.exe`: portable entry point after packaging.
- `SaveScummer-windows-x64-<mode>.zip`: complete portable folder contents.
- `build-report.json`: last successful command's timings and executable sizes.
- `desktop`: CMake build tree (not the distribution).

The folder includes Qt plugins, Qt/Visual C++ DLLs, license notices, and SHA-256
checksums. Keep it together. Existing packages must be closed, including their
background host, before replacing them. A compile failure stops the build before
deployment, so an old successful package can remain on disk; check the command's
exit status and report timestamp. Packages are unsigned.

## Development data

The dev runner uses `.runtime/dev` for its database/settings/history and disables
global shortcuts, tray, notifications and startup integration. It connects the UI
to the explicitly started host. Host logs are in `build/dev/host.*.log`.
This separates app state from the normal `%LOCALAPPDATA%/SaveScummer` instance,
but configured game paths still refer to real files. Use `-Run -Demo` for simulated
Save/Load operations without touching games. Portable packages use normal app data
by default, including a dev package launched directly.

## VS Code

Open the repository folder in VS Code and install the recommended Microsoft C/C++
extension (PowerShell 7, `pwsh`, must be on PATH). In Run and Debug, select
**Save Scummer: app** and press **F5**. **Save Scummer: demo** runs simulated data.
Both compile Rust and Qt first using the existing dev build. **Ctrl+F5** runs
without debugging; **Ctrl+Shift+B** builds the dev binaries.

The app configuration starts a background host with OS integrations disabled and
data in `.runtime/vscode`, then debugs the Qt executable using MSVC symbols.
Stopping the debug session gracefully shuts down that host. After an interrupted
VS Code session, the next pre-launch task cleans up its recorded host; you can
also run **Save Scummer: stop debug host** from Tasks. Stop the current debug
session before launching another configuration. Configured game paths are real;
use the demo for simulated operations. These configurations debug the Qt UI, not
the separate Rust host. Because Qt dev uses RelWithDebInfo, some locals may be
optimized out and stepping may skip source lines.

The configurations use the local Qt 6.5.3 SDK. If you build with another SDK,
update the PATH entries in `.vscode/launch.json` and the build's QtPrefix together.
The **Save Scummer: package release** task runs the standard release command.

## Lower-level entry points

`scripts/build-desktop.ps1` builds/tests Qt only. Its legacy default directory is
`build/desktop`, configuration Release. `scripts/run-desktop.ps1` runs that legacy
layout without rebuilding. `scripts/package-windows.ps1` deploys already-built
files; it does not compile them. Prefer root `build.ps1` to avoid mixing profiles
or accidentally using an older host during active Rust development.
