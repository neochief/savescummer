# SaveScummer

I want to create a Windows app that would help me save and load backups of game save files or data, mainly for roguelike games.

The app's data model should have a game library containing catalog-known games discovered on the computer and custom games added by the user, together with the directory (we'll call it DIR) to back up for each game (basically, either the save game dir or the entire game data dir if the game is clever enough to prevent tampering with the saves).

Ideally, this app should be cross-platform and work on Windows, Linux and macOS.

The app icon is already available at [assets/icon.svg](assets/icon.svg).

## ARCHITECTURE and MODULE BOUNDARIES

The app consists of independently testable modules with explicit interfaces. Keep game and operation rules independent of the UI framework, database implementation and OS integration details. Package the modules as one application; internal modules are libraries, while the main desktop executable, background host and command-line client have distinct responsibilities and lifecycles.

### Selected technology stack

- Rust for the core application, background host, snapshot engine, scanner, monitor and platform adapters; Cargo for Rust builds and tests.
- Qt 6 Widgets with C++ for the separate desktop UI; CMake for its build. Keep the C++ client focused on presentation and communication with the host.
- SQLite for history, settings and durable operation records, owned by the Rust host through the storage module.
- A versioned local JSON command/query/event protocol over Windows named pipes and Unix-domain sockets on macOS/Linux. The UI and host communicate as separate processes; no direct Rust/C++ bindings are required.
- Windows first, with portable core modules and explicit macOS/Linux adapters. Build, package and test each supported platform separately.

The stack was selected after a small Windows release-build proof of concept demonstrated host/UI communication, busy rejection, progress, UI reconnection and an operation continuing after UI termination. It measured approximately 51 MiB of combined working set (9.3 MiB private resident memory) and a 153 ms median UI reopen with warm caches. These are minimal-demo measurements, not production budgets: real file copying, SQLite, scanning, monitoring and platform integrations were absent. The experimental code is outside the application source tree; the layout below defines the production repository.

### Application executables

The application distribution contains three executables with one canonical identity each:

| Component | Windows filename | macOS/Linux filename | Role |
| --- | --- | --- | --- |
| Main application | `SaveScummer.exe` | `SaveScummer` | The user-facing Qt desktop application and normal entry point. It starts or reuses the background host, connects to it and presents the main window. |
| Background host | `SaveScummer.Host.exe` | `SaveScummer.Host` | The single per-user background process. It owns application state, operations, monitoring, integrations, tray behavior and the local service endpoint. |
| Command-line client | `SaveScummer.CLI.exe` | `SaveScummer.CLI` | The console client for scripting, diagnostics and every supported host command/query. It starts the host when a command requires one and no host is running. |

Treat these filenames, including capitalization and suffixes, as part of the application contract. The main application is the only unsuffixed executable. Do not ship aliases, duplicate launchers or another executable named only `savescummer`. Installed components locate one another by their canonical sibling filenames. Explicit executable-path overrides are allowed for development and tests, but are not required in a complete distribution.

Use descriptive lowercase build-target identifiers independently of installed filenames: `savescummer-desktop` for the CMake target, and `savescummer-host` and `savescummer-cli` for the Cargo binary targets. Configure or package those targets to produce the canonical platform filenames above. Debug-symbol filenames use the same canonical stem as their executable, such as `SaveScummer.pdb`, `SaveScummer.Host.pdb` and `SaveScummer.CLI.pdb` on Windows.

Embed consistent application identity in every Windows executable. Set the product name to `SaveScummer`; use `SaveScummer`, `SaveScummer Background Host` and `SaveScummer Command-Line Client` as the respective file descriptions; set each original filename to its canonical Windows filename; and apply the application version, company/copyright fields and icon through the native version resource. Explorer, Task Manager, crash reports, startup registration, process management and build reports must therefore use coherent names.

### Repository layout

Use one repository, one Cargo workspace for Rust packages, and a CMake build for C++ components. Each backend module is a separately testable Rust crate. These library boundaries do not introduce additional running processes: the background host links the backend modules, and the desktop UI runs separately.

```text
savescummer/
├── Cargo.toml
├── Cargo.lock
├── CMakeLists.txt
│
├── apps/
│   ├── host/                   # Rust background-host executable
│   ├── cli/                    # Rust command-line client executable
│   └── desktop/                # C++ / Qt Widgets main executable
│       ├── src/
│       └── tests/
│
├── crates/
│   ├── core/                   # Commands, policies, workflows, interfaces
│   ├── snapshots/              # Copy, staging, replacement, rollback
│   ├── storage/                # SQLite implementation and migrations
│   ├── scanner/                # Catalog parsing and installation discovery
│   ├── monitor/                # Game activity and active-stack rules
│   ├── platform/               # OS adapters
│   │   └── src/
│   │       ├── windows/
│   │       ├── macos/
│   │       └── linux/
│   └── ipc/                    # Rust transport and wire-message handling
│
├── protocol/                   # Shared message schema and example fixtures
├── catalog/
│   └── games/
│       └── void-war.yaml
├── integrations/
│   └── windows-explorer/       # Separate native shell extension
│
├── tests/
│   ├── integration/            # Tests spanning modules/processes
│   ├── fake-game/              # Small controllable test executable
│   └── fixtures/
├── assets/                     # Icons and sounds
├── packaging/                  # Platform-specific distribution files
├── scripts/                    # Build, test and packaging commands
├── PLAN.md
└── PLAN-INTEGRATION-TESTS.md
```

Repository and dependency rules:

- `apps/host` owns background startup, lifecycle and wiring of concrete implementations. `crates/core` owns application policy and workflow coordination; it must not depend on Qt, concrete SQLite storage or OS APIs.
- `apps/cli` is a protocol client with console input/output. It does not link or reproduce host workflows; it sends the same commands and queries as other clients and renders structured replies for people and scripts.
- Define dependency interfaces with the module that consumes them. Implementations satisfy those interfaces, and the host supplies them. Keep crate dependencies acyclic and each module buildable and testable independently of the full application.
- `apps/desktop` owns presentation and its client connection to the host. It communicates through the shared service protocol and must not link backend implementations, access SQLite or manipulate game saves.
- `protocol/` is the authoritative shared wire-contract specification, including versioned message schemas and compatibility fixtures. Both Rust and C++ protocol tests use those fixtures. `crates/ipc` implements Rust transport and message handling; transport details stay outside the core application.
- Keep module tests in their owning crates and Qt UI tests in `apps/desktop/tests`. Root `tests/integration` holds cross-module and cross-process scenarios, using a Cargo workspace test package where applicable. Share the controllable fake-game executable and test data through `tests/fake-game` and `tests/fixtures`, following `PLAN-INTEGRATION-TESTS.md`.
- Keep game definitions in `catalog/games`, independently of scanner implementation. Keep the Explorer extension in its own native build target; it forwards requests to the host rather than implementing save/load rules.
- Build scripts wrap standard Cargo and CMake commands. Use Cargo's test runner and Qt Test; do not introduce a custom test framework. Packaging produces one application distribution containing `SaveScummer`, `SaveScummer.Host`, `SaveScummer.CLI`, their runtime dependencies and required integrations.
- Keep build outputs, downloaded SDKs and local runtime data out of version control. Commit source, game definitions, protocol fixtures, migrations, build configuration and the application Cargo lockfile.

### Application processes

Run one `SaveScummer.Host` process per user. It owns the core application, persistent state, operation locks, startup recovery, scanning and game monitoring. It must operate without a main window or client: shortcuts, Explorer commands and CLI commands must still work, operations must finish, history must persist, and sound/notification feedback must remain available.

`SaveScummer` is an optional graphical client that connects to the host through a local command/query/event interface. Opening it starts `SaveScummer.Host` from the same application directory when necessary and attaches to the single existing host otherwise. Closing or crashing the UI must not stop the host or cancel an operation. Reopening the UI retrieves current state and progress; it must not depend on events received by the previous UI instance.

`SaveScummer.CLI` is an optional console client of the same service contract. A command that needs a host starts the canonical sibling `SaveScummer.Host` when necessary, waits for readiness and sends the request. The CLI never becomes a second host and never implements application operations locally. Commands that only resolve local CLI syntax or print help do not start the host.

The host composes modules and manages their lifecycle. Business rules live in the core application and can also run inside a test process without starting a background host, desktop window or IPC server. The UI and CLI depend on the service contract and can run against a fake service during development.

### Modules

| Module | Owns | Boundary |
| --- | --- | --- |
| UI | Game widgets, history presentation, progress display, configuration forms and confirmation dialogs | Sends commands and renders returned state/events, including cached artwork supplied by the host. Does not download artwork, copy game files, access SQLite directly, scan installations or implement operation policy. |
| Artwork service | Steam artwork resolution, download queue, cache validation and persistence | Runs asynchronously in the existing background host process, independently of scanning and Save/Load. Publishes cached asset availability to the UI through the service contract. |
| Core application | SAVE, LOAD, REVERT, Flush and interrupted-operation recovery workflows; validation, busy/recovery states, operation locking and coordination | Sole entry point for actions from UI, shortcuts and Explorer. Coordinates the other modules through interfaces. |
| Snapshot engine | Snapshot discovery, native copy naming, file copying, staging, replacement and rollback | Accepts resolved paths and operation context; reports results and progress. Does not select the active game, render UI or decide history policy. |
| History and settings store | Games, overrides, snapshot metadata, history entries and durable operation records | Repository interface for atomic batches of changed records, indexed lookups and paginated per-game reads, with a SQLite implementation. Does not manipulate game files or decide when an operation is permitted. |
| Game catalog and scanner | Catalog parsing/validation, installation discovery, path resolution and Proton translation | Returns resolved candidates and availability using discovery providers. Does not start backup/restore operations or overwrite user choices. |
| Game monitor | Process-to-game association, launch/close observations and ACTIVE STACK ordering | Consumes process/focus observations and resolved games. Publishes changes independently of the main window. |
| Platform integrations | OS discovery providers, known-folder resolution, process/focus observations, shortcuts, file-manager commands, tray, autostart, sounds and notifications | Separate adapters behind narrow interfaces. Translate OS events into core commands/observations and core results into platform feedback. |
| Background host | Module composition, single-instance ownership, local service transport, startup and shutdown | Supplies concrete implementations. Contains no duplicate SAVE/LOAD/REVERT logic. |
| Command-line client | Command parsing, script-friendly output and protocol connection management | Uses the public service contract. Contains no persistence, scanning, monitoring or SAVE/LOAD/REVERT implementation. |

