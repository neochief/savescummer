# Save Scummer — Build & Release

I want building, packaging and releasing the app to be boring: one command to build, one command to cut a release, and one file per platform for users to download.

This plan covers only the machinery around the app: builds, packaging, installers, CI and releases. App behavior lives in PLAN.md; the few things this plan needs from the app are listed under WHAT THE APP MUST PROVIDE.

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

- **One build tool, in Rust.** All automation is `cargo xtask`: no PowerShell, bash or Python scripts, no just/make. It runs the same on every OS and needs nothing beyond the Rust toolchain we already require.
- **CI runs what developers run.** Workflows only call `cargo xtask`, so a CI failure can always be reproduced locally.
- **One version, in `Cargo.toml`.** Every binary, the installer, the bundle and the git tag read it, so they can't drift apart.
- **One file per platform per release,** in the friendliest format that platform has. Users never have to pick, and nothing else is uploaded.
- **Fixed executable names.** Platforms change how the programs are packaged, never what they're called.
- **Nothing is downloaded silently.** Tools come only from `cargo xtask setup …`, at pinned versions, so developers and CI build with the same tools.
- **User data is sacred.** No install, upgrade, uninstall or clean ever touches the app's data directory.
- **Only a human publishes.** Tooling makes draft releases; I look at them and press publish.
- **Every failure says what to run next,** e.g. "Qt 6.11.2 not found — run `cargo xtask setup qt`".


## WHAT USERS GET

The app is three programs, plus a shell extension on Windows:

- **Desktop** — C++/Qt 6 client
- **Host** — Rust background host (SQLite, monitoring, operations)
- **CLI** — Rust command-line client
- **Explorer extension** — context menu (Windows only)

The build always produces `SaveScummer`, `SaveScummer.Host` and `SaveScummer.CLI` (`.exe` on Windows).

| Platform | Release file | Why this format |
| --- | --- | --- |
| Windows 10/11 x64 | `SaveScummer-windows-x64-<ver>-setup.exe` | What Windows users expect; installs per-user, no admin |
| macOS 13+, Apple Silicon | `SaveScummer-macos-arm64-<ver>.dmg` | The standard drag-to-Applications install |
| Linux x86_64, glibc 2.35+ | `SaveScummer-linux-x86_64-<ver>.AppImage` | One file runs on every distro, no root, nothing to install |

What ends up on the user's machine:

