# Tauri UI

This is a separate Tauri 2 process over the existing `savescummer-ipc` v1 host protocol. The Rust bridge in `src-tauri/src/bridge.rs` is the only layer that connects to the host or reads its cached artwork. The webview has no shell or general filesystem capability.

## Run the vertical slice

From `apps/ui`, run `pnpm install --frozen-lockfile` and `pnpm build`. From the repository root, run `cargo build -p savescummer-host -p savescummer-cli -p savescummer-ui`. Then launch the demo host with:

```sh
SAVESCUMMER_UI_EXE="$PWD/target/debug/savescummer-ui" \
  target/debug/savescummer-host --demo --data-dir /tmp/savescummer-ui-trial --no-catalog-update
```

The host launches the UI on startup. To raise the same window later, use `target/debug/savescummer-cli --data-dir /tmp/savescummer-ui-trial/data show-ui`. The demo host stores its protocol socket in the `data` subfolder; a non-demo host uses the data directory directly. The Tauri binary also accepts `--data-dir` for direct debugging and `SAVESCUMMER_HOST_EXE` for reconnect tests. Closing the UI leaves the host running. When a demo host starts the UI, it passes the simulated environment to the UI so a replacement host cannot fall through to the real machine.

For a macOS WebKit bundle smoke test, build an unsigned trial app from `apps/ui` with `pnpm tauri build --debug --bundles app --no-sign --config '{"bundle":{"active":true}}'`. Point `SAVESCUMMER_UI_EXE` at `target/debug/bundle/macos/SaveScummer.app/Contents/MacOS/savescummer-ui` when launching the demo host. This bundle is for local inspection; its `com.savescummer.ui.trial` identity is deliberately separate from the host's eventual app identity.

On Windows, `cargo xtask dist` builds and tests the host, CLI and Tauri UI, assembles the package, and produces the Inno Setup installer in `dist/`. For a standalone debug UI binary, run `pnpm tauri build --debug --no-bundle` from `apps/ui`. The Tauri build embeds the Vite assets in the executable. A plain `cargo build -p savescummer-ui` builds the development executable, which expects the Vite server and otherwise shows a connection error. The Tauri Windows build also requires `src-tauri/icons/icon.ico`.

## Checks

Run `pnpm test` and `pnpm build` in `apps/ui`, then `cargo test -p savescummer-ui` and `cargo test -p savescummer-host` from the root. The UI tests use a fake bridge for deterministic state, operation, delete, and dialog flows. The demo host exercises the actual host launch and local socket boundary.

## Trial status

The main game and virtualized history view, Save/Load/Revert, labels, deletion, scan, Add, Configure, Settings preferences, More, and Flush use the host protocol. The installed artwork is returned only from the host's cache. The web assets, fonts, and icons are bundled locally.

Windows packaging now includes this UI. Exact Whiteboard event-icon parity, cross-engine visual and screen-reader validation, and packaged cold-open/full memory measurements remain. Native Browse dialogs choose file and folder paths; the host validates the resulting text. Windows WebView2 has been checked on Windows; Linux WebKitGTK still needs a Linux machine. The macOS and Linux release packaging paths have not been migrated to Tauri.

The unsigned macOS debug app was inspected in WKWebView against the isolated demo host: local game art, fonts, history, Settings, Add, Configure, and More loaded, with native controls over the dark title area. The Add form initially inherited the selected executable path; this was corrected after inspection. The package has not been signed, notarized, or run through the cross-platform trial gate.

### Shortcut and accessibility follow-up (2026-09-28)

