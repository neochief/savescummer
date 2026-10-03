# Save Scummer — Build & Release

I want building, packaging and releasing the app to be boring: one command to build, one command to cut a release, and one file per platform for users to download.

**Current implementation (2026-10-03):** Windows builds the UI, host and CLI in an Inno Setup installer. macOS builds the same three programs in an app bundle and DMG. Linux releases them in AppImages for x86_64 and aarch64. The tag workflow checks and packages all four targets, then creates a draft release with all four files if every job passes. Use [docs/building.md](docs/building.md) for current commands. Real-game validation of catalog entries is tracked separately in [docs/testing.md](docs/testing.md#real-game-checks).

This plan covers only the machinery around the app: builds, packaging, installers, CI and releases. App behavior lives in PLAN-HOST.md and PLAN-UI.md; the few things this plan needs from the app are listed under WHAT THE APP MUST PROVIDE.

This plan contains both implemented behavior and acceptance targets. The status above and [docs/building.md](docs/building.md) identify what is configured today; the platform DONE WHEN lists require verification on their target systems.

Windows builds the complete Tauri UI installer, macOS the disk image, and Linux an AppImage for each supported architecture. The shared build machinery keeps platform details in modules:

- shared code stays platform-neutral
- platform logic lives in its own module
- nothing assumes Windows paths, tools or file names

Tag CI checks every platform before creating the draft release.


## PRINCIPLES

- **One build tool, in Rust.** All automation is `cargo xtask`: no PowerShell, bash or Python scripts, no just/make. It runs the same on every OS. Besides Rust, it needs only what the build itself needs: the UI frontend's toolchain (see TOOLCHAINS) and `gh` for publishing.
- **CI runs the same build commands developers run.** The tag workflow installs prerequisites and calls `cargo xtask` for tests, packaging and publishing.
- **One release version, in `Cargo.toml`.** The Tauri and npm versions are checked against it and updated by `cargo xtask release`.
- **One file per platform per release,** in the friendliest format that platform has. Users never have to pick, and nothing else is uploaded.
- **Fixed executable names.** Platforms change how the programs are packaged, never what they're called.
- **Pin build inputs.** `cargo xtask setup` installs pinned packaging tools. Every platform also needs Node.js and the pnpm version declared in `apps/ui/package.json`; `pnpm install --frozen-lockfile` fetches the locked frontend dependencies. The Windows installer may fetch WebView2 during setup if the runtime is absent.
- **User data is sacred.** No install, upgrade, uninstall or clean ever touches the app's data directory or the checkpoint store, wherever the user has moved it: checkpoints are the user's saves.
- **Only a human publishes.** Tooling makes draft releases; I look at them and press publish.
- **Every failure says what to run next,** e.g. a missing Tauri build prerequisite names pnpm or the relevant `cargo xtask setup` command.


## WHAT USERS GET

The app is three programs:

- **Host** — Rust; the app itself and its entry point (SQLite, monitoring, operations, tray, hotkeys)
- **UI** — Tauri/WebView2 on Windows, Tauri/WebKit on macOS and Tauri/WebKitGTK on Linux, started by the host
- **CLI** — Rust command-line client

The Windows build always produces `SaveScummer.exe` (the host), `SaveScummer.UI.exe` and `SaveScummer.CLI.exe`. The macOS and Linux packages use the same names without `.exe`; the Linux AppImage dispatches to them through `AppRun`. Every launcher, shortcut and sign-in entry points at `SaveScummer`.

| Platform | Release file | Why this format |
| --- | --- | --- |
| Windows 10/11 x64 | `SaveScummer-windows-x64-<ver>-setup.exe` | What Windows users expect; installs per-user, no admin |
| macOS 13+, Apple Silicon | `SaveScummer-macos-arm64-<ver>.dmg` | The standard drag-to-Applications install |
| Linux x86_64, glibc 2.35+ | `SaveScummer-linux-x86_64-<ver>.AppImage` | One file to run without root or installation |
| Linux aarch64, glibc 2.35+ | `SaveScummer-linux-aarch64-<ver>.AppImage` | The same, for ARM Linux |

These minimums are recorded here and in xtask platform constants. Build flags, `Info.plist`, Linux build systems, CI runners and test machines must follow them.

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

- `test` — the full quality gate for Rust, UI and catalog (see TEST). `test crates`, `test e2e` and `test ui` run broad development sections.
- `build` — build Rust and the UI for development.
- `run [--demo] [--stop-other-hosts]` — dev build, then the dev host, launched the way a user launches the app, so it shows the UI. `--demo` uses simulated operations.
- `host start [--demo]` / `host stop` — just the dev host, with `--minimized`; used by VS Code debugging.
- `dist` — build the release package and this platform's release file in `dist/`, without running tests.
- `clean [--deep]` — stop output processes, remove `build/` and `dist/` (`--deep` also `target/`). It needs no packaging tools, so a broken setup can always be cleaned.
- `release <version>` — cut a release (see RELEASING).
- `publish` — upload `dist/` to a draft GitHub release (see RELEASING).
- `setup inno|linux-tools|cargo-about` — install a pinned packaging tool (see TOOLCHAINS).
- `catalog [--check] [--strict]` — the game catalog; owned by PLAN-CATALOG.md.

Every command validates its inputs up front.

`cargo xtask run`:

1. Does a dev build.
2. Starts the dev host with `--data-dir .runtime/dev` (or `.runtime/dev-demo` with `--demo`), and records it in `build/dev/session.json`.
3. Waits for the host's `"ready":true` line. The host shows the UI itself, as it does for a user.

The dev host uses its own data, so development never touches the real app's data. An installed host is left running, and xtask warns that the dev instance won't own the tray icon or global shortcuts. `--stop-other-hosts` stops it instead (gracefully).

The dev host runs from the dev APP PACKAGE and outlives xtask. On macOS, xtask opens the app through Launch Services so the app owns its privacy requests; xtask reads the ready line and pid from `.runtime/dev/host.log` (or the demo data folder's log). On other platforms, startup output goes to `build/dev/logs/host.log` and the host inherits only its own stdio (NUL and the log), so a terminal pipeline or IDE task reading xtask's output does not hang until the host exits. `run` opens the packaged Tauri UI on all three platforms.

xtask honors `CARGO_TARGET_DIR` like Cargo, so it can build next to another checkout's running binaries.

## VERSION

The version lives in `Cargo.toml` → `[workspace.package] version`. The git tag is always `v<version>`. Only `cargo xtask release` changes it.

Everything else reads it:

- **xtask** parses `Cargo.toml` with the `toml` crate — no pattern matching.
- **Tauri UI** has matching versions in `apps/ui/package.json` and `apps/ui/src-tauri/tauri.conf.json`; `cargo xtask release` updates both, and the build checks the Tauri version.
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
| `.runtime/` | tools and dev data — slow to rebuild, and holds data | never |

```text
savescummer/
|-- .cargo/config.toml   the xtask alias
|-- rust-toolchain.toml  pinned Rust
|-- about.toml           licenses cargo-about accepts
|-- xtask/               the build program
|-- docs/                installation, building and manual testing guides
|-- packaging/
|   |-- windows/         savescummer.iss, README.txt
|   |-- macos/           Info.plist.in, com.savescummer.SaveScummer.host.plist (login agent)
|   `-- linux/           AppRun, SaveScummer.UI (the UI's wrapper), com.savescummer.SaveScummer.desktop
|-- assets/              icons, sounds, asset tooling
|-- target/
|-- build/
|   |-- dev/             UI build, APP PACKAGE, session.json, logs
|   |-- release/         UI build, APP PACKAGE
|   `-- tmp/             scratch
|-- dist/
`-- .runtime/
    |-- tools/           Inno Setup, cargo-about, Tauri's AppImage tools (tauri-cache/), appimagetool
    `-- dev/             dev app data (the dev host's --data-dir)
```

There is no `scripts/` directory. `.gitignore` covers the four roots.


## APP PACKAGE

The APP PACKAGE is the assembled, runnable app under `build/<mode>/package/`: a folder on Windows, a `.app` on macOS, an AppDir on Linux. It's what the release file wraps, so what I test locally is exactly what ships.

Every current package contains:

- the host, CLI and Tauri UI executables, with their platform runtime files
- `THIRD-PARTY-LICENSES.html`
- `WEB-THIRD-PARTY-LICENSES.html` for JavaScript production dependencies
- `.savescummer-package.json` (mode, version, platform, UI toolkit, configuration, creation time), which marks it as generated output
- `SHA256SUMS.txt`

`THIRD-PARTY-LICENSES.html` is generated by cargo-about from the packaged Rust crates, including the Tauri UI, for the platform's own target, and merged into one page listing each license text once. A crate under a license not in `about.toml` fails packaging, so licensing is checked on every build rather than at release time. The app's own crates are `publish = false` and ignored as private, so they need no license of their own.

Each platform module declares what its package must contain, and packaging fails if anything required is missing. All three platforms require the host, CLI and UI; Linux also requires Tauri's AppDir hook and its UI wrapper.

The frontend build uses `pnpm install --frozen-lockfile`, embeds the Vite assets and stages `SaveScummer.UI` with the host. `test` runs the UI typecheck and headless tests separately. Linux packages Tauri's AppDir and adds a `SaveScummer.UI` wrapper around the bundled WebKitGTK libraries. Packaging fails if any required executable is absent.

Cargo can't put dots in binary names, so Cargo builds `savescummer-host` and `savescummer-cli`, and packaging renames them to the fixed names.

Release packages have no debug symbols; dev packages keep them.

A package is assembled in a fresh `.staging-<uuid>` sibling and swapped into place only when complete, after stopping anything running from the old one. A failed build never damages the existing package.


## STOPPING PROCESSES

A running host may be in the middle of a save, and Windows can't replace a running executable. So xtask stops processes politely, always with the same routine:

1. Ask UIs to close.
2. Ask hosts to shut down through their sibling CLI (`--data-dir <dir> shutdown`, up to 30 s), so an accepted operation finishes.
3. Terminate whatever is still running.

It's used for:

- **Output processes.** Every `build` (so also `run`, `host start` and `dist`), `test` and `clean` starts by stopping everything running from this checkout's output folders: `target/`, `build/` and `dist/`. That covers hosts and CLIs started straight from `target/`, the dev host, packaged copies, UIs and test binaries. Why: a rebuild must be clean. A process left running either locks its executable (Windows can't replace it, so the build fails with "access denied") or keeps serving old code next to the new build. Exempt are xtask itself and Cargo's build scripts, which belong to a build in progress. Never stopped: anything from `.runtime/` or installed copies (see below).
- **The dev host.** `build/dev/session.json` records its PID, start time and path. It's stopped only if all three still match, so a reused PID is never killed. The session file is deleted last.
- **Other hosts,** only with `run --stop-other-hosts`.

## TOOLCHAINS

The toolchains come in two layers, so the UI frontend can be replaced without touching anything else:

- **Core** — Rust and the packaging tools. The host, CLI, xtask, packaging and releases depend only on these.
- **UI frontend** — Tauri, Node.js and pnpm on Windows, macOS and Linux.

The packaging tools are installed by setup commands into `.runtime/`. UI builds on every platform use Node.js and the version of pnpm pinned in `apps/ui/package.json` from `PATH`; the tag workflow installs them on every platform.

### Core

**Rust** is pinned in `rust-toolchain.toml`: channel, `minimal` profile, rustfmt and clippy. rustup applies it everywhere. Bumping it is a deliberate one-line commit.

**Packaging tools:**

- `setup inno` — the pinned Inno Setup 6 installer from jrsoftware's GitHub release, checked by SHA-256, installed silently in portable mode into `.runtime/tools/inno-setup/`, so it registers nothing on the machine. Not winget or Chocolatey: they aren't reliably on CI runners and don't pin versions.
- `setup linux-tools` — the AppImage tools for the build machine's architecture, each checked by SHA-256: the `linuxdeploy` build and `AppRun` Tauri's AppImage bundler runs (pinned by Tauri's commit), `linuxdeploy-plugin-appimage` (a tagged release, not `continuous`), `appimagetool` and the type2 AppImage runtime. Tauri's three go into `.runtime/tools/tauri-cache/tauri/`, which builds give Tauri as `XDG_CACHE_HOME`, so it finds them and downloads nothing; Tauri edits them in place once it runs them, so a tool present counts as installed. linuxdeploy's GTK and GStreamer plugins aren't pinned tools: they're scripts built into Tauri's CLI (pinned by `pnpm-lock.yaml`), which it writes there itself. The GStreamer one needs the system's `patchelf`, which packaging checks for.
- `setup cargo-about` — pinned version, built with `cargo install --locked` into `.runtime/tools/cargo-about/`.

All pins, and the supported-platform minimums, live in `xtask/src/pins.rs`; CI caches are keyed on that file.

### What a UI frontend provides

Whatever the frontend is built with, it plugs into xtask the same way:

- **its prerequisites:** Tauri uses Node.js and pnpm from `PATH` on every platform
- **a build step** that takes the mode and the version and produces the `SaveScummer.UI` executable, plus the runtime files it needs, for the APP PACKAGE
- **a test step** that runs in tag CI on every platform (`pnpm test`)
- **its license notices,** shipped in every package

The Tauri frontend has setup, build, test and runtime packaging paths on all three platforms. The core, release file naming and executable names remain shared.

## TEST

Tests always build as dev: they check dev-only behavior too, such as dev hosts refusing to create a sign-in entry.

`cargo xtask test` runs, stopping at the first failure:

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets --locked -- -D warnings`
3. `cargo test --workspace --locked -- --test-threads=1` (one Rust invocation for all workspace packages, including end-to-end tests; those that steal focus require `SAVESCUMMER_DESKTOP_TESTS=1`)
4. `cargo build --workspace --exclude xtask --locked` (rebuilding xtask would relink the running `xtask.exe`, which Windows can't replace; clippy and the tests above cover it)
5. `cargo xtask catalog --check`
6. `pnpm install --frozen-lockfile` and `pnpm test` (UI typecheck and headless tests)

### Testing entry points

| Command | Scope |
| --- | --- |
| `cargo xtask test` | The full gate above. Tag CI runs this once on each platform before `cargo xtask dist`. |
| `cargo xtask test crates` | Rust tests in every workspace package except `savescummer-e2e`: the libraries, host, CLI, Tauri bridge and xtask. |
| `cargo xtask test e2e` | The `savescummer-e2e` host and CLI integration suite, run serially. |
| `cargo xtask test ui` | Frozen pnpm install, TypeScript typecheck and headless React tests. |
| `cargo xtask catalog --check` | Compare the generated catalog with `catalog/catalog.json` without running the other test sections. |

The three named `test` sections are for development. They do not run formatting, Clippy, the workspace build, catalog check or the other sections. The full gate uses one `cargo test --workspace` invocation instead of invoking `test crates` and `test e2e` separately: different Cargo feature sets would rebuild shared dependencies twice. `dist` only packages; CI runs the full gate before it.

For a narrower run, use the test runner directly:

- One Rust package: `cargo test -p savescummer-host --locked` (substitute `savescummer-core`, `savescummer-ui` or another workspace package).
- One end-to-end file: `cargo test -p savescummer-e2e --test checkpoints --locked -- --test-threads=1` (`checkpoints` is the filename under `tests/e2e/tests/`).
- One React test file, from `apps/ui`: `pnpm exec vitest run src/App.test.tsx`. This runs Vitest only; `cargo xtask test ui` also typechecks.

The end-to-end tests that take window focus report a skip and pass when `SAVESCUMMER_DESKTOP_TESTS` is absent. CI sets the variable; if it is set on a locked Mac, those tests fail instead of silently passing. Four manual or very slow Rust tests are marked `#[ignore]` and remain outside the normal full gate: two 100,000-row scale tests, audible sound playback and a Windows tray integration test.


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

`cargo xtask dist` produces the complete app bundle and DMG with the Tauri UI. User-visible macOS behavior follows the [platform guidance](PLAN.md#platforms); manual checks are in [docs/testing.md](docs/testing.md).

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
- Signed and verified as the last step, after all packaged binaries and runtimes have been staged.

### Release file

- A DMG made with `hdiutil create -format UDZO` from a folder holding the app and an `Applications` link, so installing is drag-and-drop.

### Integration

**Launch at login:** a login agent registered with `SMAppService.agent`, through the shared `--autostart on|off` code. Its plist ships in the bundle (`Contents/Library/LaunchAgents/com.savescummer.SaveScummer.host.plist`) and runs `Contents/MacOS/SaveScummer --minimized`. macOS lists it as SaveScummer in *Login Items*, under *Allow in the Background*. A custom `--data-dir` can't be carried, so `--autostart on` refuses with one on macOS.

### Upgrade and removal

The [installation guide](docs/install.md) gives the user steps; packaging preserves these behaviors:

- **Upgrade:** quit (menu-bar icon → Quit), drag the new app over the old one. Replacing a running app is safe on macOS, since running processes keep the old files, and the next launch runs the new version.
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

x86_64 and aarch64, on distributions with glibc 2.35 or newer. Each architecture has one AppImage that runs without installation or root access. Game monitoring works for native games; Wine and Proton games use the Linux process adapter (PLAN-HOST.md MONITOR AND ACTIVE STACK).

Both architectures build, package and upload AppImages in tag CI. `cargo xtask publish` requires both files in the draft release.

### Build

Build on the oldest supported Ubuntu, 22.04, on each architecture. A binary only runs on a glibc at least as new as the one it was built against, so the oldest supported system has to be the build system. A newer machine (like an Ubuntu 26.04 development VM) builds and tests fine, but its AppImage only runs on systems as new as it.

### App package and release file

The UI's GTK and WebKit come from Tauri's own AppImage bundling (`tauri build --bundles appimage`), which solves what bundling WebKitGTK needs: it copies WebKit's helper processes (`WebKitWebProcess`, `WebKitNetworkProcess`, the injected bundle) into the AppDir and rewrites their path in the bundled library to one relative to the working directory, deploys GTK's modules with linuxdeploy's GTK plugin, and adds that plugin's environment hook. `bundleMediaFramework` also deploys GStreamer's plugins with linuxdeploy's GStreamer plugin and its hook. Why: the bundled libgstreamer looks for plugins only inside the AppDir, and WebKitGTK's web process (2.52 at least) aborts at startup when it can't create `appsink` or `autoaudiosink`, so without them the window stays blank (background color only). xtask builds it only when packaging, from the pinned tools (`setup linux-tools`), and keeps its AppDir; its own AppImage isn't used.

The APP PACKAGE is `build/<mode>/package/SaveScummer.AppDir`: Tauri's AppDir with

- the host and CLI beside the UI in `usr/bin/` (`SaveScummer`, `SaveScummer.CLI`); the Tauri UI stays `usr/bin/savescummer-ui`
- `usr/bin/SaveScummer.UI`, from `packaging/linux/SaveScummer.UI`: the UI's wrapper, which runs linuxdeploy's GTK and GStreamer hooks, puts `usr/lib` on the library path and starts the UI from `usr/` (where WebKit finds its helpers)
- `AppRun` replaced by `packaging/linux/AppRun`, and Tauri's desktop entry and icons by `packaging/linux/com.savescummer.SaveScummer.desktop` and `assets/icon.svg`. The entry is named for the app id, `com.savescummer.SaveScummer`, the same as the macOS bundle; the UI names its window class (GTK's program name, the Wayland app id) the same, and `StartupWMClass` repeats it, so docks group the window under the entry and show its icon
- licenses, notices, manifest and checksums in `usr/share/savescummer/`

`packaging/linux/AppRun` is the entry point and dispatches on its first argument, so one file serves as all three programs:

- `ui` runs `SaveScummer.UI` (the host runs this to show the window)
- `cli …` runs `SaveScummer.CLI`
- anything else runs the host, `SaveScummer`, with those arguments

**The bundled libraries reach only the UI.** The host and CLI need only the system's base libraries (libc, libm, libgcc_s), and the host starts games, so they run with the environment exactly as given. The UI's wrapper keeps the environment it started with (`SAVESCUMMER_OUTER_ENV`), and whatever the UI starts gets that back (`savescummer_platform::process::outer_env`): the host it restarts, the browser and the file manager. Why: a game, browser or file manager that inherits `LD_LIBRARY_PATH` pointing into the AppImage loads its GTK instead of the system's, and may crash.

`appimagetool`, with the pinned type2 runtime (`--runtime-file`), turns the AppDir into `SaveScummer-linux-<arch>-<ver>.AppImage`, so users don't need `libfuse2`. `finish_package` checks with `file` that every program is built for the build machine's architecture.

### Integration

- **Launch at login:** the host writes and removes `~/.config/autostart/com.savescummer.SaveScummer.desktop` with `Exec="<AppImage path>" --minimized`, through the shared `--autostart on|off` code. The path comes from `$APPIMAGE`, which the AppImage runtime sets, and the host keeps it current (see WHAT THE APP MUST PROVIDE). Like macOS, the entry always starts the default data folder: `--autostart on` with another `--data-dir` is refused.
- **App menu entry:** an AppImage installs nothing itself, so a release host started from one writes `~/.local/share/applications/com.savescummer.SaveScummer.desktop` (`Exec="<AppImage path>"`) and the icon (`~/.local/share/icons/hicolor/scalable/apps/savescummer.svg`) at every start, rewriting them only when they differ (a new version is a new file). It marks the entry `X-SaveScummer-Target` like the autostart entry and leaves an entry of the same name that isn't its own alone. Why the host and not only AppImage tools: besides the menu, the desktop names the app by this entry: the dock groups the window under it, notifications carry its name, and the shortcuts portal registers the host under its id (PLAN-HOST, Linux under Wayland). Without the entry the portal files the shortcuts under whatever app started the host. Development builds never write it.

### Upgrade and removal

The README gives both:

- **Upgrade:** download the new AppImage, quit the old one, start the new one, delete the old file.
- **Remove:** turn off launch at login, quit, delete the file, and delete `~/.local/share/applications/com.savescummer.SaveScummer.desktop` and `~/.local/share/icons/hicolor/scalable/apps/savescummer.svg` to drop it from the app menu. `~/.local/share/SaveScummer` is never touched.

### Done when

1. `dist` on the oldest supported Ubuntu leaves exactly `SaveScummer-linux-<arch>-<ver>.AppImage` in `dist/`, for x86_64 and aarch64.
2. After `chmod +x`, it runs on the oldest supported Ubuntu and current Fedora, both stock, without `libfuse2`, under both X11 and Wayland.
3. `….AppImage --version` and `….AppImage cli --version` print the Cargo version.
4. Enabling launch at login writes the XDG entry pointing at the AppImage and the host starts at login. Running a newer AppImage re-points the entry, and disabling removes it.
5. Nothing ever touches `~/.local/share/SaveScummer`.


## CI

The only workflow is `.github/workflows/release.yml`. Setup actions install toolchains and caches; every platform calls `cargo xtask test` once, then `cargo xtask dist` to build its release file:

- Runs on `v*` tags only, with a concurrency group that never cancels, so a release is never half-built.
- Windows and macOS upload their installer and disk image from `dist/`; Linux x86_64 and aarch64 upload their AppImages. Every upload uses `if-no-files-found: error`.
- A final publish job (`needs:` all three platform jobs, including both Linux matrix entries; `contents: write`) runs only after every platform succeeds. It downloads all four files into `dist/`, fetches the tag and runs `cargo xtask publish` with `GH_TOKEN`.


## RELEASING

### Cutting a release

`cargo xtask release <version>`:

1. Checks that the version has three parts (a leading `v` is fine), I'm on a branch, the tree is clean, the version differs from the current one, and tag `v<version>` exists neither locally nor on origin.
2. Writes the version into `Cargo.toml`, `apps/ui/package.json` and `apps/ui/src-tauri/tauri.conf.json`, then refreshes `Cargo.lock` (`cargo update --workspace`).
3. Runs the catalog check. If it fails, the bump is undone and the tree is left clean.
4. Commits "Release <version>", creates the annotated tag `v<version>`, and pushes the branch only (`--no-push` prints the commands instead).

It runs on the `release` branch, after merging `main` into it locally. The branch push updates the catalog channel (PLAN-CATALOG.md 6), so the local catalog check is the gate before the catalog goes out. Pushing the tag by hand starts `release.yml`; the tag workflow tests and packages all four targets before drafting the release.

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

- `README.md` — brief app workflow and CLI usage, linking to the guides.
- `docs/install.md` — installation and checkpoint locations for users.
- `docs/building.md` — prerequisites, `cargo xtask` commands, outputs, release runbook, CI and dev data.
- `docs/testing.md` — manual release smoke test and real-game catalog checks.

Whenever the build changes, this plan and `docs/building.md` change with it.


## NOT DOING

- Update checks or auto-update — the app never checks for its own updates; users install the next release. Its only network use is fetching catalog updates and Steam artwork.
- Intel Mac or 32-bit builds.
- Distro packages, Flatpak, Microsoft Store, Mac App Store — the four release files cover the supported platforms.
- Explorer, Finder or file-manager integration on any platform — a game's saves can span several folders, so there's no single folder to right-click; the app never touches the file manager.
- Auto-publishing releases — a human always publishes.
- Renaming the executables, or cleaning `.runtime/`.