Platform integrations are a family of small adapters, not one interface that every module must depend on. For example, the scanner needs discovery/path providers, the monitor needs process/focus observations, and feedback needs sound/notification delivery. Each can be replaced independently.

### Persistence and per-game data access

Use one SQLite database for the application. Scope ordinary game operations to that game's affected records. The core prepares explicit insert, update and delete batches through the repository interface; the store applies each batch in one transaction. A Save or operation-phase update must not rewrite unchanged records for that game or any other game. Settings changes update settings only, plus required revision metadata. Monitor and discovery changes may affect several games in one batch without rewriting their histories.

Publish in-memory changes only after the transaction commits. On failure, retain the previous committed state in both memory and the database. Persist operation admission before acknowledging acceptance, and commit successful completion, checkpoint metadata and the corresponding history entry together. Keep durable replacement and recovery boundaries explicit; reducing database writes must not weaken crash recovery or SQLite durability settings.

Use indexed lookups for game ownership, checkpoint IDs, operation IDs and request IDs, and an index supporting per-game history ordering. Allocate durable history sequence and checkpoint registration order without scanning all historical records; preserve their ordering across restart and migration. Do not clone, compare or serialize the whole library to commit a game operation. Keep per-game operation exclusion and serialize database commits as needed; separate databases or parallel writers are not required.

Keep active configuration, current operation/recovery state and bounded read caches in memory. Read historical rows, terminal operation journals and retired checkpoint records through repository queries when needed, rather than loading the entire audit history at startup. Startup recovery must find every unresolved operation directly. Lazy loading must preserve global path-overlap checks, retained recovery-path reservations, exact checkpoint lookup and request idempotency; data absent from a cache is not evidence that it does not exist.

The core owns visible-history and checkpoint-eligibility rules. Maintain per-game summaries and an indexed visible-history projection that supports bounded page reads, with session context calculated independently of page boundaries. Storage may persist derived indexes supplied by the core but must not implement a second version of the policy. Invalidate or update the affected game's projection after checkpoint, history, configuration or session changes; artwork changes and unrelated game activity do not rebuild it. Make any rebuild coherent with its source revision, and process large rebuilds in bounded batches.

Retain durable records until the existing Flush rules permit their removal. Pagination and cache eviction do not delete history, checkpoints or recovery data. Persist whether a game is catalog-known or custom independently of whether its paths were configured by the user; a path override on a known game must not make it custom. Schema migrations preserve game origin, IDs, ordering, original-directory ownership, exact action references and operation journals atomically. When migrating older records that predate explicit origin, classify catalog IDs as known and other manually configured IDs as custom so existing user-created games remain manageable.

### Shared service contract

Expose a small, versioned local API with plain data types and stable IDs:

- Commands: Save, Load (default or an explicit saved checkpoint ID), Revert (the exact recovery checkpoint ID referenced by a selected Loaded/Reverted history entry), retry interrupted-operation recovery, resolve recovery (an explicit interrupted operation and choice), confirmed Flush history, add and configure a custom game, atomically forget a custom game after confirmed cleanup, configuration updates for known games, rescan and request an asynchronous artwork cache check for known games.
- Queries: current library summaries, active stack, configuration, exact checkpoints and their restore eligibility, paginated per-game history, operation status by ID and recovery status.
- Events: game availability/activity changes, scan state and discovery changes, artwork availability changes, history changes, operation start/progress/completion/failure, recovery status changes and configuration changes.

The game read model and IPC state expose the catalog's optional Steam app ID (`stores.steam`) and the artwork service's current asset availability and validated local cache paths. Include artwork state in the initial snapshot and publish changes so a newly attached UI can display cached icons and update as downloads finish. Steam metadata resolution and download state belong to the host's artwork service; artwork URLs, image hashes and cache paths are not catalog fields or durable game records. Keep the Rust read model, request/response schemas, fixtures and desktop parser aligned with the artwork check command, state and events.

Use the same core command handling for every caller. The host validates commands and acquires operation locks; a disabled UI button is never the enforcement mechanism. Commands return an accepted operation ID or a structured rejection such as busy, unavailable or recovery needed. Clients can query an accepted operation after reconnecting instead of reissuing a destructive command. A client disconnect must not cancel an accepted operation.

History responses expose the exact saved or recovery checkpoint reference for each action. The core resolves and validates that checkpoint; clients do not supply trusted filesystem paths. History-entry IDs remain useful for presentation and audit relationships, but are not a second source of restore eligibility.

The connection must provide a consistent current-state snapshot and subsequent events, with revisions or equivalent resynchronization so clients cannot miss a change while attaching. Progress and errors use structured fields; user-facing wording is supplied by the UI or feedback adapter. A transport failure or disconnect must not be reported as success.

Routine state snapshots contain library summaries: settings, game configuration and origin, active stack, scan-in-progress state, artwork, diagnostics, per-game availability, the default checkpoint's display metadata, whether visible history and Flush are available, and current operation/recovery and failure information. Do not include accumulated history, all checkpoints or terminal operation journals. Historical details are queried separately. Watch checks lightweight revisions before constructing a response; unchanged polling must not clone the library or rebuild history. Live-directory availability checks still run when needed even without a metadata event.

History queries require a game ID and accept an opaque cursor and a bounded page size. Default to 50 rows and cap requests at 200, with a serialized byte budget below the 8 MiB frame limit. Each row includes its display timestamps, exact checkpoint action target and current action availability, so the client does not need a global checkpoint map. Order the visible timeline newest first by durable history sequence, using stable IDs as an additional cursor key; equal timestamps never cause omissions or duplicates. Manual-copy modification estimates continue to govern default LOAD selection and session association as specified separately.

Bind each history cursor to its game, host instance and history-projection revision. Read each page from one coherent revision. A relevant same-game change invalidates existing cursors with an explicit reload response; unrelated games, progress-only updates and artwork changes do not. Host restart invalidates old cursors. Compute session-marker visibility using the full relevant session context, never just the raw records inside a page.

All potentially large collections have bounded responses. Flush and Forget previews return counts and a confirmation revision, with optional Details paths fetched in pages bound to that revision. Forget uses the same cleanup scope and revalidation rules as Flush. Other oversized replies require pagination or an explicit size error, never silent truncation; report a specific error if one item exceeds the response budget. Keep historical record growth out of routine state payloads rather than increasing the frame limit.

Define wire read models independently of internal mutable state. Version incompatible service changes explicitly and update `SaveScummer.Host`, `SaveScummer.CLI`, `SaveScummer`, the Explorer bridge, schemas and shared fixtures together. Reject mismatched versions clearly and document the required host/integration upgrade steps; do not fall back to unbounded state transfer. `SaveScummer.CLI` history supports explicit paging and a streaming all-pages mode with bounded memory; if its cursor becomes invalid, report that history changed instead of silently duplicating or omitting entries.

Keep IPC local to the signed-in user. The core application must not depend on the selected IPC transport. Module interfaces and service messages must not expose UI controls, framework-specific objects, SQL rows or mutable internal state.

### Independent development and testing

- Define module contracts before implementations. Supply fake implementations for dependencies so each module can be developed and exercised independently.
- Test core workflows without the UI, using controllable filesystem/store/clock behavior to exercise failures, overlapping requests and recovery transitions. For Forget, inject deletion, commit and interruption failures and verify that the custom entry remains visible until its entire Flush phase and final removal commit succeed; retries must be idempotent.
- Test the snapshot engine against isolated temporary directories, including locked/unavailable files where supported, partial copies, collisions, failed replacement and rollback.
- Test scanner providers with fixture catalogs, registry/launcher metadata and directory layouts. No installed Steam client or actual game library should be required for these tests.
- Test custom-game creation, restart persistence, name and path editing, installed/uninstalled transitions, monitoring, and coexistence with catalog entries. Verify that rescans neither discard nor recreate forgotten custom games.
- Test the monitor with recorded or synthetic launch/focus/close sequences, including multiple processes per game.
- Test the store's persistence, incremental row writes, indexed per-game queries, transaction boundaries and restart recovery separately. Verify actual writes, including the absence of writes to unchanged records. Replacement store implementations must satisfy the same repository contract.
- Test the UI against a fake service that can produce busy, progress, failure, unavailable-snapshot and disconnected states. Cover the always-present Installed games section, scan progress, zero/singular/plural post-scan feedback based only on newly found known games, custom-game validation, narrow-width action wrapping, uninstalled custom rows and Forget confirmation/failure behavior. Verify each dialog's fixed Enter/default action, confirm that moving keyboard focus does not change it, and keep every dialog action row free of icons.
- Test `SaveScummer.CLI` against a fake service and the real host contract. Cover host autostart, every command/query family, bounded history paging and streaming, structured failures, exit codes and script-friendly output.
- Test the host's artwork service with a fake downloader and temporary cache: cache reuse after restart, missing/deleted/corrupt icon downloads, post-scan discovery, UI attachment, duplicate queue suppression, offline failures and interrupted writes. Verify that slow downloads do not block scanning, service requests or Save/Load. These tests must not require live Steam access.
- Keep platform-specific integration checks separate from portable module tests. Verify the service contract end to end with the UI absent, including shortcut/Explorer/CLI command handling and UI reattachment during an operation. On every packaged platform, verify that the main application starts its canonical sibling host, the CLI starts and connects to that same host, and the host opens or focuses the canonical main application from the tray.

History policy, operation locks and file replacement behavior must have a single authoritative implementation. Replacing the UI, discovery provider or storage implementation must not require recreating those rules.


