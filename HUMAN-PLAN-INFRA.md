# Save Scummer — Build & Release

I want building, packaging and releasing the app to be boring: one command to build, one command to cut a release, and one file per platform for users to download.

This plan covers only the machinery around the app. App behavior lives in PLAN.md.

This is the target design. Where the code disagrees, the code changes.

Windows is the platform that should already work. macOS and Linux are planned but not implemented yet. Even so, every piece of the infrastructure is built with them in mind: shared code stays platform-neutral, platform logic goes in its own module, and nothing assumes Windows paths, tools or file names.

Build it in this order: the shared parts (XTASK, VERSION, DISK, APP PACKAGE, TOOLCHAINS), then Windows end to end, then macOS and Linux. CI gets a job for each platform as it lands. The work is done when every platform's DONE WHEN list passes.


## THE PIECES

The app is three programs, plus a shell extension on Windows:

- **Desktop** — C++/Qt 6 client
- **Host** — Rust background host (SQLite, monitoring, operations)
- **CLI** — Rust command-line client
- **Explorer extension** — context menu (Windows only)

The build always produces `SaveScummer`, `SaveScummer.Host` and `SaveScummer.CLI` (Windows adds `.exe`). Platforms change how they're packaged, never what they're called.

Each release has exactly one file per platform. Here's what the user ends up with:

**Windows 10/11 x64** — `SaveScummer-windows-x64-<ver>-setup.exe` (Inno Setup) installs `%LOCALAPPDATA%\Programs\SaveScummer\`:

```text
bin\SaveScummer.exe              desktop
bin\SaveScummer.Host.exe         host
bin\SaveScummer.CLI.exe          CLI
bin\savescummer-explorer.dll     Explorer extension
```

**macOS 13+, Apple Silicon** — `SaveScummer-macos-arm64-<ver>.dmg`. The user drags `SaveScummer.app` to Applications:

```text
Contents/MacOS/SaveScummer       desktop
Contents/MacOS/SaveScummer.Host  host
Contents/MacOS/SaveScummer.CLI   CLI
```

**Linux x86_64, glibc ≥ 2.35** — `SaveScummer-linux-x86_64-<ver>.AppImage`. The file itself is the install and acts as all three programs:

```text
<file>.AppImage                  desktop
<file>.AppImage host …           host
<file>.AppImage cli …            CLI
```

There's no Explorer extension equivalent on macOS or Linux.

All release files are named `SaveScummer-<os>-<arch>-<version>[-<suffix>].<ext>`. Arch tags follow each OS's habit (`x64` on Windows, `arm64`/`x86_64` elsewhere).


## XTASK

All build and release automation is one Rust program, `cargo xtask`. No PowerShell, bash or Python scripts, no just/make. CI runs the same commands developers run.

- `cargo xtask check` — the quality gate
- `cargo xtask build [--release] [--test] [--package]` — build Rust and the desktop; optionally run tests and assemble the APP PACKAGE (always assembled with `--release`)
- `cargo xtask run [--demo] [--stop-other-hosts]` — dev build, then start the dev host and the desktop (see below)
- `cargo xtask host start|stop` — just the dev host (for VS Code debugging)
- `cargo xtask dist` — release build with tests, then the platform's release file in `dist/`
- `cargo xtask clean [--deep]` — stop running output, remove `build/` and `dist/` (`--deep` also `target/`); works even without Qt or other tools installed
- `cargo xtask release <version>` — cut a release
- `cargo xtask publish` — upload `dist/` to a draft GitHub release
- `cargo xtask setup qt|inno|linux-tools|cargo-about` — install a pinned tool
- `cargo xtask explorer build|register|unregister|check [--dev]` — Windows only
- `cargo xtask catalog [--check] [--strict]` — see PLAN-CATALOG.md

`xtask/` is a workspace crate, invoked through the alias in `.cargo/config.toml` (`xtask = "run --package xtask --"`). Platform code lives in `windows.rs`, `macos.rs`, `linux.rs`, compiled only on their OS.

When I run `cargo xtask run`, I want it to:

1. Do a dev build.
2. Start the dev host hidden, with `--data-dir .runtime/dev`, and record it in `build/dev/session.json`.
3. Wait up to 30 s for the host's `"ready":true` line.
4. Start the desktop connected to that host.

`--demo` uses simulated operations.

Every failure says what to run next, for example: "Qt 6.11.2 not found — run `cargo xtask setup qt`".

Nothing is downloaded silently. Qt, Inno Setup, AppImage tools and cargo-about are fetched only by `cargo xtask setup …`, at pinned versions.


## VERSION

The version lives in one place: `Cargo.toml` → `[workspace.package] version`. The git tag is always `v<version>`.

Everything else reads it:

- xtask parses it with the `toml` crate.
- CMake reads it with a regex anchored to a line start, so keep `[workspace.package]` above any dependency written as its own table.
- Windows version resources come from `build.rs` (which must print `cargo:rerun-if-changed=Cargo.toml`, or a bump ships a stale version) and from CMake `.rc.in` templates.
- macOS `Info.plist` is filled from it.

Only `cargo xtask release` changes it.


## DISK

```text
savescummer/
|-- xtask/            the build program
|-- packaging/        licenses/, windows/savescummer.iss, macos/Info.plist.in, linux/AppRun + .desktop
|-- assets/           icons, sounds
|-- target/           Cargo's; never written directly
|-- build/            everything regenerable
|   |-- dev/          desktop build, APP PACKAGE, session.json, logs, build-report.json
|   |-- release/      same for release
|   `-- tmp/          scratch
|-- dist/             release files and nothing else
`-- .runtime/         never cleaned
    |-- Qt/<ver>/     Qt SDK
    |-- tools/        aqtinstall venv, linuxdeploy, appimagetool
    `-- dev/          dev app data
```