**Windows** — `%LOCALAPPDATA%\Programs\SaveScummer\`:

```text
bin\SaveScummer.exe              desktop
bin\SaveScummer.Host.exe         host
bin\SaveScummer.CLI.exe          CLI
bin\savescummer-explorer.dll     Explorer extension
```

**macOS** — `SaveScummer.app` in Applications:

```text
Contents/MacOS/SaveScummer       desktop
Contents/MacOS/SaveScummer.Host  host
Contents/MacOS/SaveScummer.CLI   CLI
```

**Linux** — the AppImage itself is the install and acts as all three programs:

```text
<file>.AppImage                  desktop
<file>.AppImage host …           host
<file>.AppImage cli …            CLI
```

All release files are named `SaveScummer-<os>-<arch>-<version>[-<suffix>].<ext>`, built by one function in xtask. Arch tags follow each OS's habit (`x64` on Windows, `arm64`/`x86_64` elsewhere) and are never unified.


## XTASK

`xtask/` is a workspace crate (`publish = false`), run as `cargo xtask` through the alias in `.cargo/config.toml` (`xtask = "run --package xtask --"`). Platform code lives in `windows.rs`, `macos.rs` and `linux.rs`, each compiled only on its OS; shared modules never contain platform logic.

Commands:

- `check` — the quality gate (see CHECK).
- `build [--release] [--test] [--package]` — build Rust and the desktop. `--test` runs Rust and desktop tests. `--package` assembles the APP PACKAGE; `--release` always does.
- `run [--demo] [--stop-other-hosts]` — dev build, then the dev host and a desktop connected to it. `--demo` uses simulated operations.
- `host start [--demo]` / `host stop` — just the dev host; used by `run` and by VS Code debugging.
- `dist` — `build --release --test`, then the platform's release file in `dist/`.
- `clean [--deep]` — stop output processes, remove `build/` and `dist/` (`--deep` also `target/`). Works without Qt or any other tool, so a broken setup can always be cleaned.
- `release <version>` — cut a release (see RELEASING).
- `publish` — upload `dist/` to a draft GitHub release (see RELEASING).
- `setup qt|inno|linux-tools|cargo-about` — install a pinned tool (see TOOLCHAINS).
- `explorer build|register|unregister|check [--dev]` — Windows only (see Windows).
- `catalog [--check] [--strict]` — the game catalog; owned by PLAN-CATALOG.md.

Every command validates its inputs up front.

`cargo xtask run`:

1. Does a dev build.
2. Starts the dev host hidden, with `--data-dir .runtime/dev`, and records it in `build/dev/session.json`.
3. Waits up to 30 s for the host's `"ready":true` line.
4. Starts the desktop connected to that host.

The dev host uses its own data, so development never touches the real app's data. An installed host is left running, and xtask warns that the dev instance won't own the tray icon or global shortcuts. `--stop-other-hosts` stops it instead (gracefully).

`build/<mode>/build-report.json` records each build's steps, timings, and binary paths and sizes.


## VERSION

The version lives in `Cargo.toml` → `[workspace.package] version`. The git tag is always `v<version>`. Only `cargo xtask release` changes it.

Everything else reads it:

- **xtask** parses `Cargo.toml` with the `toml` crate — no pattern matching.
- **CMake** reads it before `project()` with a regex anchored to a line start (`"\n"`, since CMake's `^` means start of file), so an inline dependency like `foo = { version = "1.2.3" }` never matches. This means `[workspace.package]` must stay above any dependency written as its own table (`[dependencies.foo]` with `version = …` on its own line).
- **Windows version resources** come from `winresource` in `build.rs` for the host and CLI, and from CMake-filled `.rc.in` templates for the desktop and Explorer DLL. `build.rs` must print `cargo:rerun-if-changed=Cargo.toml`, or a version bump ships a stale resource.
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
|-- packaging/
|   |-- licenses/        Qt license texts, shipped on every platform
|   |-- windows/         savescummer.iss
|   |-- macos/           Info.plist.in
|   `-- linux/           AppRun, SaveScummer.desktop
|-- assets/              icons, sounds, asset tooling
|-- target/
|-- build/
|   |-- dev/             desktop build, APP PACKAGE, session.json, logs, build-report.json
|   |-- release/         desktop and Explorer builds, APP PACKAGE, build-report.json
|   `-- tmp/             scratch; the test temp dir is emptied each run
|-- dist/
`-- .runtime/
    |-- Qt/<ver>/<kit>/  Qt SDK
    |-- tools/           aqtinstall venv, Inno Setup, linuxdeploy, appimagetool
    `-- dev/             dev app data (the dev host's --data-dir)
```

There is no `scripts/` directory. `.gitignore` covers the four roots plus `*.db`, `*.db-shm` and `*.db-wal`.


## APP PACKAGE

The APP PACKAGE is the assembled, runnable app under `build/<mode>/package/`: a folder on Windows, a `.app` on macOS, an AppDir on Linux. It's what the release file wraps, so what I test locally is exactly what ships.

Every package contains:

- the three executables and the Qt libraries they need
- the Qt license texts from `packaging/licenses/`
- `THIRD-PARTY-LICENSES.html`
- `.savescummer-package.json` (mode, version, platform, Qt version, configuration, creation time), which marks it as generated output
- `SHA256SUMS.txt`

`THIRD-PARTY-LICENSES.html` is generated by cargo-about from the crates the host and CLI link. A crate under a license not in `about.toml` fails packaging, so licensing is checked on every build rather than at release time.

Release packages have no debug symbols; dev packages keep them.

A package is assembled in a fresh `.staging-<uuid>` sibling and swapped into place only when complete, after stopping anything running from the old one. A failed build never damages the existing package.


## STOPPING PROCESSES

A running host may be in the middle of a save, and Windows can't replace a running executable. So xtask stops processes politely, always with the same routine:

1. Ask desktops to close.
2. Ask hosts to shut down through their sibling CLI (`--data-dir <dir> shutdown`, up to 30 s), so an accepted operation finishes.
3. Terminate whatever is still running.

It's used for:

- **Output processes.** Before rebuilding or cleaning, anything running from `build/` or `dist/` is stopped. Never anything from `target/` or `.runtime/`.
- **The dev host.** `build/dev/session.json` records its PID, start time and path. It's stopped only if all three still match, so a reused PID is never killed. The session file is deleted last.
- **Other hosts,** only with `run --stop-other-hosts`.

Every dev build also removes launch-at-login entries pointing into `target/` or `build/`, so signing in never starts a debug host. Other entries are the user's and stay.


## TOOLCHAINS

**Rust** is pinned in `rust-toolchain.toml`: channel, `minimal` profile, rustfmt, clippy, and the `aarch64-apple-darwin` target. rustup applies it everywhere. Bumping it is a deliberate one-line commit.

**Qt follows the latest minor release,** currently 6.11, with the exact patch pinned in one xtask constant. Open-source Qt only patches its newest minor, so staying current is the only way to get fixes:

- patches are taken promptly
- a new minor is adopted within about two months, in its own commit, once all three platforms build and pass tests
- the app uses only Widgets, Network and Svg, the most stable parts of Qt, so a bump is normally just a rebuild

**C++:**

- Windows: MSVC from Visual Studio 2022+. No CMake generator is passed, so CMake picks the newest Visual Studio; we don't manage generators or shim MSVC.
- macOS: Apple Clang from Xcode 15+
- Linux: GCC 11+
- CMake 3.21+, from PATH (Visual Studio, Xcode command-line tools or the distro provide it)

**Tools** are installed only by their setup command, into `.runtime/`, and xtask uses them from there:

- `setup qt` — the pinned Qt kit via aqtinstall (in a venv under `.runtime/tools/`; needs Python 3.9+) into `.runtime/Qt/<version>/`. Safe to re-run. CI caches it keyed on the pin.
- `setup inno` — the pinned Inno Setup 6 installer from jrsoftware.org, checked by SHA-256, installed silently per-user into `.runtime/tools/inno-setup/`. Not winget or Chocolatey: they aren't reliably on CI runners and don't pin versions.
- `setup linux-tools` — linuxdeploy with its Qt plugin, and appimagetool; pinned release, checked by SHA-256.
- `setup cargo-about` — pinned version.

### The desktop build

xtask drives CMake:

- Configures `build/<mode>/desktop` with the Qt kit as `CMAKE_PREFIX_PATH`, `RelWithDebInfo` for dev and `Release` for release.
- Builds `savescummer-desktop`, and `desktop-tests` with `--test`.
- Runs ctest with `SAVESCUMMER_TEST_HOST` set to the freshly built host (Linux CI adds `QT_QPA_PLATFORM=offscreen`). On failure it prints the JUnit XML.

The CMake project sets C++17, `AUTOMOC`/`AUTORCC`, `find_package(Qt6 6.11 REQUIRED COMPONENTS Widgets Network Svg)`, the `SaveScummer` output name, `install()` rules, and `qt_generate_deploy_app_script` for deploying Qt. Test screenshots go to `build/<mode>/desktop/screenshots`, never the source tree.


## CHECK