## SCAN, GAME LIBRARY, KNOWN AND CUSTOM GAMES

When the background host starts, it should perform a quick scan for gaming platforms and catalog-known games that are present on the user's computer. Found platforms and games are displayed in the main app window's list whenever the UI is attached. The scan is performed each time the host starts, when the user selects **Scan for known games**, and periodically (once every 15 minutes), including while the UI is closed. Coalesce or serialize overlapping scan requests so manual, startup and periodic scans cannot race. Publish scan-in-progress state so every attached UI represents the same host-owned activity.

The scanner follows three steps:

1. Find candidate installations using launcher metadata, platform application records, known locations and user overrides.
2. Match candidates to the game library using stable identifiers where available, then validate their expected executable paths. A similar display name alone is insufficient.
3. Resolve the game's data DIR using the library's path rule in the correct native or Proton environment.

Keep scanning bounded to these sources and declared paths; do not recursively search entire drives. Multiple sources finding the same installation must resolve to one known-game entry. Deduplicate by the installation directory's filesystem identity using native Windows or Unix facilities, not by lowercasing paths: aliases of one directory are one candidate, while distinct directories on case-sensitive filesystems remain distinct. Preserve user overrides and all custom-game records across rescans. A scan does not attempt to rediscover forgotten custom games from their old paths.

Installation detection is separate from save-data availability. An installed game may not have created DIR yet; this must not make the game appear uninstalled. If multiple distinct installations or data locations remain plausible, require a one-time choice in Configure rather than guessing from modification times. A temporarily unavailable drive or unreadable discovery source must not be treated as confirmed uninstallation. For a custom game, the configured executable path is its installation evidence: mark it installed when the executable is a file, mark it uninstalled only when absence is confirmed through an accessible ancestor, and retain the previous state for unavailable drives or inconclusive access failures. Every scan refreshes this status without changing the configured path. Scanning never deletes snapshot files, custom-game records or durable history records; it can retire externally removed or changed checkpoints and update the visible history as described below.

## Game platforms

### Steam

Discover the Steam client and all configured Steam libraries, then read local installed-game metadata. Match games by Steam app ID and resolve the installation directory from the metadata rather than assuming the display name is the installation folder name.

The Steam discovery implementation is shared across Windows, macOS and Linux, with platform-specific client-location handling. It must account for multiple libraries and supported alternative Steam installation locations. Verify that the expected game executable exists before accepting a candidate as an installed game.

On Windows, the scanner may also derive the uninstall registry key "Steam App <appid>" from the declared Steam ID. This fallback is shared scanner behavior, not a registry declaration repeated in every Steam game's library entry.

### Windows application records

Read installed-app registry records from the current-user and machine-wide uninstall locations, including applicable 32-bit and 64-bit registry views. Treat names and installation locations as discovery hints, then validate them against the game definition and actual files. Registry records are not a complete inventory and do not generally declare a game's save directory.

For non-Steam games, allow a game-specific registry locator when needed. Do not query every application through Windows Installer or assume that all portable games have registry entries.

### macOS applications

For games with a declared native macOS definition, allow lookup by a known bundle identifier and validate the returned application location and executable. A bundle identifier is an optional game-library field for games that need it.

### Known paths and user overrides

Use game-specific known installation paths and explicit user-selected locations as fallbacks for portable games or installations not found through other sources. Broad package-manager discovery is not required for the initial scanner.

## Game library

Keep entries declarative and small. Each entry declares:

- A stable game ID and display name.
- An `info` text field containing game-specific instructions for saving and loading, including any required in-game navigation or memory-reset steps.
- Store identifiers, such as a Steam app ID, when available.
- Per-platform executable paths, relative to the discovered installation directory.
- A per-platform data-directory rule declaring both the base location and the game-specific subdirectory. The whole resolved DIR is the snapshot unit.

Steam discovery, Steam registry fallback, path-root resolution, Proton translation and snapshot naming are shared scanner behavior. Steam artwork resolution, downloading and caching belong to a separate artwork service within the same background host process. Do not declare icon paths, artwork URLs or image hashes in library entries; the Steam app ID is sufficient input for the artwork resolver. Add optional game-specific locators, alternative data locations or runtime exceptions only when a game needs them. User overrides are local settings and do not modify the shared catalog.

Store `info` as a UTF-8 multiline string in the shared game definition. Support paragraphs and plain-text numbered lists, preserving their order and displaying each procedure as an ordered list starting at 1; treat the content as text, without interpreting HTML or executable commands. Instructions describe the manual steps the player takes around SaveScummer operations. They do not automate the game or change core Save/Load behavior. Include `info` in new definitions; allow an empty string while instructions are unavailable and treat a missing field in older definitions as empty. Do not invent generic instructions for games whose procedure has not been documented.

### Custom game library

Users can add games that are not in the shared catalog. Persist these entries in the same per-user game library and subject them to the same core validation, monitoring, snapshot, history, operation-lock and recovery rules as known games. A custom entry contains:

- An opaque, host-generated stable ID that is independent of its display name and cannot collide with catalog IDs.
- A nonempty, user-editable display name.
- One absolute executable path used for installation status and process monitoring.
- One absolute data DIR used by Save, Load, Revert, Explore and backup discovery.
- Explicit `custom` origin. Keep origin separate from the existing notion of user-configured paths because known games can also have path overrides.
- Empty `info`; the ordinary no-instructions presentation applies.

Adding a custom game must not require either configured path to exist. This allows a user to register an expected executable before installation and an expected DIR before first launch. Resolve each path through its existing ancestor, enforce absolute-path and data-directory safety rules, and do not create the executable, installation directory or DIR while validating. Compute the initial installed state from the executable using the same conclusive-presence rules as a scan. Reject an invalid addition atomically without leaving a partial library entry. Data-directory overlap rules apply across known and custom games. Display names do not identify games and need not be unique; path safety and opaque IDs prevent accidental ownership collisions.

Configure allows a custom game's name, executable and DIR to be edited. Known-game names and catalog instructions remain catalog-controlled, while their existing executable and DIR override workflow remains available. A configuration update preserves the game's stable ID, origin, snapshots and history, and commits only after all new values validate. Returning to an earlier resolved DIR restores eligibility for its surviving checkpoints under the ordinary path rules.

Custom entries persist regardless of installation state. A missing executable produces the `Uninstalled` status but does not by itself disable Save, Load, history or recovery when the configured DIR and required checkpoints remain available. Rescanning re-evaluates the configured executable; it never replaces custom paths with catalog discoveries or converts a custom entry into a known one merely because names or paths resemble a catalog game.

### Complete example: Void War

```yaml
id: void-war
name: Void War
info: |
  To save the current game progress:
  1. Exit to Main Menu (the game saves data here).
  2. Run "Save" in SaveScummer.
  3. In-game, choose Continue from Main Menu.

  To load the saved game progress:
  1. Exit to Main Menu (loading from here won't work because the game data is still in memory).
  2. Go to Tutorial (this clears the last run from memory).
  3. Run "Load" in SaveScummer.
  4. In-game, go to Main Menu, then Continue. You should see the loaded state.

stores:
  steam: 2853590

platforms:
  windows:
    executables:
      - "Void War.exe"

    data_dir: "{APPDATA}/Void_War"
```

