# Host restructure

A one-time refactor that makes the code match the plans before any macOS or Linux adapter is written. It doesn't add a platform. When it's done, Windows works exactly as before, apart from the start-up changes below. macOS and Linux still have stub adapters, but each one is a single file to fill in, and the rest of the code no longer assumes Windows.

Status (2026-09-25): implemented and checked on macOS (DONE WHEN 1, 2 and 4). Still to check on a Windows machine: `cargo xtask check` and `dist` there, and item 3.

The target design is already in the plans: the host is the app, the UI is its window (PLAN.md, PLAN-HOST.md PROCESSES), and every OS feature is an adapter in its own file (PLAN-HOST.md PRINCIPLES). This document only lists what the code has to change to get there, in order. Once the work is done it has served its purpose and can be deleted.


## 1. THE HOST IS THE APP

What the plans say, and the code doesn't do yet:

- **A user launch shows the UI.** Started without `--minimized`, the host opens the UI once it's ready. Today `--minimized` is parsed but never read, and the host never opens the UI on its own.
- **Starts that shouldn't show the UI pass `--minimized`.** That's the sign-in entry (already the case), the CLI starting a host for a command, the UI starting a missing host, and `cargo xtask host start`. Why: today the CLI starts the host without the flag, which would pop up a window for every script once the host honors it.
- **A second launch hands over.** A host that finds another one running (the lock is taken) asks it over the protocol to show the UI, then exits as it does today (the "another host is running" ready line, exit code 3). Why keep the exit code: tooling already relies on it.
- **Showing the UI means one window.** The host keeps track of the UI's connection (the UI identifies itself when it connects). If a UI is connected, the host tells it to come to the front; otherwise it starts one. The tray's Main window, a second launch and (later) macOS "reopen" all go through this one path.
- **The protocol gains** a Show UI command and an event that tells the UI to come to the front (PLAN-HOST PROTOCOL). The CLI gets the command too, so tests can drive it.

Tests: the e2e suite checks that a second launch reaches the running host and that a host started with `--minimized` starts nothing. The UI doesn't exist yet, so "starts one" is checked with a stand-in executable in the package folder.


## 2. NAMES

| Before | After | What it is |
| --- | --- | --- |
| `SaveScummer.Host` | `SaveScummer` | the host: the app and its entry point |
| `SaveScummer` | `SaveScummer.UI` | the window |
| `SaveScummer.CLI` | unchanged | the CLI |
| `apps/desktop` | `apps/ui` | the UI's source (doesn't exist yet) |

- Only packaged names change. The Cargo binaries stay `savescummer-host` and `savescummer-cli`; packaging renames them, as it already does.
- Everything that names an executable follows: xtask's naming, packaging and process stopping, the CLI's `host_exe()`, the host's UI lookup, the autostart entry and its tests, the Inno Setup script (the Start menu entry and the finish-page checkbox now run `SaveScummer.exe`), `.vscode`, docs.
- "Desktop" becomes "UI" in names too: CMake targets (`savescummer-ui`, `ui-tests`), the build folder (`build/<mode>/ui`), xtask's frontend step.
- No migration: the only release, v0.1.0, is an unpublished draft, so no installed copy has the old names.


## 3. ONE FILE PER OS

Moves and small reshapes, no behavior change on Windows. The pattern is xtask's: shared code in `mod.rs`, one file per OS, chosen by `#[cfg_attr(..., path = "...")]` in one place, with an `unsupported.rs` wherever an OS isn't done yet.

- **`crates/platform`:** `integration`, `autostart` and `sounds` become folders (`mod.rs`, `windows.rs`, `unsupported.rs`). `watch.rs`'s drive-release code becomes a `volumes` seam, with `drives.rs` as its Windows version and a no-op elsewhere.
- **A main-loop seam:** `platform::integration::run_main_loop(wait)`. The host calls it in place of waiting for shutdown on its main thread. On Windows it just waits; the tray keeps its own thread. Why now: macOS must run its event loop on the main thread, and this is the only place the host's structure has to allow for it.
- **`platform::process`:** `hard_exit()` and `detach(&mut Command)`, so `apps/host` and `apps/cli` no longer call `windows-sys` themselves. Unix: `_exit` and a new process group.
- **`crates/monitor`:** the Windows process source and the stub move from `lib.rs` into `source/{windows,unsupported}.rs`. The matching rules stay shared.
- **`crates/scanner`:** `mod os` moves into `os/{windows,unix}.rs`. `use_registry` becomes `query_os`, which is what it means ("ask the live OS"), with a serde alias so existing environment files still load. Steam's `registry.vdf` becomes a field `detect()` fills in, instead of a Linux-only branch, so Linux and macOS share it.
- **`crates/ipc`:** the transport (named pipes, Unix sockets: bind, accept, connect, busy) moves into `ipc/src/transport/{windows,unix}.rs`. `apps/host/src/server.rs` keeps only the connection handling and no platform code.
- **`crates/snapshots`:** `fsx` gets `fsx/{windows,unix}.rs` for identity, rename-without-replace and the error classification. The Unix error numbers come from `libc` constants, which fixes their meaning on macOS, and "not a directory" counts as missing.


## 4. TESTS AND WARNINGS

- **Unit fixtures** that hardcode `C:/` get a small path helper, so the same test runs on every OS (`core/safety`, `ipc`'s endpoint test).
- **The e2e world** splits its Windows-only helper (the scratch registry key) into `common/windows.rs`. The world itself stays a simulated Windows machine; a native macOS world belongs to PLAN-MACOS.
- **Tests that need an adapter that doesn't exist yet** (process monitoring, file watching on macOS) are ignored off Windows, each with a reason that names the missing adapter. Why not leave them failing: `cargo xtask check` has to pass on every OS, so a failure means a regression.
- **Warnings:** dead-code and unused-import warnings off Windows are fixed, so `clippy -D warnings` passes on macOS.


## NOT IN THIS REFACTOR

- Any macOS or Linux adapter (PLAN-MACOS.md).
- Replacing the hand-written Windows tray, menu and hotkeys with the cross-platform crates (`tray-icon`, `muda`, `global-hotkey`). Planned for when the macOS integration is written, so both OSes are tested together (PLAN-MACOS.md).
- The UI's technology. PLAN-UI.md describes what the window does; the toolkit stays Qt for now.


## DONE WHEN

1. `cargo xtask check` passes on macOS, with the adapter-dependent tests ignored and every ignore giving its reason.
2. The Windows target type-checks from macOS for every crate that doesn't need a C toolchain, and `cargo xtask check` and `cargo xtask dist` pass on Windows.
3. On Windows: the installer's "Launch SaveScummer" and the Start menu entry run `SaveScummer.exe`; sign-in starts it with no window; launching it again while it runs reaches the running host; the tray, hotkeys, sounds and autostart work as before.
4. No file outside a per-OS module mentions `windows_sys`, except dependency declarations.
5. The plans describe the result on their own, without this document.