`cargo xtask check` runs, stopping at the first failure:

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets --locked -- -D warnings`
3. `cargo test --workspace --locked`
4. `cargo build --workspace --locked`
5. `cargo xtask catalog --check`

Desktop tests are not part of `check`; they run with `build --test`.


## Windows

### App package

`build/<mode>/package/SaveScummer-windows-x64/`, containing:

- the `cmake --install` deploy, with Qt DLLs in `bin/`
- `SaveScummer.Host.exe` and `SaveScummer.CLI.exe`
- the Visual C++ runtime DLLs, copied next to the app from the newest Visual Studio (found with vswhere), so users don't need to install a redistributable
- the Explorer DLL (release only)
- `README.txt` with the LGPL attribution, source links, and the note that the Qt DLLs may be replaced with interface-compatible builds
- dev only: the PDBs

Every binary gets a version resource (see VERSION) and `assets/icon.ico`.

### Explorer extension

A classic COM `IContextMenu` handler, registered per-user under `HKCU\Software\Classes`, so no admin is needed. On Windows 11 it shows under *Show more options*: the modern `IExplorerCommand` menu needs MSIX and code signing, which we don't have.

Its identity lives only in `integrations/windows-explorer/identity.h`. xtask parses it and passes it to Inno Setup (`/D` defines) and to its own registration code:

| | Release | Dev |
| --- | --- | --- |
| CLSID | `{3F8F42CE-463F-41B6-98D1-8C8D16B88931}` | `{43BFBA41-D0AB-44D3-A5D6-600EB5C74D18}` |
| Name | `SaveScummer` | `SaveScummerDev` |

The separate dev identity means a dev registration never shadows the installed one.

- `explorer build` builds the DLL and runs its COM tests.
- `explorer register|unregister [--dev]` writes or removes the per-user registration: `InprocServer32` with `ThreadingModel=Apartment`, plus `Directory\shellex\ContextMenuHandlers\<name>`.
- `explorer check` exercises the real handler without registering anything.

The extension's Rust bridge finds the host's data directory through the `savescummer-platform` crate — the same code the host uses — never by joining a folder name itself.

Explorer keeps the DLL loaded, so it can't be overwritten in place: the installer replaces it on restart, and a fresh Explorer process picks up the change.

### Installer

`packaging/windows/savescummer.iss`, a per-user Inno Setup installer to `%LOCALAPPDATA%\Programs\SaveScummer`. Per-user means no admin prompt. Its payload is exactly the release APP PACKAGE. Only `cargo xtask dist` compiles it, passing the version, payload, output and Explorer identity as `/D` defines.

- **Stable `AppId`** and `PrivilegesRequired=lowest`, so upgrades replace in place without admin.
- **Two tasks, "Explorer menu" and "Launch at sign-in",** both `checkedonce` with `UsePreviousTasks=yes`: checked on first install, unchecked on upgrade, so a user who turned one off is never opted back in.
- **Launch at sign-in** is set by running the installed `SaveScummer.Host.exe --autostart on`, so the host stays the only writer of that entry. The uninstaller runs `--autostart off`, which removes the entry only if it points at this install.
- **Before replacing files,** `PrepareToInstall` runs the installed `SaveScummer.CLI.exe --no-start shutdown`, so an in-flight save finishes. Inno's Restart Manager (`CloseApplications=yes`) closes the desktop.
- **The Explorer DLL** is its own `[Files]` entry with `restartreplace`.
- **Registry writes are HKCU only;** the CLSID subtree uses `uninsdeletekey`.
- **`%LOCALAPPDATA%\SaveScummer` is never touched.**
- **Unsigned.** SmartScreen shows "More info → Run anyway"; the README and release notes say so. An off-by-default `SignedBuild` block (`SignTool`, `SignedUninstaller`) lets signing be switched on later without other changes.

`dist` fails if Inno Setup is missing (there's nothing to ship without it), or if the payload lacks `bin/SaveScummer.exe` or the Explorer DLL.

### Done when

1. `clean` stops recorded and output processes and removes `build/` and `dist/`; `--deep` also removes `target/`; `.runtime/` is untouched.
2. `run` builds and runs against `.runtime/dev`, recording `build/dev/session.json`; an installed host keeps running unless `--stop-other-hosts` is passed.
3. `dist` leaves exactly one file in `dist/`, the `-setup.exe`. The release package under `build/release/package/` has `SHA256SUMS.txt` and `THIRD-PARTY-LICENSES.html`, and no PDBs.
4. CMake configure shows the Cargo version; screenshots land in `build/<mode>/desktop/screenshots`.
5. Installing needs no admin, puts the binaries under `%LOCALAPPDATA%\Programs\SaveScummer\bin`, and the Explorer task adds the menu (under *Show more options* on Windows 11).
6. The sign-in task is checked on first install and unchecked on upgrade. When it's checked, sign-in starts the host minimized.
7. Installing over a running host shuts it down gracefully first. A DLL update completes after restart.
8. Uninstalling removes the app, its registrations and its own sign-in entry, never `%LOCALAPPDATA%\SaveScummer`.


## macOS

Apple Silicon (M1 or later) only, macOS 13+ (Qt 6.11's minimum). Intel Macs aren't supported, and xtask refuses to build on one.

### Build

Built on an Apple Silicon Mac. The whole app agrees on its minimum OS:

- Rust: `--target aarch64-apple-darwin`, `MACOSX_DEPLOYMENT_TARGET=13.0`
- CMake: `CMAKE_OSX_ARCHITECTURES=arm64`, `CMAKE_OSX_DEPLOYMENT_TARGET=13.0`
- `Info.plist`: `LSMinimumSystemVersion` 13.0

### App package

`build/<mode>/package/SaveScummer.app`:

- Bundle ID `com.savescummer.SaveScummer`, fixed forever, because macOS keys permissions and settings on it.
- The three executables side by side in `Contents/MacOS/`.
- `Contents/Info.plist` from `packaging/macos/Info.plist.in`: `CFBundleShortVersionString` and `CFBundleVersion` from the Cargo version, minimum OS 13.0, the icon.
- Qt frameworks deployed by `macdeployqt`; licenses, notices, manifest and checksums in `Contents/Resources/`.
- Ad-hoc signed (`codesign --force --deep --sign -`) as the last step, because Apple Silicon won't run binaries without a valid signature and `macdeployqt` breaks the linker's signatures when it rewrites library paths.

### Release file

- A DMG made with `hdiutil create -format UDZO` from a folder holding the app and an `Applications` link, so installing is drag-and-drop.
- Never a zip: PowerShell- and .NET-style zips break the symlinks inside Qt frameworks.
- Unsigned and not notarized, so macOS blocks the first launch; the user opens it via *System Settings → Privacy & Security → Open Anyway*. The README and release notes show this with screenshots.

### Integration

- **Launch at login:** the host writes and removes `~/Library/LaunchAgents/com.savescummer.host.plist` (pointing at the host inside the bundle, with `--minimized --data-dir <data>`) and loads or unloads it with `launchctl`, through the shared `--autostart on|off` code. macOS shows its "Background item added" notice.
- **No Finder integration:** the only sanctioned route is a FinderSync extension, which needs proper signing.

### Upgrade and removal

The README gives both:

- **Upgrade:** quit (menu-bar icon → Quit), drag the new app over the old one. Replacing a running app is safe on macOS, since running processes keep the old files, and the next launch runs the new version.
- **Remove:** turn off launch at login, quit, drag to Trash. `~/Library/Application Support/SaveScummer` is never touched.

### Done when

1. `dist` on an Apple Silicon Mac leaves exactly `SaveScummer-macos-arm64-<ver>.dmg` in `dist/`; on an Intel Mac it refuses with a clear message.
2. Every binary in the bundle is arm64 (`lipo -archs`), and `codesign --verify --deep --strict` passes.
3. The DMG shows the app and an Applications link; dragging installs it; after *Open Anyway* it runs on a clean macOS 13+ machine.
4. `Info.plist` has the Cargo version and minimum OS 13.0.
5. Enabling launch at login writes the LaunchAgent and the host starts at login; disabling removes it.
6. Replacing the app while it runs doesn't corrupt data, and the next launch runs the new version.
7. Nothing ever touches `~/Library/Application Support/SaveScummer`.


## Linux

x86_64, any mainstream distro with glibc 2.35+ (Ubuntu 22.04+, Fedora, Arch, SteamOS…). No `.deb` or `.rpm`: one AppImage runs on all of them, without installing or root.

### Build

Built on Ubuntu 22.04, locally or `ubuntu-22.04` in CI. A binary only runs on a glibc at least as new as the one it was built against, so the oldest supported system has to be the build system.

### App package and release file

The APP PACKAGE is `build/<mode>/package/SaveScummer.AppDir`, made by `linuxdeploy` with its Qt plugin. It contains:

- the three executables
- Qt and every library not guaranteed on a base system (per the AppImage exclude list)
- both the `xcb` and `wayland` Qt platform plugins, so it runs under X11 and Wayland
- `packaging/linux/SaveScummer.desktop` and the icon
- licenses, notices, manifest and checksums

`packaging/linux/AppRun` is the entry point and dispatches on its first argument, so one file serves as all three programs:

- `host …` runs `SaveScummer.Host`
- `cli …` runs `SaveScummer.CLI`
- anything else runs the desktop

`appimagetool` turns the AppDir into `SaveScummer-linux-x86_64-<ver>.AppImage` using the static AppImage runtime, so users don't need `libfuse2`. Unsigned.

### Integration

- **Launch at login:** the host writes and removes `~/.config/autostart/SaveScummer.desktop` with `Exec="<AppImage path>" host --minimized --data-dir "<data>"`, through the shared `--autostart on|off` code. The path comes from `$APPIMAGE`, which the AppImage runtime sets. Each version is downloaded under a new file name, so the host re-points the entry to itself on every start.
- **No file-manager integration:** there's no cross-desktop way to do it. Adding the app to the menu is left to the user's AppImage tool (Gear Lever, AppImageLauncher…), which reads the embedded `.desktop` file.

### Upgrade and removal

The README gives both:

- **Upgrade:** download the new AppImage, quit the old one, start the new one, delete the old file.
- **Remove:** turn off launch at login, quit, delete the file. `~/.local/share/SaveScummer` is never touched.

### Done when

1. `dist` on Ubuntu 22.04 leaves exactly `SaveScummer-linux-x86_64-<ver>.AppImage` in `dist/`.
2. After `chmod +x`, it runs on stock Ubuntu 22.04 and current Fedora without `libfuse2`, under both X11 and Wayland.
3. `….AppImage host --version` and `….AppImage cli --version` print the Cargo version.
4. Enabling launch at login writes the XDG entry pointing at the AppImage and the host starts at login. Running a newer AppImage re-points the entry, and disabling removes it.
5. Nothing ever touches `~/.local/share/SaveScummer`.


## CI

Two workflows in `.github/workflows/`, every step a `cargo xtask` command.

**ci.yml** — so nothing merges that breaks a platform:

- Runs on branch pushes, pull requests and manual runs; not tags.
- A concurrency group per ref cancels superseded runs.
- One required job each on `windows-latest`, `macos-latest` (Apple Silicon) and `ubuntu-22.04`: checkout, Rust cache, Qt cache, `setup qt` (Linux also `setup linux-tools`), `check`, `build --test`.

**release.yml** — builds all three files into one draft release:

- Runs on `v*` tags only, with a concurrency group that never cancels, so a release is never half-built.
- One build job per platform: checkout with full history, caches, `setup qt` and `setup cargo-about` (plus `setup inno` on Windows, `setup linux-tools` on Linux), `dist`, upload its one file from `dist/` (`if-no-files-found: error`).
- A final publish job (`needs:` all three, `contents: write`) downloads the three files into `dist/`, fetches the tag and runs `cargo xtask publish` with `GH_TOKEN`.


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
2. Checks that exactly the three release files for that version are in `dist/`.
3. If the release is already published, refuses. If a draft exists, re-uploads the files (`--clobber`). Otherwise creates a draft with `--generate-notes`.
4. Prints the draft URL.

Tooling only ever makes drafts; I publish by hand. A published release is never changed — that's what a new version is for. To rebuild a draft from a different commit, delete the tag locally and on origin and push it again.


## WHAT THE APP MUST PROVIDE

These belong in PLAN.md, but this plan depends on them:

- **`SaveScummer.Host --autostart on|off [--data-dir <dir>]`** sets the launch-at-login preference, writes or removes the platform's entry, and exits:
  - Windows: a `Run` value
  - macOS: the LaunchAgent
  - Linux: the XDG autostart entry

  `off` only removes an entry pointing at this host. The in-app checkbox uses the same code, so the host is the only writer and the entry can't get out of sync.
- **With autostart on, the host re-points its entry to its own path on every start** — needed for AppImages, harmless elsewhere.
- **Data directories come from the `savescummer-platform` crate,** the single source every component uses:
  - Windows: `%LOCALAPPDATA%\SaveScummer`
  - macOS: `~/Library/Application Support/SaveScummer`
  - Linux: `~/.local/share/SaveScummer`


## DOCS

- `docs/building.md` — the how-to per OS: prerequisites, every `cargo xtask` command and flag, outputs, the release runbook, CI, dev data locations, VS Code tasks.
- `README.md` — the short version, linking to the guide:
  - install, upgrade and removal per platform, including the SmartScreen, *Open Anyway* and `chmod +x` steps
  - where data lives
  - the quickest build commands

Whenever the build changes, this plan and `docs/building.md` change with it.


## NOT DOING

- Code signing on any platform — it costs money and yearly upkeep; the workarounds above are documented instead.
- Update checks or auto-update — the app never phones home; users install the next release.
- Intel Mac, 32-bit or ARM Linux builds.
- Distro packages, Flatpak, Microsoft Store, Mac App Store — the three release files cover everyone.
- Shell integration outside Windows — see macOS and Linux above.
- Auto-publishing releases — a human always publishes.
- Renaming the executables, or cleaning `.runtime/`.