`clean` removes `build/` and `dist/`, `target/` only with `--deep`, and `.runtime/` never. There is no `scripts/` directory.

The repo root also holds `rust-toolchain.toml` (pinned Rust) and `about.toml` (licenses cargo-about accepts). `.gitignore` covers the four roots plus `*.db`, `*.db-shm` and `*.db-wal`.

`build-report.json` records each build's steps, timings, and binary paths and sizes.


## APP PACKAGE

The APP PACKAGE is the assembled, runnable app under `build/<mode>/package/`: a folder on Windows, a `.app` on macOS, an AppDir on Linux.

Every APP PACKAGE contains:

- the three executables and the Qt libraries they need
- Qt license texts
- `THIRD-PARTY-LICENSES.html`, generated by cargo-about; a crate under a license not in `about.toml` fails packaging
- `.savescummer-package.json` (mode, version, platform, Qt version, creation time)
- `SHA256SUMS.txt`

Release packages have no debug symbols; dev packages keep them.

A package is assembled in a fresh `.staging-<uuid>` sibling and swapped into place only when it's complete. A failed build never damages the existing package.


## STOPPING PROCESSES

A running host may be in the middle of a save, and Windows can't replace a running executable. Before rebuilding or cleaning, xtask stops whatever runs from `build/` or `dist/`:

1. Ask desktops to close.
2. Ask hosts to shut down through their CLI (`--data-dir <dir> shutdown`, up to 30 s), so an accepted operation finishes.
3. Terminate whatever is still running.

Never stop anything running from `target/` or `.runtime/`.

The dev host is recorded in `build/dev/session.json` (PID, start time, path). It's stopped only if all three still match, so a reused PID is never killed.

If the user's installed host is running, `run` leaves it alone and warns that the dev instance won't own the tray icon or global shortcuts. `--stop-other-hosts` stops it instead.

Every dev build removes launch-at-login entries pointing into `target/` or `build/`, so signing in never starts a debug host. Other entries are the user's and stay.


## TOOLCHAINS

