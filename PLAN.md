# Save Scummer

I want an app that saves and loads backups of game save data, mainly for roguelike games, so a bad run can be undone. It should feel like part of playing: press a hotkey to save, press another to load, and see every save, load and revert in a timeline.

The app finds supported games on the computer by itself and lets the user add others. For each game it backs up one directory: the save folder, or the whole data folder when the game resists save tampering.

It should work on Windows, macOS and Linux. Windows comes first; the other two are planned from the start so they're new adapters, not a rewrite.

The app icon is [assets/icon.svg](assets/icon.svg).

This document is the map. Each part of the app has its own plan, written so that part can be rebuilt from scratch without reading the others (see PLANS).


## TERMS

- **DIR** — the one directory backed up for a game. The whole DIR is the unit of every backup.
- **Checkpoint** — a copy of DIR, kept as an ordinary folder next to it. A **saved** checkpoint is one the user made: with Save, or by copying the folder in Explorer. A **recovery** checkpoint (the UI calls it a "recovery point") is the state captured automatically just before a Load or Revert.
- **Label** — a short name the user gives a saved checkpoint ("Before boss fight").
- **History** — the per-game timeline: saves, loads, reverts, game starts and closes. Rows point at checkpoints; they never own files.
- **Load** restores a saved checkpoint. **Revert** restores the recovery checkpoint of a Load or Revert. Both keep the current state as a new recovery checkpoint first, so nothing is ever lost.
- **Flush** deletes all of a game's checkpoints and history, after confirmation.
- **ACTIVE STACK** — the running games, ordered by which one the user switched to last. Its top is the hotkeys' target.
- **Known game** — found through the catalog. **Custom game** — added by the user with their own paths.
- **Catalog** — the built-in list of supported games, with what proves each is installed and where its saves may be.


## HOW IT FITS TOGETHER

The app is three programs, plus a shell extension on Windows:

| Program | Role |
| --- | --- |
| `SaveScummer` | The desktop UI (C++/Qt). The normal entry point. |
| `SaveScummer.Host` | The background host (Rust). One per user. Owns every rule, all state and all file operations. |
| `SaveScummer.CLI` | A console client (Rust) for scripts and diagnostics. |
| Explorer extension | Adds Save and Load to the Windows Explorer right-click menu. |

```text
            ┌──────────────┐   ┌──────────────┐   ┌────────────────────┐
            │ SaveScummer  │   │ SaveScummer  │   │ Explorer extension │
            │ (desktop UI) │   │ .CLI         │   │ (Windows)          │
            └──────┬───────┘   └──────┬───────┘   └─────────┬──────────┘
                   └──────── local protocol (per user) ─────┘
                                      │
                            ┌─────────┴──────────┐
                            │  SaveScummer.Host  │── hotkeys, sounds, tray,
                            │  core + adapters   │   game monitoring, sign-in
                            └─────────┬──────────┘
                                      │ uses
                            ┌─────────┴──────────┐
                            │ catalog resolver   │
                            └────────────────────┘
```

- **The host works alone.** With no window open, scans, monitoring, hotkeys, operations, history and sounds all keep working. Closing or crashing the UI cancels nothing.
- **Clients are thin.** The UI, CLI and Explorer extension only ask the host and show its answers. None of them touches game files, the database or the catalog. Why: four entry points must never disagree about what's safe.
- **Whoever needs the host starts it.** The UI and CLI start the host from their own install folder when it isn't running, and otherwise attach to the one that is. At sign-in the host starts in the tray.
- **The catalog decides, the host acts.** Where a known game's saves are is decided by the catalog module; the host runs it and stores the result, but never adds rules of its own.


## PLANS

