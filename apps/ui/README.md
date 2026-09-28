# Tauri UI trial

This is a separate Tauri 2 process over the existing `savescummer-ipc` v1 host protocol. The Rust bridge in `src-tauri/src/bridge.rs` is the only layer that connects to the host or reads its cached artwork. The webview has no shell or general filesystem capability.

## Run the vertical slice

From `apps/ui`, run `pnpm install --frozen-lockfile` and `pnpm build`. From the repository root, run `cargo build -p savescummer-host -p savescummer-cli -p savescummer-ui`. Then launch the demo host with:

```sh
SAVESCUMMER_UI_EXE="$PWD/target/debug/savescummer-ui" \
  target/debug/savescummer-host --demo --data-dir /tmp/savescummer-ui-trial --no-catalog-update
```

The host launches the UI on startup. To raise the same window later, use `target/debug/savescummer-cli --data-dir /tmp/savescummer-ui-trial/data show-ui`. The demo host stores its protocol socket in the `data` subfolder; a non-demo host uses the data directory directly. The Tauri binary also accepts `--data-dir` for direct debugging and `SAVESCUMMER_HOST_EXE` for reconnect tests. Closing the UI leaves the host running. When a demo host starts the UI, it passes the simulated environment to the UI so a replacement host cannot fall through to the real machine.

For a macOS WebKit bundle smoke test, build an unsigned trial app from `apps/ui` with `pnpm tauri build --debug --bundles app --no-sign --config '{"bundle":{"active":true}}'`. Point `SAVESCUMMER_UI_EXE` at `target/debug/bundle/macos/SaveScummer.app/Contents/MacOS/savescummer-ui` when launching the demo host. This bundle is for local inspection; its `com.savescummer.ui.trial` identity is deliberately separate from the host's eventual app identity.

On Windows, run `pnpm tauri build --debug --no-bundle` from `apps/ui`, then point `SAVESCUMMER_UI_EXE` at the repository's `target/debug/savescummer-ui.exe`. For an optimized executable, use `pnpm tauri build --no-bundle` and `cargo build --release -p savescummer-host -p savescummer-cli --locked`, then use the three binaries in `target/release`. The Tauri build embeds the Vite assets in the executable. A plain `cargo build -p savescummer-ui` builds the development executable, which expects the Vite server and otherwise shows a connection error. The Tauri Windows build also requires `src-tauri/icons/icon.ico`.

## Checks

Run `pnpm test` and `pnpm build` in `apps/ui`, then `cargo test -p savescummer-ui` and `cargo test -p savescummer-host` from the root. The UI tests use a fake bridge for deterministic state, operation, delete, and dialog flows. The demo host exercises the actual host launch and local socket boundary.

## Trial status

The main game and virtualized history view, Save/Load/Revert, labels, deletion, scan, Add, Configure, Settings preferences, More, and Flush use the host protocol. The installed artwork is returned only from the host's cache. The web assets, fonts, and icons are bundled locally.

The trial is not yet a replacement for Qt. Shortcut reassignment, exact Whiteboard event-icon parity, cross-engine visual and screen-reader validation, release bundle identity and packaging, and packaged cold-open/full memory measurements remain. Native Browse dialogs choose file and folder paths; the host validates the resulting text. The Settings dialog shows the host's current fixed shortcut defaults as read-only. Windows WebView2 has been checked on Windows; Linux WebKitGTK still needs a Linux machine. `PLAN-BUILD.md` and `PLAN-MACOS.md` remain unchanged until the gate passes.

The unsigned macOS debug app was inspected in WKWebView against the isolated demo host: local game art, fonts, history, Settings, Add, Configure, and More loaded, with native controls over the dark title area. The Add form initially inherited the selected executable path; this was corrected after inspection. The package has not been signed, notarized, or run through the cross-platform trial gate.

### FTL live-install check (2026-09-27/28)

An isolated host with system integrations and catalog updates disabled detected the local Steam FTL installation, its cached artwork, and its macOS save set. Save from the packaged UI created a checkpoint in `/private/tmp/savescummer-ui-ftl-live/checkpoints`; hashes of all five files in the live FTL profile were unchanged. For Load, the trial host was configured to use a temporary copy of that profile. After changing the copy's `continue.sav`, Load from the UI restored the copy to its original hashes. The live profile still matched its pre-test hashes. The isolated host and UI were shut down afterward.

### Windows WebView2 and FTL check (2026-09-28)

The Windows Tauri debug build rendered in WebView2 and connected to an isolated demo host. Save and Load succeeded from the UI. Closing the UI left the host running; `show-ui` reopened it, and the UI restarted a stopped host against the same isolated data directory.

An isolated real host detected the local Steam FTL installation and its Windows save set, including a redirected `Documents/My Games` folder. FTL launched from Steam and the host reported it running. Save from the UI created a checkpoint without changing the live `continue.sav` hash. For Load, the host was configured through the UI to use a copy of the FTL profile. After changing only the copy's `continue.sav`, Load restored its original hash; the live file stayed unchanged. FTL and the isolated host were then closed. This checked the debug executable, not a Windows installer or release package.

### Mac debug-build process sample (2026-09-27)

The local demo host launched one UI; terminating that UI left the host alive, `show-ui` started a new UI, and killing the demo host made the UI start a replacement against the simulated environment. The replacement reported only the six demo games. On the same Mac, `ps` reported 88,752 KiB RSS for the host and 95,648 KiB for the UI while open; after closing the UI, the host was 88,768 KiB and the UI process was gone. A repeated `show-ui` took 200 ms from the first request until the host acknowledged an open UI watch connection. These are debug-build process samples, not visual-ready latency, and exclude separate WebKit helper processes. The earlier Windows proof of concept in `PLAN.md` recorded about 51 MiB combined working set and a 150 ms reopen with a different workload, so these figures are not a like-for-like regression result.