- Rust is pinned in `rust-toolchain.toml` (with rustfmt, clippy and the `aarch64-apple-darwin` target).
- Qt follows the latest minor release, currently 6.11, with the exact patch pinned in one xtask constant. Open-source Qt only patches its newest minor, so patches are taken promptly and a new minor is adopted within about two months, once all three platforms pass. The app only uses Widgets, Network and Svg.
- C++: Visual Studio 2022+ on Windows, Xcode 15+ on macOS, GCC 11+ on Linux. CMake ≥ 3.21.

Tools are installed only by their setup command, at pinned versions, into `.runtime/`. xtask uses them from there, so developers and CI run the same versions:

- `setup qt` — installs the pinned Qt kit with aqtinstall (in a venv under `.runtime/tools/`; needs Python 3.9+) to `.runtime/Qt/<version>/`. Safe to re-run. CI caches it keyed on the pin.
- `setup inno` — downloads the pinned Inno Setup 6 installer, checks its SHA-256, and installs it silently per-user into `.runtime/tools/inno-setup/`. No winget or Chocolatey: they aren't reliably on CI runners and don't pin versions.
- `setup linux-tools` — linuxdeploy with its Qt plugin and appimagetool, pinned release, checked by SHA-256.
- `setup cargo-about` — pinned version.

The desktop is built with CMake (`RelWithDebInfo` for dev, `Release` for release). On Windows no generator is passed, so CMake picks the newest Visual Studio. Desktop tests run through ctest against the freshly built host (`SAVESCUMMER_TEST_HOST`; Linux CI adds `QT_QPA_PLATFORM=offscreen`). If they fail, xtask prints the JUnit XML. Test screenshots go to `build/<mode>/desktop/screenshots`, never the source tree.


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

A folder, `build/<mode>/package/SaveScummer-windows-x64/`, with Qt DLLs in `bin/`, the VC runtime DLLs copied next to the app (from the newest Visual Studio, found with vswhere), the Explorer DLL (release only), and a `README.txt` with the LGPL notice and the note that the Qt DLLs may be replaced. Dev packages add PDBs. Every binary gets a version resource and `assets/icon.ico`.

### Explorer extension

A classic `IContextMenu` handler registered per-user under `HKCU\Software\Classes`, no admin needed. On Windows 11 it shows under *Show more options*. The modern menu needs MSIX and code signing, which we don't have.

Its identity lives only in `integrations/windows-explorer/identity.h`. xtask reads it and passes it to Inno Setup and its own registration code:

| | Release | Dev |
| --- | --- | --- |
| CLSID | `{3F8F42CE-463F-41B6-98D1-8C8D16B88931}` | `{43BFBA41-D0AB-44D3-A5D6-600EB5C74D18}` |
| Name | `SaveScummer` | `SaveScummerDev` |

So a dev registration never shadows the installed one.

- `explorer build` builds the DLL and runs its COM tests.
- `explorer register|unregister [--dev]` writes or removes the per-user registration.
- `explorer check` exercises the real handler without registering anything.

The extension finds the host's data directory through the `savescummer-platform` crate, the same code the host uses.

Explorer keeps the DLL loaded, so updates to it land after a restart.

### Installer

A per-user Inno Setup installer to `%LOCALAPPDATA%\Programs\SaveScummer`, no admin. Its payload is exactly the release APP PACKAGE. The `.iss` is only ever compiled by `cargo xtask dist`.

- Stable `AppId`, so upgrades replace in place.
- Two tasks, "Explorer menu" and "Launch at sign-in". Both are checked on first install and unchecked on upgrade, so a user who turned one off is never opted back in.
- Launch at sign-in is set by running `SaveScummer.Host.exe --autostart on`. The uninstaller runs `--autostart off`.
- Before replacing files, it runs `SaveScummer.CLI.exe --no-start shutdown` so an in-flight save finishes. Restart Manager closes the desktop.
- The Explorer DLL uses `restartreplace`. Registry writes are HKCU only.
- `%LOCALAPPDATA%\SaveScummer` is never touched.
- Unsigned: SmartScreen shows "More info → Run anyway", and the README says so. An off-by-default `SignedBuild` block lets signing be switched on later.

