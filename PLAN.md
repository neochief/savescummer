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
├── Cargo.toml                    # Workspace manifest; [workspace.package] version is the single source
├── Cargo.lock
├── CMakeLists.txt                # C++ desktop; reads the app version from Cargo.toml
├── build.ps1                     # Single Windows entry point: dev, release, clean
│
├── apps/
│   ├── host/                   # Rust background-host executable
│   ├── cli/                    # Rust command-line client executable
│   └── desktop/                # C++ / Qt Widgets main executable
│       ├── src/
│       ├── compat/             # Build-compatibility headers (Qt 6.5 on MSVC 19.38+)
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
│
├── packaging/                  # Per-platform distribution and installer files
│   └── windows/
│       ├── licenses/           # LGPL/GPL notices bundled into every package
│       └── installer/          # Inno Setup definition for the 1.0 installer
│   # (macos/ and linux/ are added as those platforms qualify)
│
├── scripts/                    # Build, test, packaging, release commands
│   ├── package-common.ps1      # Platform-neutral packaging core (shared by all OS scripts)
│   ├── package-windows.ps1     # Windows packaging: Qt deploy, VC runtime, portable ZIP
│   ├── release-github.ps1      # Draft-first GitHub Release publisher (local and CI)
│   ├── release.ps1             # One command: bump version, check, commit, tag, push
│   ├── setup-qt.ps1            # Multiplatform pinned-Qt bootstrap (aqtinstall)
│   └── build-desktop, check, explorer helpers, ...
│
├── .github/workflows/          # CI and tag-triggered release workflows
├── PLAN.md
├── PLAN-UI.md                  # Main-window UI plan and mockups
├── PLAN-ERRORS.md              # Failure, interruption and notice catalog
├── PLAN-INFRA.md               # Build, distribution and release plan (implementation steps)
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

### Build, distribution and releases

Keep build, packaging and release tooling a separate, well-defined layer so a
complete reimplementation reproduces the same output layout. Treat the
application version as a single source of truth: the Cargo workspace manifest
(`[workspace.package] version`). The CMake project and every packaging/release
script read that version, and the release tag must equal `v<version>`.

Generated output lives outside the committed source tree, separated by purpose
(all gitignored):

- `target/` — Cargo cache only. Scripts never place anything else here; removed
  only by a deep clean.
- `build/` — regenerable intermediates: `dev/` and `release/` CMake trees, logs,
  build reports and the dev package; `tmp/` holds disposable scratch (including
  the test temp directory). Wiped by a normal `clean`.
- `dist/` — release distributables, flat and OS/arch-tagged:
  `SaveScummer-<os>-<arch>-<version>.<ext>` (for example
  `SaveScummer-windows-x64-0.1.0.zip`, later
  `SaveScummer-macos-arm64-0.1.0.tar.gz` and
  `SaveScummer-linux-x86_64-0.1.0.tar.gz`). Every archive contains one
  top-level per-platform folder holding the canonical executables.
- `.runtime/` — machine-local state that must survive a clean: vendored SDKs
  (Qt, CMake tools), the aqtinstall environment used by `scripts/setup-qt.ps1`,
  and development app data. `clean` never touches it.

One entry point, `build.ps1`, orchestrates the Windows dev, release and clean
flows: compile Rust and the Qt desktop, run optional tests, package, and write
a build report. `clean` removes `build/` and `dist/`; `clean -Deep` also removes
`target/`; it gracefully stops a recorded development host first and never
removes `.runtime/`. Windows builds default to the VS 2019 generator and fall
back to the newest installed Visual Studio when it is absent; Qt 6.5 headers on
MSVC 19.38+ use the compatibility header in `apps/desktop/compat`.