The UI test suite passes 20 tests, including shortcut capture, duplicate and host-unavailable rejection, menu arrow keys, Escape, and focus restoration. The complete host, UI bridge, and end-to-end Rust suite passed before the final Settings transaction adjustment; its focused shortcut protocol test passed again afterward, verifying validation, capture suppression, and persistence after host restart. `cargo check -p savescummer-platform --target x86_64-pc-windows-msvc --offline` and `cargo fmt --check` pass. Cross-checking the entire Windows host on this Mac stops at its missing MSVC resource compiler and C library headers. The rebuilt unsigned macOS bundle displayed the reassigned `⌥F6` and `⌥F10` hints, reopened Settings with those values, and kept them after rejecting a duplicate inline. A synthetic `⌥F6` sent to the webview did not produce an OS global hotkey event, so a physical-key or trusted event-source check is still needed to confirm activation of the new Carbon registration. The host hotkey path succeeded through the CLI against the same isolated demo host.

Computer-use accessibility inspection found that WebKit did not focus a button after a pointer click. The dialogs now remember their explicit opener, and the More button takes focus when clicked. In the rebuilt bundle with VoiceOver enabled, Escape from Settings returned focus to Settings; the More menu accepted Arrow Down, focused its named items, opened Configure with Return, and returned focus to More on Escape. A pointer-opened More menu also closed with Escape. The accessibility tree exposed names for the dialogs, shortcut fields, menu items, and destructive history controls. Speech output was not captured. VoiceOver was returned to its original off setting after this check.

A follow-up packaged WKWebView inspection removed the decorative brand icon and wordmark from the accessibility tree, gave the web document a purpose-first title, and replaced duplicate native `title` help on game cards and More with hover/focus hints. Game cards and More now appear once by name in the accessibility tree. macOS still exposes its native window, scroll area, and HTML content ancestors; the tree is not a transcript of VoiceOver speech. Speech output remains unverified.

### FTL live-install check (2026-09-27/28)

An isolated host with system integrations and catalog updates disabled detected the local Steam FTL installation, its cached artwork, and its macOS save set. Save from the packaged UI created a checkpoint in `/private/tmp/savescummer-ui-ftl-live/checkpoints`; hashes of all five files in the live FTL profile were unchanged. For Load, the trial host was configured to use a temporary copy of that profile. After changing the copy's `continue.sav`, Load from the UI restored the copy to its original hashes. The live profile still matched its pre-test hashes. The isolated host and UI were shut down afterward.

### Windows WebView2 and FTL check (2026-09-28)

The Windows Tauri debug build rendered in WebView2 and connected to an isolated demo host. Save and Load succeeded from the UI. Closing the UI left the host running; `show-ui` reopened it, and the UI restarted a stopped host against the same isolated data directory.

An isolated real host detected the local Steam FTL installation and its Windows save set, including a redirected `Documents/My Games` folder. FTL launched from Steam and the host reported it running. Save from the UI created a checkpoint without changing the live `continue.sav` hash. For Load, the host was configured through the UI to use a copy of the FTL profile. After changing only the copy's `continue.sav`, Load restored its original hash; the live file stayed unchanged. FTL and the isolated host were then closed. This checked the debug executable, not a Windows installer or release package.

### Windows installer check (2026-09-28)

`cargo xtask dist` built the optimized Windows host, CLI and Tauri UI, ran the Rust and UI suites, assembled a checked package, and produced the Inno Setup installer. A silent per-user install into an isolated test location placed all three executables. Launching the installed host opened its sibling UI; Save and Load succeeded against simulated game data. The test install was uninstalled afterward. The installer is unsigned; the WebView2 download path was compiled but not exercised because this machine already has the Runtime.

### Mac debug-build process sample (2026-09-27)

The local demo host launched one UI; terminating that UI left the host alive, `show-ui` started a new UI, and killing the demo host made the UI start a replacement against the simulated environment. The replacement reported only the six demo games. On the same Mac, `ps` reported 88,752 KiB RSS for the host and 95,648 KiB for the UI while open; after closing the UI, the host was 88,768 KiB and the UI process was gone. A repeated `show-ui` took 200 ms from the first request until the host acknowledged an open UI watch connection. These are debug-build process samples, not visual-ready latency, and exclude separate WebKit helper processes. The earlier Windows proof of concept in `PLAN.md` recorded about 51 MiB combined working set and a 150 ms reopen with a different workload, so these figures are not a like-for-like regression result.