`dist` fails if Inno Setup is missing or the payload lacks `bin/SaveScummer.exe` or the Explorer DLL.

### Done when

1. `clean` stops recorded and output processes and removes `build/` and `dist/`; `--deep` also removes `target/`; `.runtime/` is untouched.
2. `run` works against `.runtime/dev`; an installed host keeps running unless `--stop-other-hosts` is passed.
3. `dist` leaves exactly one file in `dist/`, the `-setup.exe`. The release package has `SHA256SUMS.txt` and `THIRD-PARTY-LICENSES.html`, and no PDBs.
4. CMake configure shows the Cargo version.
5. Installing needs no admin, and the Explorer task adds the menu.
6. The sign-in task is checked on first install and unchecked on upgrade. When it's checked, sign-in starts the host minimized.
7. Installing over a running host shuts it down gracefully first. A DLL update completes after restart.
8. Uninstalling removes the app, its registrations and its sign-in entry, never `%LOCALAPPDATA%\SaveScummer`.


## macOS

Apple Silicon only, macOS 13+. xtask refuses to build on an Intel Mac. Rust, CMake and `Info.plist` all target arm64 and macOS 13.0.

The APP PACKAGE is `SaveScummer.app`, bundle ID `com.savescummer.SaveScummer` (fixed forever; macOS keys permissions on it). The three executables sit side by side in `Contents/MacOS/`, Qt is deployed by `macdeployqt`, and licenses and notices go to `Contents/Resources/`. The last step is an ad-hoc signature (`codesign --force --deep --sign -`), because Apple Silicon won't run unsigned binaries and `macdeployqt` breaks the linker's signatures.

The release file is a DMG with the app and an Applications link, for drag-and-drop install. Never a zip: zips break the symlinks inside Qt frameworks.

Unsigned and not notarized: the first launch needs *System Settings → Privacy & Security → Open Anyway*. The README shows it with screenshots.

Launch at login is a LaunchAgent, `~/Library/LaunchAgents/com.savescummer.host.plist`. The host writes it (pointing at itself inside the bundle, with `--minimized --data-dir <data>`) and loads or unloads it with `launchctl`. macOS shows its "Background item added" notice.

No Finder integration; it would need proper signing.

- **Upgrade:** quit, drag the new app over the old one.
- **Remove:** turn off launch at login, quit, drag to Trash. `~/Library/Application Support/SaveScummer` is never touched.

### Done when

1. `dist` on an Apple Silicon Mac leaves exactly the `.dmg` in `dist/`; on an Intel Mac it refuses with a clear message.
2. Every binary in the bundle is arm64 (`lipo -archs`), and `codesign --verify --deep --strict` passes.
3. The DMG shows the app and an Applications link; after *Open Anyway* the app runs on a clean macOS 13+ machine.
4. `Info.plist` has the Cargo version and minimum OS 13.0.
5. Enabling launch at login writes the LaunchAgent and the host starts at login; disabling removes it.
6. Replacing the app while it runs doesn't corrupt data, and the next launch runs the new version.


## Linux

x86_64, any distro with glibc 2.35+ (Ubuntu 22.04+, Fedora, Arch, SteamOS…). No `.deb` or `.rpm`.

Built on Ubuntu 22.04, because a binary only runs on a glibc at least as new as the one it was built against.

The APP PACKAGE is `SaveScummer.AppDir`, made by `linuxdeploy` with its Qt plugin. It includes both the `xcb` and `wayland` Qt plugins and the `.desktop` file. `appimagetool` turns it into the AppImage using the static runtime, so users don't need `libfuse2`. Unsigned.

`AppRun` dispatches on its first argument (`host`, `cli`, anything else runs the desktop), so the one file serves as all three programs.