Packaging is per platform but shares a platform-neutral core.
`package-common.ps1` owns version reading, staging layout, the package
manifest, checksum generation and identity templates; each `package-<os>.ps1`
adds only OS-specific steps (Windows: Qt deployment, Visual C++ runtime, ZIP,
PDBs; later macOS: app bundle and dmg; Linux: tarball or AppImage). The Windows
1.0 distribution is an Inno Setup per-user installer (primary) that registers
the Explorer extension and enables sign-in autostart by default on a first
install (upgrades keep a user's opt-out) and ships an uninstaller that preserves
the user's backup data, plus a portable ZIP (secondary) carrying the same
binaries and an opt-in Explorer registration. No packaging script may reproduce
policy already owned by the shared core.

Releases are published to GitHub Releases from a `v<version>` tag. Creating a
release always produces a draft with auto-generated notes for human review
before it is published. The release asset is the platform installer — the single
file users need — and GitHub adds its own generated source archives to every
release regardless; the portable archive and `.sha256` sidecars are explicit
opt-ins of the release scripts.
`scripts/release-github.ps1` publishes the draft locally with the GitHub CLI;
`.github/workflows/release.yml` builds and tests the same artifacts on a per-OS
matrix from the tag and attaches every platform's output to one draft, so the
local and CI paths never diverge.

`.github/workflows/ci.yml` runs on every push, pull request and manual dispatch.
Windows must pass `scripts/check.ps1`, the Qt desktop build and its tests.
Ubuntu and macOS run the portable Rust checks as non-blocking porting signals.
Fresh machines bootstrap the pinned Qt SDK with `scripts/setup-qt.ps1`
(aqtinstall; Windows, macOS and Linux) and CI caches the result; the Rust
toolchain is pinned by `rust-toolchain.toml`.

### Application processes

Run one `SaveScummer.Host` process per user. It owns the core application, persistent state, operation locks, startup recovery, scanning and game monitoring. It must operate without a main window or client: shortcuts, Explorer commands and CLI commands must still work, operations must finish, history must persist, and sound/notification feedback must remain available.

`SaveScummer` is an optional graphical client that connects to the host through a local command/query/event interface. Opening it starts `SaveScummer.Host` from the same application directory when necessary and attaches to the single existing host otherwise. Closing or crashing the UI must not stop the host or cancel an operation. Reopening the UI retrieves current state and progress; it must not depend on events received by the previous UI instance.

`SaveScummer.CLI` is an optional console client of the same service contract. A command that needs a host starts the canonical sibling `SaveScummer.Host` when necessary, waits for readiness and sends the request. The CLI never becomes a second host and never implements application operations locally. Commands that only resolve local CLI syntax or print help do not start the host.

The host composes modules and manages their lifecycle. Business rules live in the core application and can also run inside a test process without starting a background host, desktop window or IPC server. The UI and CLI depend on the service contract and can run against a fake service during development.

### Modules

| Module | Owns | Boundary |
| --- | --- | --- |
| UI | Presentation of games, history, instructions, errors and configuration dialogs | Sends commands and renders returned state/events, including cached artwork supplied by the host. Does not download artwork, copy game files, access SQLite directly, scan installations or implement operation policy. |
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

- Commands: Save, Load (default or an explicit saved checkpoint ID), Revert (the exact recovery checkpoint ID referenced by a selected Loaded/Reverted history entry), retry interrupted-operation recovery, resolve recovery (an explicit interrupted operation and choice), confirmed Flush checkpoints, add and configure a custom game, atomically forget a custom game after confirmed cleanup, configuration updates for known games, rescan and request an asynchronous artwork cache check for known games.
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
- Test the UI against a fake service that can produce busy, failure, unavailable-snapshot and disconnected states. UI-specific coverage is specified in `PLAN-UI.md`; failure and interruption scenarios and their tests are specified in `PLAN-ERRORS.md`. Keep every dialog's Enter/default action fixed against keyboard focus and keep dialog action rows free of icons.
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

Custom entries persist regardless of installation state. A missing executable hides the custom game from the sidebar instead of showing an `Uninstalled` status; its configuration, checkpoints and history are retained, and the ordinary core rules apply whenever the configured DIR and required checkpoints remain available. Rescanning re-evaluates the configured executable; it never replaces custom paths with catalog discoveries or converts a custom entry into a known one merely because names or paths resemble a catalog game.

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

Normalize paths and resolve existing directory aliases before comparing them, respecting the filesystem's case rules. Compare directory components and identity rather than raw string prefixes, so "Game" and "Game2" do not conflict. DIR may not exist yet: resolve its existing ancestors and validate the intended location without creating it. Recheck resolved locations before file operations so changed paths cannot bypass validation. A save directory that is a symbolic link, junction or other alias is resolved to its real target; operations run against the target, and the link itself is never renamed, replaced or copied. A link that is repointed, broken or otherwise no longer resolves to the configured directory fails validation until the location is configured again.

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
- SAVE, game exit and app restart never delete existing snapshots or history. Only an explicit, confirmed Flush checkpoints operation, including the mandatory Flush phase of Forget this game, deletes them. There is no automatic expiry or single UNDO PATH.
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

Register newly discovered manual checkpoints in the database without changing their folder names or contents. Repeated discovery of an unchanged checkpoint must not duplicate its history entry. Offer Restore and include it among the ordinary saved checkpoints eligible for default LOAD and confirmed Flush checkpoints. Database IDs identify checkpoint generations independently of folder names.

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

All entry points (UI, global shortcuts and Explorer) use the same operation handling and per-game operation lock. Allow only one operation per game at a time, including Flush checkpoints, Forget this game and changes to configured paths. Reject additional requests while that game is busy; do not queue them for later execution. Disable conflicting UI actions and give brief, rate-limited busy feedback for shortcuts and Explorer requests. Holding a shortcut must not repeatedly trigger operations.

For every Restore or Revert:

1. Resolve the requested checkpoint ID and apply the shared game, kind, original-directory and generation-availability checks before creating any new snapshot. A history row contributes only the reference to its saved or recovery checkpoint.
2. Copy the current DIR into a new recovery snapshot. If DIR is missing or the copy fails, stop with a clear error and leave DIR untouched.
3. Prepare the requested replacement in a separate staging directory before changing DIR. Incomplete copies must not appear as usable snapshots.
4. Replace DIR while retaining enough data to recover if replacement fails. Attempt rollback on failure; retain recovery and staging data needed for recovery if rollback cannot complete, and report the failure.
5. Mark the operation complete only after successful replacement. Keep both the source snapshot and the newly captured recovery snapshot.

Filesystem changes and database changes cannot share one transaction. Persist a pending/completed/failed operation record so startup can identify interrupted operations. Record the original resolved live DIR, source checkpoint ID and exact source path, recovery snapshot, staging and retained-original paths, together with the filesystem identities/change signatures required to validate them. Persist the intended replacement step before changing paths and its result afterward, so startup can reconcile the record with the actual directories. Preserve their recovery files and surface the interruption instead of silently treating it as a successful load or deleting recovery data. Failed and pending operations must not appear as completed history actions.

SAVE also publishes a snapshot and its history entry only after its copy succeeds. Flush checkpoints and the Flush phase of Forget report deletion failures and retain records for remaining snapshots instead of claiming that cleanup completed.

### Interrupted-operation recovery

The host checks interrupted operations before allowing new operations for the affected game. Recovery is deterministic and never asks the user to choose; the rules below run at startup, including while no UI is attached, and the outcome is recorded on the operation. The user-visible messages and buttons are cataloged in `PLAN-ERRORS.md`.

1. **Nothing on disk changed.** Termination before the live directory was touched (preparation, recovery copy, staging, publishing a save) clears the operation as failed and releases the game.
2. **The original can be verified and the live location is empty.** If DIR is missing and the retained original is verifiably intact, restore the retained original to DIR. The requested operation never took effect. Persist the resolution before clearing the block.
3. **Completion can be verified.** If the replacement was installed (verified against the recorded staging identity) but the history entry was not committed, finish the operation and commit the history entry exactly as a completed SAVE, LOAD or REVERT.
4. **Nothing can be verified.** Keep the current DIR exactly as it is, mark the operation failed without a history entry, and retain all recovery material. Release the game when a coherent DIR exists; otherwise keep it blocked and surface the sticky error, retrying automatic resolution at each host start.

Never blindly roll back over possibly newer current data, never retry the requested LOAD or REVERT automatically, and never delete retained material while an interruption is unresolved. Keep saved checkpoints and recovery snapshots. This recovery runs even when no UI is attached. The host `recover` command remains a capability but is not exposed in the main window; it is retained for a possible future management screen.

### Forget custom game

Forgetting is available only for a custom game and requires explicit confirmation. It is a single host-owned, idempotent operation, not a client-side sequence of Flush followed by an unrelated record deletion. The confirmation is bound to the same refreshed, revision-checked deletion scope as Flush and shows the saved-backup, recovery-point and incomplete-copy counts, with paginated Details paths when requested.

The main window does not expose this operation; custom games are treated as permanent hidden records. The host capability is retained for a possible future management screen.

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

While the desktop window is focused, a shortcut acts on the selected game's Save or Load control, including a stopped game. Otherwise it targets the game at the top of ACTIVE STACK. When neither exists, the shortcut has no target.

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

For fast operations, sequence the start and result cues so both remain distinguishable; audio timing must not delay file operations or extend the operation lock. Do not loop sounds during copying. Failures also produce a notification when the app is hidden; the in-window presentation is specified in `PLAN-UI.md` and `PLAN-ERRORS.md`.

Provide one app-wide "Play sounds" setting, enabled by default. It controls these cues without per-sound configuration.


## Explorer extension

I also want to register two Explorer extensions that would show up in the Explorer context menu:

Save - it should only appear if I right-clicked on DIR; it should do the save operation.
Load - it should only appear if I right-clicked on an ordinary saved copy of DIR; it should run LOAD using that backup, including preserving the current state and recording history. Recovery snapshots are accessed through their Revert actions in the app history.


## Autolaunch

Enabling Launch on startup registers the canonical `SaveScummer.Host` executable from the application directory with `--minimized` and the canonical absolute `SaveScummer` path. At sign-in, this starts or reuses the background host and shows its tray icon without opening the main window. The host's core must not require a UI client to initialize or operate.

The Windows per-user installer enables this by default on a first install (task `checkedonce`; an upgrade presents it unchecked so an in-app opt-out is preserved). The portable package itself writes no sign-in entry: only the in-app checkbox or `startup on` adds one.

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