| Plan | Covers | Depends on |
| --- | --- | --- |
| [PLAN-HOST.md](PLAN-HOST.md) | The host and CLI: library, monitoring, checkpoints, history, labels, operations, recovery, storage, hotkeys, sounds, tray, Explorer menu, artwork, and the **protocol** every client uses. Its own tests. | The catalog resolver |
| [PLAN-UI.md](PLAN-UI.md) | The desktop main window: layouts, sidebar, actions, history, dialogs. Its own tests. | The protocol in PLAN-HOST |
| [PLAN-CATALOG.md](PLAN-CATALOG.md) | The catalog: how it's authored and built, and the resolver that picks each game's DIR at runtime. Its own tests. | Nothing |
| [PLAN-BUILD.md](PLAN-BUILD.md) | Builds, packaging, installers, CI and releases. | The few things the app must provide, listed there |
| [PLAN-ERRORS.md](PLAN-ERRORS.md) | A catalog of failure and interruption scenarios: what the host detects, what the user sees, how to test it. Spans host and UI. | PLAN-HOST, PLAN-UI |

Rules for the plans:

- **Each plan is self-contained** for its own part. It names what it needs from another part (an interface, a protocol) but doesn't restate that part's rules.
- **Each plan says how to test its part.** There is no separate test plan.
- **The host owns the protocol.** The UI plan changes to follow it, not the other way round. Both change together when the protocol does.
- **Plans explain why.** They describe principles, main parts and core logic, and leave implementation details to the code.


## PRINCIPLES

These hold across every part:

- **The user's saves come first.** Only an explicit Delete or Flush removes a checkpoint. Installs, upgrades, uninstalls, restarts, scans and failures never touch the user's backups or the app's data.
- **Files on disk are the truth.** Checkpoints are plain folders the user can see and copy in Explorer. The database only describes them.
- **Change nothing you can't verify.** An uncertain state is left alone and explained, never "fixed" by guessing.
- **One authority.** Every rule lives in one place, the host. Replacing the UI, a platform adapter or the storage never means reimplementing a rule.
- **Portable core, thin platform adapters.** OS features (process watching, hotkeys, tray, sounds, file-manager menus, sign-in) are small replaceable adapters around a core that knows no OS.


## PLATFORMS

| | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Status | First target | Planned | Planned (SteamOS in mind) |
| Game monitoring | Yes | To investigate | To investigate |
| Global hotkeys | Yes | To investigate | To investigate |
| File-manager menu | Explorer extension | Not planned | Not planned |
| Proton games | — | — | Resolved inside the game's prefix (catalog) |


## TECHNOLOGY

- **Rust** for the host, CLI and every backend module. Why: one safe, fast, portable language for the part that touches the user's files.
- **Qt 6 Widgets (C++)** for the desktop UI, in its own process. Why: a native-feeling, lightweight desktop app, kept apart so it can be replaced without touching the rules.
- **SQLite**, owned only by the host, for configuration, checkpoint records, history and the operation journal.
- **A versioned local JSON protocol** over named pipes (Windows) and Unix sockets (macOS, Linux) between the host and its clients. No direct Rust/C++ bindings.

This stack was chosen after a small Windows proof of concept showed host/UI communication, busy rejection, progress, UI reconnection and an operation surviving the UI being killed. It used about 51 MiB of combined working set and reopened the UI in about 150 ms. Those were demo numbers without real copying, SQLite or scanning, not budgets.


## REPOSITORY

| Path | What | Plan |
| --- | --- | --- |
| `apps/host`, `apps/cli` | The host and CLI programs | HOST |
| `crates/` | Backend modules: core rules, snapshots, storage, scanner, monitor, platform adapters, IPC | HOST |
| `crates/catalog`, `crates/catalog-build`, `catalog/` | The catalog resolver, builder and data | CATALOG |
| `protocol/` | The protocol's schemas and shared example messages, tested by both Rust and C++ | HOST |
| `apps/desktop` | The Qt desktop UI and its tests | UI |
| `integrations/windows-explorer` | The Explorer extension | HOST |
| `tests/` | Cross-module tests and the fake-game program | HOST |
| `xtask/`, `packaging/`, `.github/` | Build, packaging, CI and release tooling | BUILD |
| `assets/` | Icons and sounds | — |

Module rules:

- The core rules know nothing about Qt, SQLite, the protocol or the OS; the host wires in the real implementations.
- Each backend module can be built and tested on its own. Dependencies between modules never form a cycle.
- The desktop UI never links backend code, opens the database or touches game files.