Launch at login is `~/.config/autostart/SaveScummer.desktop`, written by the host with `Exec="<AppImage path>" host --minimized --data-dir "<data>"`. The path comes from `$APPIMAGE`. Each version is downloaded under a new file name, so the host re-points the entry to itself on every start.

No file-manager integration. Adding the app to the menu is left to tools like Gear Lever or AppImageLauncher.

- **Upgrade:** download the new AppImage, quit the old one, start the new one, delete the old file.
- **Remove:** turn off launch at login, quit, delete the file. `~/.local/share/SaveScummer` is never touched.

### Done when

1. `dist` on Ubuntu 22.04 leaves exactly the `.AppImage` in `dist/`.
2. After `chmod +x`, it runs on stock Ubuntu 22.04 and current Fedora without `libfuse2`, under both X11 and Wayland.
3. `….AppImage host --version` and `….AppImage cli --version` print the Cargo version.
4. Enabling launch at login writes the XDG entry and the host starts at login. Running a newer AppImage re-points the entry, and disabling removes it.


## CI

Two workflows, and every step is a `cargo xtask` command.

**ci.yml** runs on branch pushes, pull requests and manual runs (not tags), cancelling superseded runs. One required job each on `windows-latest`, `macos-latest` and `ubuntu-22.04`: cache Rust and Qt, `setup qt` (Linux also `setup linux-tools`), `check`, `build --test`.

**release.yml** runs on `v*` tags and is never cancelled. One job per platform checks out full history, runs its setup commands (`setup qt` and `setup cargo-about`, plus `setup inno` on Windows and `setup linux-tools` on Linux), runs `dist`, and uploads its single file. A final publish job (with `contents: write`) collects all three files, fetches the tag and runs `cargo xtask publish` with `GH_TOKEN`.


## RELEASE

When I run `cargo xtask release <version>`, I want it to:

1. Check that the version has three parts (a leading `v` is fine), I'm on a branch, the tree is clean, the version is new, and tag `v<version>` doesn't exist locally or on origin.
2. Write the version into `Cargo.toml` (formatting preserved) and refresh `Cargo.lock`.
3. Run `cargo xtask check` (`--skip-checks` for emergencies). If it fails, undo the bump and leave the tree clean.
4. Commit "Release <version>", create the annotated tag `v<version>`, and push both (`--no-push` prints the two commands instead).

The tag starts release.yml.


## PUBLISH

`cargo xtask publish` works the same locally and in CI:

1. Check that `gh` is logged in, `origin` exists, and tag `v<version>` points at `HEAD`.
2. Check that all three release files for that version are in `dist/`.
3. If the release is already published, refuse. If a draft exists, re-upload the files. Otherwise create a draft with generated notes.
4. Print the draft URL.

Tooling only ever makes drafts; I publish by hand. A published release is never changed — that's what a new version is for. To rebuild a draft from a different commit, delete the tag locally and on origin and push it again.


## WHAT THE APP MUST PROVIDE

These belong in PLAN.md, but this plan depends on them:

- `SaveScummer.Host --autostart on|off [--data-dir <dir>]` writes or removes the platform's launch-at-login entry and exits. `off` only removes an entry pointing at this host. The in-app checkbox uses the same code, so the host is the only writer.
- With autostart on, the host re-points its entry to its own path on every start.
- Data directories come from the `savescummer-platform` crate: `%LOCALAPPDATA%\SaveScummer`, `~/Library/Application Support/SaveScummer`, `~/.local/share/SaveScummer`.


## DOCS

- `docs/building.md` — the how-to per OS, every command and flag. The only place flags are listed.
- `README.md` — install, upgrade and removal per platform (including the SmartScreen, *Open Anyway* and `chmod +x` steps), where data lives, the quickest build commands.

Whenever the build changes, this plan and `docs/building.md` change with it.


## NOT DOING

- Code signing on any platform
- Update checks or auto-update; the app never phones home
- Intel Mac, 32-bit or ARM Linux builds
- Distro packages, Flatpak, Microsoft Store, Mac App Store
- Shell integration outside Windows
- Auto-publishing releases