This is the complete normal entry, not a placeholder requiring extra registry, icon or Proton fields. The Steam app ID is confirmed by the [Steam listing](https://store.steampowered.com/app/2853590/Void_War/), and the developer documents the [Roaming AppData save location](https://itch.io/post/11632754). The Windows executable and data directory were also verified locally when this entry was designed. Native macOS and Linux definitions are omitted until native releases and their paths are verified.

### Data-directory roots

The library declares the root and relative path; the scanner resolves that root on the current machine. It must not assume that all games save under AppData. For example:

```yaml
# Void War
data_dir: "{APPDATA}/Void_War"
```

Other games may use rules such as "{LOCALAPPDATA}/ExampleGame/Saves", "{DOCUMENTS}/My Games/ExampleGame", "{SAVED_GAMES}/ExampleGame" or "{INSTALL_DIR}/saves". These are illustrations, not additional Void War paths.

On native Windows, {APPDATA} means the current user's Roaming AppData directory, {LOCALAPPDATA} means Local AppData, and {DOCUMENTS} and {SAVED_GAMES} mean the corresponding known folders. Resolve them through platform facilities, respecting redirected locations rather than constructing paths from a hardcoded username. {INSTALL_DIR} is the validated installation directory for this candidate.

Games with verified native releases can add macos and linux blocks using the same executable/data_dir structure and appropriate roots, such as the user's home, macOS Application Support, and Linux XDG data/config directories. Respect XDG overrides and defaults. Path templates are data, not shell commands.

### Proton resolution

On Linux, reuse a game's windows definition when its Windows version is running through Proton. Resolve Windows roots such as {APPDATA} inside that game's actual prefix, not against the Linux host user's directories. {INSTALL_DIR} still refers to the discovered game installation.

For a standard Steam installation, the prefix is normally under "<Steam library>/steamapps/compatdata/<appid>/pfx". Discover the relevant library and prefix rather than hardcoding the drive, home directory or prefix user. Custom locations need reliable discovery evidence or an explicit user override.

For Void War, the typical resolved data directory is:

```text
<Steam library>/steamapps/compatdata/2853590/pfx/
  drive_c/users/steamuser/AppData/Roaming/Void_War
```

Separate native Linux from Windows-through-Proton when resolving a game. A leftover Proton prefix alone does not prove which runtime is currently in use. Use installation/runtime evidence, and require a Configure choice if it remains ambiguous. Do not silently switch between native and Proton data locations or mix their histories.

Different Proton releases do not require duplicate game definitions. Prefix resolution is shared behavior; add a game-specific exception only when verified necessary. Resolving a path does not claim compatibility with every Proton release.

### Resolved games

For each resolved game, retain:

- Stable game ID and display name.
- Explicit known/custom origin, independent of whether the user has overridden paths.
- The library entry's `info` text, exposed through the host's game read model and IPC state to the UI. Keep the catalog as its source so instructions are not duplicated in Qt code or local path overrides.
- Discovered installation identity, source and full installation path.
- Resolved platform/runtime and Proton prefix when applicable.
- Full executable paths for process identification.
- Full data DIR and its current availability.
- The catalog's optional Steam app ID, used by the host's artwork service independently of the installation's discovery source.
- Persistent local overrides, including the resolved current data-directory path.

Catalog metadata such as discovery source, platform/runtime and Steam app ID is optional for custom games. Custom games instead retain the user-supplied executable and DIR as their authoritative configuration. Both origins use one resolved-game read model so downstream monitoring and file operations do not duplicate behavior.

Do not maintain a separate `location_id` or a registry of location UUIDs. Each checkpoint records the original resolved data-directory path it belongs to. Changing a configured DIR must not silently retarget existing Restore or Revert actions: compare the checkpoint's recorded directory with the game's current DIR using the shared platform-aware path rules. Returning to the same resolved directory makes its surviving checkpoints eligible again without changing their IDs or registering duplicate checkpoints. History stays associated with the game and references checkpoints for its actions.

### Steam artwork loading and cache

Use Steam as the source of game artwork for catalog entries that declare a Steam app ID. The current UI uses game icons; library backgrounds, covers, headers and logos can use the same loader when a UI design calls for them. Do not fetch unused artwork by default. Custom games and known games without an available Steam icon keep a neutral initials placeholder in the icon box; executable and registry icons are not alternative artwork sources.

Use the existing background host process that performs scanning; do not launch an additional artwork process. After each scan publishes discovered games, the host queues cache checks and missing icon downloads in its artwork service. Run network requests asynchronously and blocking cache I/O and image validation on background workers within that process. Startup and UI-attachment checks use the same queue. Scanning, the UI event loop, game monitoring and Save/Load operations never wait for artwork; do not hold scan or operation locks during artwork work. Publish completed cache entries through IPC and update the matching UI rows without changing their layout or selection. Deduplicate queued and in-flight requests by Steam app ID and asset kind, with bounded download concurrency.

Download icons from Steam and persist them in SaveScummer's own per-user cache directory, keyed by Steam app ID and asset kind. Resolve icon URLs from Steam metadata, including any required image hash, rather than assuming the app ID alone determines an icon URL. Do not require a user-supplied Steam API key or sign-in. Keep this disposable cache separate from the catalog, game data, snapshots and history database; the app must not depend on Steam's local image cache remaining present.

Resolve the cache root through a shared platform-path provider: the Windows LocalAppData known folder plus `SaveScummer/cache`, macOS `~/Library/Caches/SaveScummer`, and Linux `$XDG_CACHE_HOME/SaveScummer` with `~/.cache/SaveScummer` as the fallback when XDG_CACHE_HOME is unset, empty or not absolute. Honor OS folder redirection and the current user's home directory; do not hardcode usernames, drive letters or Steam installation paths. Use the same relative layout below that root on every platform, such as `artwork/steam/<appid>/<asset-kind>.<extension>`. The host resolves native paths and supplies them through IPC; the UI does not reconstruct OS-specific locations. Tests inject a temporary cache root. Cache deletion is recoverable through the normal missing-asset check.

On host startup after initial discovery, and on every UI startup including attachment to an already-running host, queue a background cache check for every currently known game. The UI requests this check through IPC rather than inspecting the cache itself. Reuse readable, valid cached images immediately. Queue downloads for missing, deleted, empty or undecodable entries, even if an earlier session recorded a successful download. Repeat this check when subsequent scans publish discovered games, including already-known games whose cache files have disappeared. These checks and downloads also run when the host is operating without a UI. Valid cached images do not need to be downloaded again on every startup or scan.

Write downloads to temporary files, validate that they decode as images, and atomically publish completed cache entries. An interrupted download must never count as a cached icon. On network failure or unavailable artwork, retain any usable cached image or show the placeholder, and retry missing entries with bounded backoff while the host is active and on later startup or scan checks. Do not show blocking dialogs or repeatedly request unavailable artwork on every repaint. Closing the UI does not cancel artwork work in the host; incomplete entries left by host shutdown are retried on the next startup.

### Data-directory validation

The core validates DIR before accepting a discovered location or saving a user override. Apply the same rules to catalog paths and manual choices:

- Reject a DIR that equals, contains or is inside another configured game's DIR, including games that are temporarily unavailable. Identify the conflicting game and path in the error. Keep recorded absolute paths in overlap checks when unrelated protected locations or game directories cannot be resolved, so disconnected volumes do not disable accessible games. Resolving the actual operation target remains mandatory.
- Reject overly broad locations: disk/volume roots and network-share roots; user-profile/home and shared user roots; OS/system directories; application-data roots such as Roaming AppData, Local AppData, LocalLow and ProgramData; Documents and Saved Games roots; and shared installation containers such as Program Files, Steam library roots, steamapps and common. Include the corresponding locations on other supported platforms and inside a resolved Proton prefix. Reject ancestors of these protected locations as well.
- Base these checks on resolved platform folders and discovered library locations, not folder names or path depth alone. A game-specific child such as "{APPDATA}/Void_War" or "{DOCUMENTS}/My Games/ExampleGame" is valid if it passes the other checks. A shallow game-specific directory is not automatically invalid.

Normalize paths and resolve existing directory aliases before comparing them, respecting the filesystem's case rules. Compare directory components and identity rather than raw string prefixes, so "Game" and "Game2" do not conflict. DIR may not exist yet: resolve its existing ancestors and validate the intended location without creating it. Recheck resolved locations before file operations so changed paths cannot bypass validation.

Reject an invalid override without changing the previous configuration. If discovery yields an invalid DIR and no valid configured location remains, keep the installed game visible with a concise configuration error and disable its file operations until corrected. For an overly broad path, explain that the user must select the game's own data directory. These are enforced validation errors, not warnings with a proceed-anyway option.

## MONITOR and ACTIVE STACK

After the background host finishes the initial scan, the game monitor should track the launch, activation and termination of every resolved known or custom game's configured executables in the central data structure called ACTIVE STACK. Monitoring continues without a UI client. The core uses this state to select the game for global shortcuts and publishes it for the UI to display in game widgets.

There should be a focus stack of the launched games, and the shortcuts should only work with the latest game in the stack. There should be just one element per launched game in the stack. Two processes for the same game count as one in the stack. If all game processes are terminated, the entry is removed from the stack. If the game is focused, it moves to the top of the stack.

For example, I have FTL launched. The stack contains just that game. Now I launch Void War and it gets focused. The app detects that and adds Void War to the stack. Now if I alt-tab to FTL, FTL moves to the top of the stack. If I exit FTL, it's removed from the stack. Void War is on top of the stack, even though it may currently be in the foreground.

Game launches and closes add history markers. These markers do not create snapshots and have no Restore or Revert action. Closing a game removes it from ACTIVE STACK but does not delete any snapshots or history. Relaunching the game continues the same history. The visible timeline shows these markers only for observed sessions containing a surviving checkpoint or actionable recovery point; sessions without any such point are hidden.


## SNAPSHOTS, HISTORY and STORAGE

Snapshots contain game files. History entries describe what happened and reference snapshots by stable IDs. Displayed timestamps are labels, not identifiers; actions in the same second must remain distinct.

### Checkpoint ownership and restore eligibility

Checkpoint metadata is authoritative for restore selection and eligibility. Keep the responsibilities small:

| Record | Owns |
| --- | --- |
| Game | Stable game ID and the currently configured resolved `data_dir`. |
| Checkpoint | Stable generation ID, game ID, original resolved data-directory path, backup path, saved/recovery kind, filesystem identity and change signature, availability/removal state, timestamps and durable registration order. |
| History entry | Event ID, chronological order, event time and observed-session context, plus references to the checkpoints involved. It has no independent location ID, directory ownership or restore-availability state. |
| Operation journal | The game, accepted action and exact original live/source/recovery/staging/retained paths, source generation checks, durable phase and outcome needed for interruption recovery. |

Default LOAD selects directly from eligible saved checkpoint records. A history action supplies its referenced checkpoint ID. Both paths use one core validation rule: the checkpoint belongs to the requested game, has the required kind, was created or imported for the current resolved DIR, and its exact generation remains available. Revalidate its filesystem identity and change signature before restoring. Preserve current DIR and perform replacement through the same restore workflow for LOAD and REVERT.

Store the original data-directory association on the checkpoint, not another copy on every history row. Keep the journal's captured paths and identity checks: they describe a specific accepted operation and must remain usable even if a live directory is temporarily absent or history is hidden. Launch/close markers describe observed game sessions; they do not establish checkpoint ownership. Preserve observation epochs so unobserved host downtime does not join unrelated sessions.

Use known save time for app-created saved checkpoints and the recorded folder-modification estimate for manual ones. Break equal selection times by durable checkpoint registration order. History sequence orders timeline events only; changing or filtering the timeline must not change default LOAD selection.

When implementing this simplification, update Rust records, SQLite persistence, the service schemas/fixtures, `SaveScummer.CLI` and other entry-point target handling together. Preserve existing checkpoint/history IDs, chronological relationships and interrupted-operation paths when converting stored records. Do not recreate backup folders or discard journals as part of removing `location_id`.

### Snapshot storage and history relationships

All snapshots are siblings of DIR, in the same parent directory:

```text
Game/
|-- Void_War/                     current game data (DIR)
|-- Void_War - Copy/              saved snapshot
|-- Void_War - Copy (2)/          saved snapshot
|-- Void_War.recovery-000001/     state before a load
|-- Void_War.recovery-000002/     state before another load
`-- Void_War.recovery-000003/     state before a revert
```

- Saved snapshots follow the platform's native duplicate-directory naming convention, including localized names. Manually created sibling copies matching a supported convention are ordinary saved checkpoints, regardless of whether the app created them. Discover them automatically as "Existing backup" entries without inventing SAVE events. Their discovery time must not be presented as the time they were saved.
- Recovery snapshots use a separate reserved naming convention with unique IDs, such as "Void_War.recovery-000001". Never overwrite an existing directory when allocating a new snapshot. Recovery snapshots are excluded from default LOAD selection.
- The app treats completed snapshots as read-only. Restoration copies their contents into DIR and leaves the source snapshot intact.
- A local SQLite database stores game configuration, checkpoint metadata including original data-directory paths and registration order, history entries and operation journals. Game files remain in the sibling directories, not in the database.
- History entries have stable IDs and a stable chronological order, including when timestamps are equal. Saved and Existing backup entries reference their saved checkpoints. Loaded and Reverted entries reference the checkpoint restored and the recovery checkpoint captured immediately before the operation. They may also reference the selected earlier history entry for display and audit; that relationship does not determine restore eligibility.
- SAVE, game exit and app restart never delete existing snapshots or history. Only an explicit, confirmed Flush history operation, including the mandatory Flush phase of Forget this game, deletes them. There is no automatic expiry or single UNDO PATH.
- If a snapshot is confirmed deleted or changed externally, mark that snapshot generation removed and hide history rows whose Restore/Revert action depended on it. Keep the original IDs and references internally for audit and interrupted-operation recovery. A newer folder at the same path never becomes the target of an older action. Temporary access failures remain unavailable states, not proof of removal.

The two history actions have fixed meanings:

- **Restore** loads the exact saved snapshot referenced by a Saved or Existing backup entry.
- **Revert** loads the recovery snapshot from immediately before a Loaded or Reverted entry.

Every Restore or Revert preserves the current DIR as a new recovery snapshot and appends a history entry. Selecting an older entry never removes later entries. Reverting a revert uses exactly the same rule as reverting a load.

For example:

| Time | Event | Recorded snapshots | Action |
| --- | --- | --- | --- |
| 19:25 | Saved | Saved A | Restore A |
| 19:31 | Loaded [19:25] | Restored A; preserved previous state as B | Revert to B |
| 19:38 | Saved | Saved C | Restore C |
| 19:42 | Loaded [19:25] | Restored A; preserved previous state as D | Revert to D |
| 19:43 | Reverted [19:42] | Restored D; preserved previous state as E | Revert to E |

### Manual checkpoints

Duplicating DIR in the platform's file manager is a supported way to create a checkpoint. The user must not have to register or import the copy manually, rename it into an app-specific format, or add app metadata to it.

Scan DIR's parent directory for matching checkpoints at startup and during periodic scans, and refresh discovery when opening the game's history or resolving the default LOAD target. Match complete native duplicate names derived from DIR's actual name, using the supported platform/file-manager naming rules and their localized variants. Do not treat every folder sharing DIR's name prefix as a checkpoint.

Register newly discovered manual checkpoints in the database without changing their folder names or contents. Repeated discovery of an unchanged checkpoint must not duplicate its history entry. Offer Restore and include it among the ordinary saved checkpoints eligible for default LOAD and confirmed Flush history. Database IDs identify checkpoint generations independently of folder names.

Use a manual checkpoint's folder modification time as an estimate when ordering default LOAD candidates, with durable checkpoint registration order as the tie-breaker. Preserve its original save time as unknown; neither this estimate nor discovery time is a claimed SAVE timestamp.

Recovery snapshots and app staging directories are separate from native duplicate copies and must never be discovered as ordinary saved checkpoints.

### External backup changes and visible history

A checkpoint is one observed generation of a directory, not a permanent claim on its name. Record its filesystem identity and a recursive change signature covering relative paths, entry kinds, sizes and modification times, including the root directory's modification time. Detect replacement of the directory as well as in-place edits, added/removed files and newer modification times inside it. Revalidate before Restore/Revert as well as during discovery. Metadata signatures are change detection, not proof that a game save is internally consistent.

When an accessible parent listing confirms a checkpoint is absent, retire that generation. When the same named folder has a different identity or change signature, retire the old generation and register the new ordinary saved folder as a fresh Existing backup with new snapshot and history IDs. Do this in one metadata transaction. Recognize a replacement even when deletion and recreation both happened between scans, while SaveScummer was stopped, or reused an app-created backup's name. Repeated scans of the new generation must not add duplicates. Names of retired backups may be reused by later SAVE operations; occupied names remain protected from overwrite.

An unreadable directory, an inaccessible parent or an unavailable drive does not establish deletion or replacement. Mark the affected snapshot temporarily unavailable without retiring its generation. If a complete scan cannot establish the new signature, retain the old generation as unavailable and retry during a later refresh. Do not register a partially inspected folder as a fresh checkpoint. Changed recovery directories invalidate their old Revert target but are never imported as ordinary saved checkpoints. Retain unresolved operation journals and recovery material independently of visible-history filtering.

The core supplies the visible history consistently to every client:

- Show a Saved or Existing backup row only while its own snapshot generation has not been removed. Show a Loaded or Reverted row only while its exact pre-operation recovery generation has not been removed. A retained recovery point can keep its operation visible even if the checkpoint that was loaded has since disappeared. Temporarily unavailable generations can remain visible with their actions disabled.
- Show the observed launch and close markers bounding a session only when that session contains at least one visible backup-backed action. Use the saved/action time, or the explicitly estimated modification time for a manual checkpoint, to associate it with a session. Hide intervening sessions with no remaining checkpoints, not just sessions older than the first checkpoint. Do not invent missing session boundaries.
- With no surviving saved or recovery points, the normal history is empty even if internal launch/close or removed-checkpoint records remain. Recompute the visible timeline after external changes, new checkpoints, Flush and restart. History-arrow availability follows visible history rather than internal audit-record counts.

Removing a row from the visible timeline does not delete files or rewrite old action references. Only confirmed Flush, either on its own or as the cleanup phase of Forget this game, deletes retained snapshot files and clears durable history.


## OPERATION SAFETY

The app restores files on disk. The user is responsible for making the game pick up the restored state, for example by reloading or restarting the game. This applies to both Restore and Revert.

Use ordinary, best-effort file copying for saved and recovery snapshots. Do not suspend the game, detect concurrent writes, add game-specific consistency logic or automatically retry copies. Actual filesystem or copy errors fail the operation under the safety rules below. Successful copying does not guarantee a consistent game save if the game was writing during the copy. The user decides whether to pause or close the game before requesting an operation.

All entry points (UI, global shortcuts and Explorer) use the same operation handling and per-game operation lock. Allow only one operation per game at a time, including Flush history, Forget this game and changes to configured paths. Reject additional requests while that game is busy; do not queue them for later execution. Disable conflicting UI actions and give brief, rate-limited busy feedback for shortcuts and Explorer requests. Holding a shortcut must not repeatedly trigger operations.

For every Restore or Revert:

1. Resolve the requested checkpoint ID and apply the shared game, kind, original-directory and generation-availability checks before creating any new snapshot. A history row contributes only the reference to its saved or recovery checkpoint.
2. Copy the current DIR into a new recovery snapshot. If DIR is missing or the copy fails, stop with a clear error and leave DIR untouched.
3. Prepare the requested replacement in a separate staging directory before changing DIR. Incomplete copies must not appear as usable snapshots.
4. Replace DIR while retaining enough data to recover if replacement fails. Attempt rollback on failure; retain recovery and staging data needed for recovery if rollback cannot complete, and report the failure.
5. Mark the operation complete only after successful replacement. Keep both the source snapshot and the newly captured recovery snapshot.

Filesystem changes and database changes cannot share one transaction. Persist a pending/completed/failed operation record so startup can identify interrupted operations. Record the original resolved live DIR, source checkpoint ID and exact source path, recovery snapshot, staging and retained-original paths, together with the filesystem identities/change signatures required to validate them. Persist the intended replacement step before changing paths and its result afterward, so startup can reconcile the record with the actual directories. Preserve their recovery files and surface the interruption instead of silently treating it as a successful load or deleting recovery data. Failed and pending operations must not appear as completed history actions.

SAVE also publishes a snapshot and its history entry only after its copy succeeds. Flush history and the Flush phase of Forget report deletion failures and retain records for remaining snapshots instead of claiming that cleanup completed.

### Interrupted-operation recovery

The host checks interrupted operations before allowing new operations for the affected game. A failure before the app changed live DIR normally needs only an error: verify that the interruption happened before replacement began, retain the operation record and any recovery material, and release the block. A pending record alone must not cause an unnecessary restore or recovery prompt.

Automatically roll back when the operation record and filesystem establish that restoring the retained original data will not overwrite possibly newer current data. For example, if DIR was moved aside and the host crashed before installing the replacement, restore the retained original to DIR. Verify the result and persist the resolution before clearing the block. Report "The previous load was interrupted. Your original save has been restored." Use corresponding wording for an interrupted Revert. Keep saved checkpoints and recovery snapshots; do not automatically retry the requested LOAD or REVERT. This recovery runs even when no UI is attached.

If replacement may have completed or current DIR may contain subsequent game progress, do not blindly roll back. Keep the game in "Recovery needed" and offer two choices in a small recovery prompt:

- **Keep current game data:** accept the current DIR without replacing it. Require an existing, accessible directory; disable this choice while DIR is missing. Recheck it when the command runs, then record the user's choice and resolve the interruption. This accepts the user's chosen files, not a claim that the app has validated the game's save format.
- **Restore data from before the interrupted operation:** use the complete recovery snapshot or verified retained original associated with that operation. If DIR exists, first preserve its current contents as a new recovery snapshot; if this fails, leave DIR untouched and keep recovery unresolved. Stage and restore the chosen pre-operation data, retaining the source and all earlier snapshots. This dedicated recovery action can recreate a missing DIR and must not fail merely because normal LOAD requires a current directory to preserve.

If permissions, file locks, missing recovery material or disk problems prevent recovery, show the specific error and provide **Open recovery folder** and **Retry recovery** actions. Opening the folder must not resolve the interruption. After fixing the filesystem manually, the user can retry recovery or choose Keep current game data for the repaired DIR; no database editing is required.

Recovery commands use the same core validation and per-game lock, target the interrupted operation's original data location, and remain available while ordinary SAVE, LOAD, REVERT, Flush history, Forget and path changes are blocked. Allow only one recovery attempt at a time. Recovery attempts must also be recorded durably and remain recoverable if interrupted again. Preserve all retained snapshots and uncertain files until an explicit Flush after recovery is resolved. Persist the resolution and its retained snapshot references without turning the original failed or interrupted action into a successful Loaded/Reverted history entry or playing its completion cue. Clear the block only after filesystem checks and the resolution record succeed; subsequent ordinary LOAD or REVERT requests are separate operations.

### Forget custom game

Forgetting is available only for a custom game and requires explicit confirmation. It is a single host-owned, idempotent operation, not a client-side sequence of Flush followed by an unrelated record deletion. The confirmation is bound to the same refreshed, revision-checked deletion scope as Flush and shows the saved-backup, recovery-point and incomplete-copy counts, with paginated Details paths when requested.

On confirmation:

1. Reconcile externally added, changed or removed checkpoints and reject a stale confirmation revision.
2. Acquire the game's operation lock and revalidate that it still exists, is custom, is idle and has no unresolved recovery.
3. Run the complete Flush deletion workflow, including retained incomplete copies. Leave the configured live DIR, game installation and executable untouched.
4. If every required deletion and metadata cleanup succeeds, atomically remove the custom game, its remaining per-game configuration and summaries, and its ACTIVE STACK reference from the visible library. Preserve only the minimal request/operation receipt needed to make a retry idempotent, outside the visible game library.
5. If any deletion, persistence step or crash recovery remains incomplete, keep the custom game registered and report or recover the operation; never show a forgotten result while cleanup is partial.

A custom game with no backups or history still passes through this zero-item Flush workflow before removal. A forgotten game cannot be recreated by known-game scanning. The user may explicitly add the paths again later, producing a fresh custom-game ID with no connection to the deleted history. Known games reject Forget at the core even if a client incorrectly presents the command.


## SAVE

When save is triggered, I want the app to:

1. Check if the game DIR exists. If not, finish the SAVE operation.

2. Create a copy of that dir as a new sibling folder following the platform's native convention for duplicate dirs (for example, for English Windows Explorer, it's "Void_War - Copy", "Void_War - Copy (2)" and so on). Choose the next available name, accounting for existing app-created and manual copies. Never overwrite an existing path; if a naming collision occurs, choose another available name. There can be multiple copies of DIR; this is expected.

3. After the copy succeeds, register the saved snapshot and append a Saved history entry. Keep all existing saved snapshots, recovery snapshots and history entries.


## LOAD

When load is triggered, I want the app to:

1. Select the latest eligible saved checkpoint directly from checkpoint metadata, including valid imported existing backups, unless a specific saved checkpoint ID was requested. Use the current game's resolved DIR and the shared eligibility and ordering rules above; do not search history to choose a source. Recovery checkpoints must never become the default LOAD target. If no saved checkpoint is available, do nothing and finish LOAD.

2. Preserve the current DIR and restore the selected snapshot using the OPERATION SAFETY steps.

3. Append a Loaded [target] history entry referencing the selected saved checkpoint and the new recovery checkpoint, retaining any selected history-entry reference as audit context. Its Revert action restores the state from before this particular load.

The Restore action on a Saved or Existing backup history entry runs LOAD with that row's exact saved checkpoint ID as the explicit target. The checkpoint determines eligibility even when the row was displayed before a rescan or configuration change.


## REVERT

When Revert is selected for a Loaded or Reverted history entry:

1. Take the exact recovery checkpoint ID referenced by that entry, captured immediately before the specific operation. Apply the same checkpoint validation as LOAD, requiring recovery kind instead of saved kind. If it is unavailable or belongs to another game or data directory, stop with a clear error.
2. Preserve the current DIR as a new recovery snapshot and restore the selected recovery snapshot using the OPERATION SAFETY steps.
3. Append a Reverted [target] history entry referencing the operation being reverted and the new recovery snapshot. Its own Revert action restores the state from before this revert.

Keep all earlier snapshots and history entries, including the recovery snapshot just restored.


## Global shortcuts

Ctrl+F5 (SAVE operation)
Ctrl+F9 (LOAD operation)

### Play sounds

SAVE and LOAD triggered through global shortcuts have separate start and completion cues. The background host's feedback adapter plays these independently of the UI. The start cue means the request was accepted and the operation has started; the completion cue means the file operation and its history record have successfully committed.

| Event | Sound |
| --- | --- |
| SAVE started | Two short, soft ascending notes |
| SAVE completed | A brighter ascending resolution |
| LOAD started | Two short, soft descending notes |
| LOAD completed | A rounded descending resolution |
| Operation failed or could not start | A distinct, low double knock |
| Request rejected because the game is busy | One quiet, dry tick |

Use the approved generated WAV files in `assets/sounds`: `save-start.wav`, `save-complete.wav`, `load-start.wav`, `load-complete.wav`, `operation-failed.wav` and `busy.wav`. Embed these assets in the host's platform feedback adapter. `scripts/generate-sounds.mjs` is the reproducible authoring source; generation is not required at build time or runtime. Keep the approved levels and timbres.

Start cues should be approximately 100 ms and completion cues approximately 200 ms. If an accepted operation fails, play the failure cue instead of completion. If it cannot start, play only the appropriate failure or busy cue. Rate-limit busy cues to avoid audio spam.

For fast operations, sequence the start and result cues so both remain distinguishable; audio timing must not delay file operations or extend the operation lock. Do not loop sounds during copying. Failures also leave a visible explanation in the game widget and produce a notification when the app is hidden.

Provide one app-wide "Play sounds" setting, enabled by default. It controls these cues without per-sound configuration.


## Explorer extension

I also want to register two Explorer extensions that would show up in the Explorer context menu:

Save - it should only appear if I right-clicked on DIR; it should do the save operation.
Load - it should only appear if I right-clicked on an ordinary saved copy of DIR; it should run LOAD using that backup, including preserving the current state and recording history. Recovery snapshots are accessed through their Revert actions in the app history.


## UI

Keep the UI slick, minimal and compact. Use spacing, restrained emphasis and control states to communicate routine activity. Avoid redundant headings, status captions and implementation details. Preserve clear action labels and show concise error text when user action is needed.

Base standard buttons on state-invariant geometry: always reserve the same transparent solid 2 px border, radius and padding, and indicate keyboard focus by changing only that border's color to a contrast-adjusted violet accent. Normal, hover, focused, default and disabled states must not change border width, padding, content bounds or outer size. Use separate deliberate variants for compact icon, split and borderless controls instead of compensating for state-dependent metrics.

In every dialog action row, use text-only buttons with no icons. Fill exactly the explicit default button with the violet accent so the filled treatment consistently communicates the action activated by Enter. The default remains fixed while focus moves between fields and buttons. Keep the focus border visible on both ordinary and accented buttons. Add custom game defaults to **Add**, Configure defaults to **Save**, and the destructive Flush history and Forget confirmations default to **Cancel**. Pressing Enter from ordinary dialog controls invokes that fixed default action.

### Main window

The main list shows running games followed by one permanent, expandable **Installed games** section. The section header is present even when its count is zero so discovery and manual addition are always reachable. Make the whole section header, apart from its independent actions, toggle expansion when one or more games are running. When none is running, keep the section expanded and make its expander non-interactive because collapsing it would only hide the library without exposing another game. Put its chevron, label and count on the left, render the count as a compact rounded bordered badge, and put **Scan for known games** plus a solid-ellipsis options button on the right. Put **Add custom game** in that options menu instead of showing it inline. At narrow widths, keep the labels readable and wrap the actions onto a second, right-aligned line within the same header instead of truncating them, hiding them or increasing the application's minimum width. Keep the header's vertical padding identical in expanded and collapsed states.

Show launched known and custom games above the section in ACTIVE STACK order. Group every nonrunning installed known game and every nonrunning custom game under Installed games; custom games remain there even when uninstalled. The count is the number of rows currently inside the section. A running custom game behaves normally: it moves into the running area and returns to Installed games when it closes. Confirmed-uninstalled known games are absent, while confirmed-uninstalled custom games remain visible. This visibility distinction comes from explicit game origin, not from whether paths were user-configured.

Expand Installed games by default when no game is running, and initially leave every row unselected in that state. When entering a running-games view, collapse it by default unless it contains the selected game; thereafter preserve the user's choice during ordinary refreshes. Expanding exposes its rows, and selecting a row reveals that game's controls and instructions. If a game closes, preserve its selection, move it into Installed games and expand the section if necessary to keep it visible. Collapsing the section while it contains the selection selects the first visible running game. If there are no rows inside the expanded section, show the compact text "No installed games. Scan for known games or add a custom game." within the section; do not replace the permanent section header with a whole-window empty state.

**Scan for known games** requests the host's ordinary rescan. Disable it while the host reports a scan in progress and temporarily label it **Scanning…**; do not start duplicate scans from repeated clicks. When the scan completes, compare the installed known games with those present when the action began and temporarily replace the action label with **No games found**, **1 game found** or **N games found**, using the singular only for one newly found game. Keep that result visible for a few seconds before restoring **Scan for known games**. A completed scan updates rows through normal state publication without changing an existing valid selection or expansion choice. Report a scan failure next to the section or in a concise error surface associated with the action, rather than attaching it to an arbitrary game row.

**Add custom game** opens the dialog specified below. After a successful addition, expand Installed games, select the new row and show its normal details. Cancel or validation failure preserves the previous selection and library state.

Use the existing `assets/icon.svg` for application branding, including the window icon and tray icon. Game widgets use each game's downloaded, cached Steam icon, with a neutral initials placeholder while missing or loading, as described under Steam artwork loading and cache. The application icon does not replace game icons. Give the main window icon and game icons matching 27 px boxes aligned to the same left edge, with the title-bar and game names aligned after the same 8 px gap. Preserve this alignment at narrower window widths.

Keep the window, title bar, help bar, game list, menus, ordinary text and borders in neutral grayscale for both light and dark appearances. Limit the icon's violet (`#9747FF`, adjusted for contrast where needed) to small functional accents: checked checkboxes, selected history actions, open dropdown controls and progress. Retain distinct semantic colors where needed, such as the Running indicator.

Use Font Awesome Pro's rounded **Duotone** SVG family for functional UI icons throughout the desktop app, except that ellipsis buttons use the normal solid Font Awesome icon so all three dots have one tone. Copy only the required SVGs from the shared asset library into the project, preserve their embedded license comments, and bundle them as Qt resources; do not depend on an installed icon font or external files at runtime. Keep application branding and downloaded game artwork unchanged. Render functional icons from the current palette at runtime: the primary layer uses the active `ButtonText` color, the secondary layer uses that same color at its SVG-defined opacity, and disabled icons use `Disabled/ButtonText`; theme changes update existing icons. Use semantic or violet accents only for the states described above.

Game rows fill the entire list width and meet their neighbors with no outer gaps, rounded row corners, surrounding borders, decorative left stripes, shadows or dividers between individual games. Hovering anywhere over a row gives its full width a subtle darker neutral background. The selected row retains a darker neutral background after the pointer leaves, extending to both inner window edges and touching adjacent rows. Keep text and controls at full readability; dim the background rather than reducing the row's opacity. Group each game's name, status and controls with internal padding. Give nonrunning and uninstalled custom games the same full-width geometry. The Installed games header also fills the width and uses matching hover shading across the entire header, with one subtle neutral dotted divider immediately above it and no surrounding gap; show this divider when one or more running widgets precede the section. Its action buttons are independent of the toggle, so clicking either does not expand or collapse the section. All statuses, including "Running", "Uninstalled" and "Not run yet", share the same text treatment and position immediately after the game title with the same small gap. Statuses have no dot or decorative icon; semantic text color can vary. ACTIVE STACK ordering and the inline Running status communicate process activity, independently of row selection.

Keep the main list container's padding at zero. Place spacing inside each game row: approximately 12 px vertically and 18 px horizontally on desktop, reduced to 10 px vertically and 14 px horizontally in narrow windows. Align the title-bar icon with this same horizontal inset. Do not impose a minimum or maximum width on game rows, the nonrunning group or instruction text; they use the window's available width. Keep the controls compact and aligned left, with 6 px between controls and 32–36 px action buttons on desktop. The selected game's action row contains Save first, then split Load and "...". There is no info button. Make the Save and main Load captions slightly larger, and render their icons at 1.5 times the former compact size, centered together with each caption. Put Explore inside every game's "..." menu. Keep the Installed games header immediately after the running widgets. Do not reserve space for hidden controls, hidden instructions or a closed dropdown.

Both the Load history and the "..." menu open downwards, directly below their own triggering button with a small gap. Position them from the actual button bounds, not a fixed offset from the game header. Use floating popup surfaces that can extend beyond the main window, without enlarging the game widget or shifting the underlying controls. Keep popups within the available screen width and constrain long history lists with scrolling. Only one popup is open at a time; clicking outside or pressing Escape dismisses it. Implement every ellipsis menu through one shared popup-menu component. Give each one-line menu item a fixed-height square icon column and center its icon within that column, so every menu uses identical icon and text alignment. Ellipsis triggers use the ordinary bordered button treatment and a square size matching neighboring action buttons.

Keep the app-wide checkboxes and keyboard hints in a persistent bottom help bar, outside the scrollable game list. From left to right, show "Save  Ctrl+F5" and "Load  Ctrl+F9", then the "Play sounds" checkbox. Place "Launch on startup" last, detached at the far right with flexible space before it. At narrow widths, wrap in this reading order without hiding labels or controls, keeping the startup setting aligned to the right. The shortcut hints refer to the active game at the top of ACTIVE STACK. Do not duplicate these settings above the game list.

The selected game widget should show the following; unselected rows show only their icon, name and status:

```text
ICON + NAME (Running)

|----| |---------------|---|
|Save| |      Load     | ↓ | [...]
|----| [ 4 seconds ago ]---|

[Game-specific instructions]
```


The action row appears above the instructions. Give Save and the complete split Load control (including its dropdown arrow) the same outer width and height. Use the larger of their preferred sizes, accounting for the Load timestamp, font scaling and translated labels; in Qt, derive this from their size hints rather than assuming the Save label is wider. Keep the pair compact and left-aligned. In narrow windows, shrink both equally to fit, preserving equal widths; elide the timestamp if necessary and expose its full value on hover and through accessibility. The "..." button stays compact and separate.

Include a detected-but-never-run state in the UI demo: the game is installed, but has not created DIR and has no saved snapshots or history. Keep it listed among the nonrunning games. Disable Save and both parts of Load, and omit the timestamp beneath Load entirely, with the Load caption vertically centered. A subdued "Not run yet" label beside the game name can identify this demo state. Also include an uninstalled custom game to verify its persistent row and status. Keep configuration accessible in both states.

Determine action availability from the resolved data and snapshot/history availability, not merely from installation state or whether the monitor has observed a launch. A closed or uninstalled custom game with existing data and backups retains its normal Save/Load controls. The absence of observed launch events alone does not establish that a game has never run. Status precedence is `Recovery needed`, then `Running`, then `Uninstalled`, then `Not run yet`; omit the status when none applies.

### Add custom game

Open a small modal dialog with this content:

```text
Game executable: [................................] [Browse…]
Save location:   [................................] [Browse…]
Name:            [................................]

                                      [Add] [Cancel]
```

Keep the fields in that order and make the dialog size to its content and non-resizable. Require a nonblank trimmed name and nonblank absolute executable and save-location paths. Browse uses a file picker for the executable and a directory picker for the save location, while retaining editable fields so expected paths that do not exist yet can be entered. When the user chooses an executable with Browse and Name is still blank, fill Name with the executable's complete filename minus its final extension; never overwrite a nonblank Name. Do not show Reset controls because a custom game has no catalog defaults. Disable Add while a request is pending, show validation errors in the dialog, preserve all entered values after rejection and create no partial entry. The host generates the stable ID; the desktop must not derive it from the name.

Make **Add** the dialog's fixed, violet-accented default button, so Enter submits the form regardless of which ordinary field or non-default control has keyboard focus. Keep both Add and Cancel text-only, and show focus independently from the default treatment.

### Game selection and info

Clicking anywhere in a game's row selects it. Give collapsed game rows a pointing-hand cursor to communicate this interaction. Exactly one visible game is selected whenever a running game exists or the user has explicitly selected an installed row; the initial no-running view deliberately has no selection. The selected game shows both its action buttons and its `info` directly below them; every other game hides both controls and instructions and leaves no space for them. Clicking the selected row again keeps it selected. Selecting another game hides the previous game's controls and instructions and dismisses its open popups. This replaces the separate info toggle and all per-game info expansion preferences.

Activating any game action button also selects its owning game and performs the action once; selection must not consume the action or require a second click. This includes Save, both parts of Load, "...", its menu actions and history Restore/Revert actions. Keep row selection available when the game is closed, has never run, is busy or needs recovery; action availability still follows the existing rules. Expose an accessible selected state and allow keyboard selection with Enter or Space, while retaining ordinary keyboard access to each action and selectable instruction text.

Initially select the game at the top of ACTIVE STACK when a game is running. When none is running, initially expand Installed games with no row selected. Preserve an existing selection through state refreshes and running/nonrunning regrouping rather than switching whenever process focus changes. If the selected game disappears, select the first visible running game, or clear selection when none is running. If the user collapses Installed games while it contains the selection, select the first visible running game; if there is none, clear the visible selection until the user explicitly selects a row after expanding it again. Row selection is UI state: it does not launch a game, mark it Running, reorder ACTIVE STACK or change the target of global shortcuts.

Show the selected game's instructions as selectable, wrapping text with compact paragraph and numbered-list spacing. Let the text expand across all available row content width. Number Save and Load procedures separately, each starting at 1. Remove the instruction area's gray border, rounded box and additional inset padding. Use the row's shared horizontal padding for the title, controls and text; leave approximately 10 px between the controls and instructions, and retain the row's bottom padding so text stays clear of adjacent rows and the outer window border. The selected row's darker surface extends behind the instructions. Showing details increases the row's height and moves following games down; it must not overlay controls or introduce an inner scroll area. The main game list handles scrolling when needed. Popups do not reset selection. The demo initially shows running Void War selected with its controls and instructions visible and its history dropdown closed.

When the selected game's `info` is empty, show "No instructions available for this game yet." in its instruction area. The demo includes the Void War instructions above and uses this empty state for games without supplied instructions. Keep the shared progress indicator attached only to Save/Load. Selecting another row does not cancel a running operation; selecting its game again shows its current progress and disabled controls.

### Busy state and progress

Whenever SAVE, LOAD, REVERT, Flush history or Forget runs for a game, immediately disable both its Save and Load buttons, including the Load dropdown arrow. Keep their normal labels and subtly dim the disabled controls. Show one thin shared progress bar integrated along the bottom of the Save/Load control group, without expanding the widget or shifting its layout. Apply this state regardless of whether the operation was started from a button, history, a shortcut, Explorer or a confirmation dialog. If the window was hidden, opening it must show the current busy state and progress.

Do not add routine status captions such as "Saving...", "Loading..." or "Finishing...", percentage text, or "disabled" labels. The control state and progress bar provide the visible feedback. Expose the operation and progress through accessibility properties. Show measured progress when the amount of work is known, covering all required copies rather than only the first copy. Use an indeterminate bar while calculating the work or during phases without measurable progress. Do not fill the bar completely or announce completion until the files and history record have successfully committed.

```text
[Icon] Void War  Running

[ Save ]  [ Load  v ]  [ ... ]
━━━━━━━━━━━━────────
```

In this mockup, Save and Load are visually dimmed and inactive; the thin line is the progress bar. No extra visible label is added.

Restore and Revert actions in any already-open history, Flush history, Forget this game, and changes to configured paths are also disabled for that game while busy. Backend locking enforces the same restriction for every entry point; disabling buttons alone is insufficient.

Keep the busy state until the operation and any required rollback have finished. After success or a safely handled failure, remove the progress bar and re-enable controls according to snapshot availability. Show failures as an error, not as a completed progress bar. If rollback cannot finish or an interrupted operation has an uncertain outcome, show "Recovery needed" with an action to open the recovery prompt described above. Retain the recovery files and block ordinary SAVE, LOAD, REVERT, Flush history, Forget and path changes until recovery is resolved. Keep the dedicated recovery choices and folder access available, disabling conflicting recovery choices while a recovery attempt is running. Opening or reconnecting the UI must retrieve the current recovery status and available choices from the host.


### Save

Save is a button that does the SAVE operation. Disable it while the game's data DIR is absent, including before a newly installed game has created its initial data.


### Load

Load is a split button. The main button runs the default LOAD operation using the latest eligible saved checkpoint selected by the core. The arrow opens the game's history, newest first, grouped by day.

- Saved and Existing backup rows offer **Restore**.
- Loaded [target] and Reverted [target] rows offer **Revert**.
- Game started and Game closed rows are compact, visually subdued markers without actions, shown only for sessions containing surviving backup-backed actions. They remain in the same daily timeline rather than creating separate session panels.
- Temporarily unavailable snapshots are clearly marked, with their affected actions disabled. Rows for removed or superseded snapshot generations are omitted from the normal timeline.

References such as [19:25] identify the target history entry. Include the date or additional detail when needed to distinguish targets; use stable IDs internally. Selecting Restore or Revert runs that action for the selected row immediately and appends the resulting event after success.

The core-selected eligible saved checkpoint's time appears below the main Load caption in smaller, subtler type. When no saved checkpoint is eligible, omit this secondary line rather than showing a placeholder or invented time. Relative labels can use the following formats:

- 4 seconds ago
- 2 minutes ago
- 1 hour and 12 minutes ago
- yesterday, 23:20:12
- Wednesday, 12:23:22
- 2012-12-12, 12:12:21

History rows use a consistent-width, two-line time column so that operations and their targets can be distinguished and row content stays aligned. For today's entries, show the same concise relative age used beneath Load (for example, `3 seconds ago`) on the first line; for older entries use `Yesterday`, a full localized weekday, or `yyyy-MM-dd`. Show the exact `HH:mm:ss` time on the second line. Size every row's time column for the longest supported relative label or localized weekday so text is never abbreviated, and align its left edge exactly with the day-group heading above it. Give each history description a small semantic icon for its entry kind (saved, imported backup, loaded, reverted, game started or game closed). The history list scrolls when it exceeds the dropdown's available height.

Opening history requests the first page for that game. Load older pages as the user scrolls, preserving day grouping, stable row IDs and exact Restore/Revert targets. Bound cached pages and rendered widgets so a long browsing session does not retain the entire timeline. The history arrow's availability comes from the host summary, not from whether the client has fetched any rows.

Discard replies for a closed popup, a different game or an earlier host instance. If the history revision invalidates a cursor, reload and preserve the visible row as an anchor when it still exists. Reconnecting retrieves the current summary and any open history page without replaying commands. Busy and recovery restrictions apply immediately to loaded rows as well as newly fetched pages; the core revalidates every action at execution.

When the game is idle, disable the main Load button if no saved snapshot is available and show the secondary caption **No checkpoints saved** in the usual timestamp position. Keep the history arrow available whenever visible history exists, including when only recovery-backed operations remain. Internal removed-checkpoint records and unrelated session markers alone do not enable it. During an operation, the busy-state rules disable both parts of the control. Revert actions live in history; there is no separate single-undo button. Disabled buttons use the platform's forbidden cursor on hover so their unavailable state is unmistakable.

Example with several relaunches in one day (the history dropdown is open):

```text
[Icon] Void War  Running

[ Save ]   [     Load      v ]   [ ... ]
                19:50
           Today

           19:50  Saved                       [ Restore ]
           19:44  Game started
           19:43  Reverted [19:42]             [ Revert  ]
           19:42  Loaded [19:25]               [ Revert  ]
           19:41  Game closed
           19:38  Saved                       [ Restore ]
           19:33  Game started
           19:32  Loaded [19:25]               [ Revert  ]
           19:31  Game closed
           19:25  Saved                       [ Restore ]
           19:20  Game started
           12:45  Game closed
           12:40  Saved                       [ Restore ]
           12:10  Game started
```

Loads and reverts can happen while the game is closed. Starting it again does not reset history or recovery points. Start and close markers describe observed events; the app must not invent exact event times for periods when it was not monitoring the game.


### ...

Should open a dropdown with miscellaneous actions:

- Explore

    Open the directory containing DIR in the OS file explorer. This action is inside the menu for every game, including detected-but-never-run games when the parent directory is accessible. Keep folder access available during Save, Load, Revert and recovery.

- Configure

    For a known game, open a small popup where you would see:

Game executable: [...prefilled path...][open icon] [Reset]
Game data dir (DIR): [...prefilled path...][open icon] [Reset]

[Save] [Cancel]

    For a custom game, also show an editable Name field, label DIR as Save location consistently with the Add dialog, and omit Reset because no catalog defaults exist. Changing and saving configuration updates the resolved game only after core validation succeeds. Show field and path errors in Configure, preserve the previous committed configuration if validation fails, and keep the entered values in the open dialog for correction.

    Size Configure to its content and make it non-resizable, without unused child controls or blank areas. Make **Save** the fixed, violet-accented default activated by Enter. Keep Save and Cancel text-only, and do not let keyboard focus move the default treatment between them.


- Flush history...

    Enable this action when any saved snapshots, recovery snapshots or history entries exist and the game is neither busy nor awaiting recovery. Recovery data retained after an interrupted operation is included only after that interruption has been resolved. Enforce this restriction in the core as well as the UI.

    Keep the confirmation brief: "Permanently delete all backups and clear this game's history?" Add "Your current game data will be kept." Show separate counts for saved backups, recovery points and incomplete copies, plus their paths, in Details. Include any retained incomplete recovery copies in the deletion scope. Make **Cancel** the fixed, violet-accented default activated by Enter; Delete backups and Cancel are text-only, and focusing Delete backups does not promote it to the default.

    On confirmation, refresh backup discovery before checking the preview revision. External deletion, replacement, modification or discovery of a backup invalidates the old confirmation and requires a new preview. On acceptance, remove the game's saved and recovery snapshots, including imported existing backups, and clear its history. Leave the current DIR untouched. Only clear records for snapshots whose deletion succeeded; report any failures. Forget this game invokes this same cleanup as its mandatory first phase; no other app action deletes retained snapshots and history.

- Forget this game

    Show this item only for custom games, separated from the ordinary actions at the bottom of the menu. Disable it while the game is busy or has unresolved recovery. Known games never expose it.

    Open a revision-bound confirmation headed `Forget "GAME NAME"?` and explain: "All backups, recovery points, incomplete copies, and history for this game will be permanently deleted. The installed game, executable, and current save data will be kept." Show the same separate counts and expandable, paginated path details as Flush. If there is nothing to delete, show zero counts but still require confirmation and execute the zero-item cleanup phase. Make **Cancel** the fixed, violet-accented default activated by Enter; Forget game and Cancel are text-only, and focusing Forget game does not promote it to the default.

    Confirmation starts the single Forget operation described under operation safety. Keep the row present and busy until cleanup and library removal have both committed. If cleanup fails or is interrupted, retain the row and show the failure or recovery state so the user can retry. On success, remove the row, close its popups, and select the first remaining visible game without showing a second success prompt.


## Autolaunch

The bottom help bar ends with [X] Launch on startup, detached on the far right. The Save/Load keyboard hints and Play sounds checkbox appear before it as described under Main window.

Enabling Launch on startup registers the canonical `SaveScummer.Host` executable from the application directory with `--minimized` and the canonical absolute `SaveScummer` path. At sign-in, this starts or reuses the background host and shows its tray icon without opening the main window. The host's core must not require a UI client to initialize or operate.

By default, launching `SaveScummer` starts or reuses `SaveScummer.Host` and shows and focuses the main window. Closing the main window leaves the background host running in the tray. UI termination or disconnection does not cancel core operations.

Clicking on the tray icon launches or focuses the canonical sibling `SaveScummer` executable and connects it to the existing host.

The tray icon's context menu has two items:

- Main window
- Exit

Exit requests shutdown of the background host and UI. Stop accepting new operations, let any active operation and required rollback reach a safe stopping point, persist state, then release platform integrations and exit. Do not shut down merely because the UI disconnected.


# OS specifics

## Naming conventions for directory copies

Ordinary saved snapshots use native duplicate-folder naming, so app-created checkpoints and manual copies fit the same workflow. Naming and recognition must account for the supported file manager's localized conventions; do not replace native saved-checkpoint names with a universal app-specific scheme.

- Windows: Explorer's duplicate-folder convention (for example, in English: "Void_War - Copy", "Void_War - Copy (2)").
- macOS: Finder's duplicate-folder convention, including localized variants.
- Linux: the supported file manager's duplicate-folder convention, including localized variants. Define and test naming and recognition for each supported file manager rather than assuming one Linux-wide convention.

Recognized native copies are accepted without proof of app ownership. Keep their original names and contents. Recovery snapshots alone use the reserved "<DIR name>.recovery-<unique ID>" convention on every platform, with filesystem-safe names and collision handling. They remain distinguishable from ordinary checkpoints and are excluded from default LOAD selection.

## Global shortcuts

- Windows: possible
- macOS: unknown
- Linux: unknown


## File explorer, Finder extension or variants

- Windows: possible to extend the Explorer context menus
- macOS: unknown
- Linux: unknown
