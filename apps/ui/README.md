# Tauri UI

This is a separate Tauri 2 process over the existing `savescummer-ipc` v1 host protocol. The Rust bridge in `src-tauri/src/bridge.rs` is the only layer that connects to the host or reads its cached artwork. The webview has no shell or general filesystem capability.

## Run the vertical slice

From `apps/ui`, run `pnpm install --frozen-lockfile` and `pnpm build`. From the repository root, run `cargo build -p savescummer-host -p savescummer-cli -p savescummer-ui`. Then launch the demo host with:

```sh
SAVESCUMMER_UI_EXE="$PWD/target/debug/savescummer-ui" \
  target/debug/savescummer-host --demo --data-dir /tmp/savescummer-ui-trial --no-catalog-update
```

The host launches the UI on startup. To raise the same window later, use `target/debug/savescummer-cli --data-dir /tmp/savescummer-ui-trial/data show-ui`. The demo host stores its protocol socket in the `data` subfolder; a non-demo host uses the data directory directly. The Tauri binary also accepts `--data-dir` for direct debugging and `SAVESCUMMER_HOST_EXE` for reconnect tests. Closing the UI leaves the host running. When a demo host starts the UI, it passes the simulated environment to the UI so a replacement host cannot fall through to the real machine.

On every platform, `cargo xtask test` checks the host, CLI and Tauri UI; `cargo xtask dist` assembles the platform release package in `dist/`. For a standalone debug UI binary, run `pnpm tauri build --debug --no-bundle` from `apps/ui`. The Tauri build embeds the Vite assets in the executable. A plain `cargo build -p savescummer-ui` builds the development executable, which expects the Vite server and otherwise shows a connection error. The Tauri Windows build also requires `src-tauri/icons/icon.ico`.

## Preview UI states

`pnpm dev` in a plain browser swaps the Tauri bridge for `src/mock/mockBridge.ts`, which serves a snapshot of the demo host. Add `?scenario=<name>` to preview a named state (empty library, running game, interrupted operation, missing access, failed save, …), or open `/scenarios.html` for the list; the JetBrains **Run UI scenarios** configuration opens it. Scenarios in `src/mock/scenarios.ts` state host facts, and `src/mock/policy.ts` mirrors `apps/host/src/policy.rs` to derive what the host would publish; keep the two in step.

The app uses one Wood sound set for interface, game run/stop, Undo, Save, Load, failure, busy, and Info drawer cues. Run `node scripts/generate-interface-sounds.mjs` from `apps/ui` to regenerate all 16 WAVs in `public/sounds` and the six host WAVs in `assets/sounds`. The browser preview plays the host cues when its simulated Save or Load runs; the desktop app plays them through the real host. The generator uses oscillators and seeded noise; it takes no application sound or sample as input. The Play sounds setting silences interface cues. Button ticks are limited to opening dialogs and starting a scan; routine navigation stays quiet. Run/stop play only after the host confirms the state change; Undo plays when the pending deletion is reversed.

## Checks

Run `pnpm test` and `pnpm build` in `apps/ui`, then `cargo test -p savescummer-ui` and `cargo test -p savescummer-host` from the root. The UI tests use a fake bridge for deterministic state, operation, delete, and dialog flows. The demo host exercises the actual host launch and local socket boundary.

## Platform packages

`cargo xtask dist` packages this Tauri UI with the host and CLI on Windows, macOS and Linux. Windows produces an installer, macOS a disk image, and Linux an AppImage for x86_64 or aarch64. The Linux app has been exercised with real games. See [building and packaging](../../docs/building.md) for prerequisites, checks and release commands.
