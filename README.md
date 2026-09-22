# SaveScummer

SaveScummer helps you save and restore progress in games where that isn't possible by design. It helps you learn difficult games faster and spend less time replaying what you already know. Roguelikes, permadeath, Ironman modes — experience them with less pain and more fun. Checkpoint before risky moments, experiment, fail, learn, and keep going.

If your time is limited, it helps you reach interesting stories, builds, and decisions without losing hours of progress before you got gud.

If you or your child is an anxious player, it lets you keep playing, experimenting, learning, and having fun without being punished for every mistake.

## How it works

Most games persist progress on disk in one way or another. SaveScummer gives you keyboard shortcuts you can use in-game to checkpoint that progress and restore it later. It can also create checkpoints automatically at regular intervals, so even if you forget to save manually, you can still avoid losing a considerable amount of time after an unexpected death.

Depending on the game, restoring progress may require returning to the main menu or even relaunching the game. It may not be perfectly convenient, but it's still much faster than repeating an evening-long run after one stupid mistake or non-optimal choice.


---

## Tech info

The first Windows runtime is implemented in Rust. The background host owns SQLite,
game monitoring and file operations. The C++ / Qt 6 Widgets desktop and command-line
clients share the same local service contract.

## Build and test

The main Windows entry point builds **both Rust and Qt** from current source:

```powershell
./build.ps1 dev -Run       # Compile incrementally and open the development app
./build.ps1 dev -Run -Demo # Simulated data, no game operations
./build.ps1 release        # Compile optimized binaries and create a portable ZIP
./build.ps1 release -Test  # Also run Rust and Qt/host integration tests
./build.ps1 clean          # Remove regenerable outputs (build/ and dist/)
./build.ps1 clean -Deep    # Also remove the Cargo cache (target/)
```

Release output: `dist/SaveScummer-windows-x64/bin/SaveScummer.exe` and
`dist/SaveScummer-windows-x64-<version>.zip` (the version comes from
`Cargo.toml`). `./build.ps1 release` also produces the per-user installer
`dist/SaveScummer-windows-x64-<version>-setup.exe` when Inno Setup 6.3+ is
available (`./scripts/setup-innosetup.ps1`). Keep the extracted folder
together. Its application executables are `SaveScummer.exe`,
`SaveScummer.Host.exe`, and `SaveScummer.CLI.exe`. See
[the build guide](docs/building.md) for prerequisites, profiles, development data,
timings, lower-level commands and installer details.

Install Rust with rustup and the Visual Studio C++ build tools. The repository pins
the toolchain in `rust-toolchain.toml`; SQLite is built from its bundled source.

```powershell
# If Rust was just installed and this shell has not picked up its PATH:
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

`scripts/check.ps1` runs these verification commands. Tests create their own temporary
directories and databases. Process tests launch only test-owned processes; restart
tests terminate a child at a deterministic durable operation phase. The ignored
`crash_worker` test is a subprocess fixture invoked by the passing restart tests,
not a skipped recovery scenario.

## Qt desktop

Install Qt 6.5 or newer (Widgets, Network, SVG and Test), CMake 3.21 or newer,
and a compatible C++ compiler. On Windows with the local Qt 6.5.3 SDK and VS 2019:

```powershell
./build.ps1 dev -Run -Demo
./build.ps1 dev -Run
```

Pass `-QtPrefix C:/Qt/<version>/<kit>` to `build.ps1` for another Qt installation;
`-Generator 'Visual Studio 17 2022'` selects VS 2022. The demo
uses memory only and never connects to the host, plays audio, or changes game files.
Normal launch connects to the host, starting it if needed. Closing the desktop
leaves the host and accepted operations running. The client also accepts `--host`,
`--data-dir`, `--endpoint` (connect only), and `--theme system|dark|light`.

Standard CMake commands work on other platforms (not yet qualified):

```text
cmake -S . -B build/local-desktop -DCMAKE_PREFIX_PATH=<Qt kit>
cmake --build build/local-desktop --config Release
ctest --test-dir build/local-desktop -C Release --output-on-failure
```

Qt tests cover framed protocol fixtures, escaped instruction text, row selection,
regrouping, unavailable history, busy/recovery/disconnected states, and reconnecting
without replaying commands. If the debug Rust host has been built, they also run
Save/Load/Revert and reconnect over real IPC against temporary data with audio off.
Visual test captures are written to `build/<mode>/desktop/screenshots` in both themes.
The desktop reads host state only; it never copies saves or opens the database.

The main window uses the standard system title bar and window frame. It includes
history Restore/Revert, Configure with host validation,
Explore, a revision-bound Flush confirmation, recovery choices, and the shared
Play sounds and Launch on startup settings. Configure exposes detected locations;
Reset uses the selected catalog location (or the sole detected default). The host
registers Ctrl+F5/Ctrl+F9, owns the tray and notifications, and runs without Qt.
While the desktop is focused, Ctrl+F5/Ctrl+F9 invoke the selected game's Save/Load
buttons, including their progress and disabled states, even if no game is running.
Otherwise the host targets the top running game. The host owns the global hotkeys
whenever OS integrations are enabled, including the default development runner, and
forwards them to the focused desktop; the desktop's local shortcuts are only a
fallback when integrations are disabled (as in a demo session);
it does not register competing global shortcuts. `--minimized` attaches without
showing a window; subsequent launches focus the existing desktop for that host.

The **Installed games** section is always present. Use **Scan for known games** to
refresh catalog discovery, or open its **…** menu and choose **Add custom game** to
register a name, executable, and save location. Custom games remain listed when their executable is unavailable
and can be removed with **Forget this game** after confirming the same cleanup
preview used by Flush.

To rebuild and package the desktop and Rust binaries with their runtime DLLs:

```powershell
./build.ps1 release
```

The output is `dist/SaveScummer-windows-x64/bin/SaveScummer.exe` and the portable
`dist/SaveScummer-windows-x64-<version>.zip`, plus the per-user installer
`dist/SaveScummer-windows-x64-<version>-setup.exe` when Inno Setup is available.
Keep the entire extracted folder together.
No Qt installation or PowerShell launcher is needed to run that executable.
Both components must build successfully before packaging; the command does not
fall back to an older host. `scripts/package-windows.ps1` is a lower-level deployment
helper for binaries you have already built, and `scripts/build-installer.ps1`
compiles the installer from a staged payload.
See [docs/building.md](docs/building.md) for the release-publishing process
(draft-first GitHub Releases).

## Run without the UI

Start the host in one terminal:

```powershell
cargo run --bin savescummer-host
```

It uses `%LOCALAPPDATA%\SaveScummer`, discovers the catalog's Steam games, scans every
15 minutes, and monitors configured executable paths. It does not create backups
until requested. Ctrl+C requests a graceful shutdown and waits for accepted work.

The same host downloads Steam game icons on a background worker after discovery.
It checks cached images on startup, after scans, and when the desktop reconnects;
missing or corrupt images are downloaded again. Icons live under
`%LOCALAPPDATA%\SaveScummer\cache` on Windows, `~/Library/Caches/SaveScummer` on
macOS, or `$XDG_CACHE_HOME/SaveScummer` (default `~/.cache/SaveScummer`) on Linux.
Downloads continue with the window closed. Offline games keep cached icons or
initials, with retries in the background. No Steam API key or login is required.
Use host options `--cache-dir <path>` to isolate the cache for testing or
`--no-artwork` to disable artwork for a run. Only icons are fetched; backgrounds
and covers are not used by the current UI.

In another terminal:

```powershell
cargo run --bin savescummer-cli -- state
cargo run --bin savescummer-cli -- save void-war
cargo run --bin savescummer-cli -- history void-war
cargo run --bin savescummer-cli -- load void-war
cargo run --bin savescummer-cli -- watch
cargo run --bin savescummer-cli -- shutdown
```

To add a custom game, supply its absolute save directory and executable. The host
assigns the stable game ID and prints the configured record:

```powershell
cargo run --bin savescummer-cli -- add-custom --name "My game" --dir "D:\Games\MyGame\saves" --exe "D:\Games\MyGame\game.exe"
```

For an isolated development instance, pass `--data-dir .runtime --no-scan` to the
host and `--data-dir .runtime` to each client command. `--no-monitor` disables
process sampling. `--steam-root` adds explicit Steam roots. `--copy-label Copie`
selects French duplicate-folder naming; the initial implementation recognizes
English `Copy` and French `Copie` names with complete numeric suffixes.

The CLI prints each request ID before transmission, then its accepted operation ID.
It waits for a terminal status and returns a nonzero exit code on failure. If a
connection is lost, query `operation <operation-id>` or retry the same command with
`--request-id <original-request-id>`. Disconnecting does not cancel accepted work.

Other commands:

- `load <game> --target <checkpoint-id>` restores the saved checkpoint referenced
  by a Saved/Existing backup row's `snapshot_id`.
- `history <game> --limit 50` returns a page of visible history. Pass its opaque
  `next_cursor` with `--cursor` for the next page, or use `--all` to stream every
  page as JSON lines. If history changes during streaming, restart the query.
- `revert <game> <recovery-checkpoint-id>` restores the recovery checkpoint referenced
  by a Loaded/Reverted row's `recovery_id`.
- `recover <game> <operation-id> keep-current|restore-before|retry` resolves recovery.
- `flush-preview <game>` lists the saved, recovery and retained directories to delete.
- `flush-details <game> --cursor <next_cursor>` reads the next page of paths from
  that preview. Counts always cover the complete deletion scope.
- `flush <game> --confirmed-revision <preview-revision>` confirms that preview.
- `forget <custom-game> --confirmed-revision <preview-revision>` performs the same
  cleanup, then permanently removes the custom game registration.
- `rescan` refreshes discovery immediately.
- `sounds on` / `sounds off` persist the app-wide Play sounds setting (on by default).
- `startup on|off` updates the current-user Windows sign-in registration and persisted
  preference. Registration failure preserves the previous preference; persistence
  failure attempts to restore the previous registration.
  Registration changes only through this command, the Launch on startup checkbox,
  or the installer. On startup the host adopts an installer-created entry into
  the preference; it never takes over an entry that points at another build and
  never recreates an entry the user or a development build removed.
  All builds share
  one `SaveScummer` entry in `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`,
  pointing to the host executable and data directory that enabled it. Keep that
  executable at a stable path. After moving it, run `startup on` from the desired
  copy (with its host running), or toggle the checkbox off and on, to update the entry.
- `save-active` / `load-active` target the monitor's top running game. Retrying an
  accepted request ID retains its original game even after focus changes.
- `explorer-targets <absolute-folder>` returns allowed Save/Load actions with exact
  checkpoint IDs. `explore <game>` opens the parent of DIR through the host.
- `reset <game>` selects the sole detected catalog location. For ambiguity, use
  `select-location <game> --dir <detected-dir> --exe <detected-exe>`.

The CLI starts the host when necessary; `--no-start` makes it connect only.
`--host <absolute-exe>` selects a host executable for development or testing;
packaged clients find `SaveScummer.Host` beside themselves automatically.
Use `--no-integrations` on isolated test hosts to disable global shortcuts, tray,
notifications and OS startup changes. `--desktop <absolute-exe>` selects the desktop
launched from the tray; by default it is beside the host. Tray Exit stops admission,
informs connected clients, waits for accepted operations, and releases integrations.

## Explorer integration

For a complete development session, run:

```powershell
./scripts/dev-explorer.ps1            # Build, test, register, restart Explorer, open test folder
./scripts/dev-explorer.ps1 -Stop      # Stop the dev host, unregister, restart Explorer
./scripts/dev-explorer.ps1 -CheckOnly # Build and test without registration or Explorer restart
```

The first command builds an isolated host and a separate development extension,
creates disposable save data, and verifies Save/Load through the actual COM menu
handler before registering it. Right-click `Saves` or `Saves - Copy`, then select
**Show more options** on Windows 11 to use **Save (dev)** or **Load (dev)**.
Edit `Saves/progress.txt` to try restoring it. The script restarts Explorer to load
the new DLL; open Explorer windows close. Rerun the command to rebuild and start
a fresh test session. No administrator rights are needed.

The development extension has its own COM registration and baked-in host data
directory. Your normal host, configuration and production extension are separate.
Test data and logs are retained under `.runtime/explorer-dev/sessions`, binaries
under `build/explorer-dev`; the script prints the current session path.
`-Stop` removes only the dev registration and gracefully stops its recorded host.
`-CheckOnly` stops its test host automatically and leaves an interactive dev session
alone. Use `-Generator 'Visual Studio 17 2022'` when building with VS 2022.

The separate native COM extension has no Qt dependency. Its Rust IPC bridge asks
the host for menu eligibility; Save appears on DIR, and Load on eligible ordinary
saved copies. Recovery and unrelated directories have no actions. The menu retains
the checkpoint ID so replacing a backup while the menu is open cannot retarget it.
The default per-user host must be running; unavailable or slow host queries fail
closed, with a 750 ms timeout. Windows 11 exposes this classic extension under
**Show more options**.

```powershell
./scripts/build-explorer.ps1
./scripts/register-explorer.ps1 -Dll ./build/explorer/Release/savescummer-explorer.dll
# Remove only this user's extension registration:
./scripts/register-explorer.ps1 -Uninstall
```

Build/test does not install the extension or enable startup. Keep the registered
DLL in a stable location. New Explorer processes load registration changes.

Released builds carry the same extension. The per-user installer registers it by
default (with a checkbox to opt out) and removes the registration on uninstall.
The portable package ships `bin\savescummer-explorer.dll` and two helper scripts
beside `SaveScummer.exe`, **Enable Explorer integration.cmd** and **Disable
Explorer integration.cmd**, so the ZIP copy can opt in without the installer.
Explorer must be restarted (or Windows signed out and back in) before it loads a
new or removed extension DLL. Updating or uninstalling a copy whose extension is
registered requests a restart for the same reason: Windows cannot overwrite a
DLL that a running Explorer has loaded.

## Windows discovery

All YAML definitions under `catalog/games` are embedded when the host is built.
`--catalog-dir <directory>` selects an external catalog for development or local
definitions; each rescan rereads it without recursively walking other directories.

Steam app IDs also match exact `Steam App <id>` uninstall keys in current-user and
machine-wide 32/64-bit registry views. Non-Steam catalog definitions can omit
`stores` and declare `registry_keys` (exact uninstall subkey names),
`known_install_dirs` (`{ROOT}/relative/path` templates), and `alternative_data_dirs`
alongside `executables` and `data_dir` in their Windows block. Expected executables
must exist; display names alone never match a game. Native installation identities
deduplicate sources while distinct data locations remain explicit choices.

The service exposes `detected_locations`, `configuration_error` and
`discovery_errors`. Invalid/ambiguous discoveries stay visible with operations
blocked until configuration succeeds. Valid user choices survive rescans and
restart. Confirmed executable removal updates installation status without deleting
history or backups; inaccessible sources/volumes preserve the previous status.

## Implemented behavior

- Separate core, snapshot, SQLite, scanner, monitor, platform and IPC crates, wired
  by the host; no UI dependencies or direct SQLite access in the client.
- SAVE, default/explicit LOAD, repeated REVERT, confirmed Flush, persistent IDs and
  history, per-game busy rejection, and idempotent accepted requests.
- Checkpoints own their original data directory and restore eligibility. Returning
  to a previous directory restores eligibility for its unchanged checkpoints.
  Default Load selects from saved checkpoints directly, using selection time and
  durable registration order for ties; history is optional audit context.
- Staged copies and exclusive renames, pre-operation recovery copies, durable
  replacement intent, automatic rollback when live data is absent, and explicit
  recovery choices when replacement may have completed. Completion and its history
  entry commit together. Partial data and retained originals survive until Flush.
- Per-user Windows named pipes with a current-user DACL and remote clients rejected;
  a data-directory instance lock; versioned JSON framing; query/reconnect/watch.
- Windows known-folder resolution and protected/overlapping data-directory checks;
  Steam registry roots, `libraryfolders.vdf`, app manifests and executable validation.
- Steam installations are deduplicated by native directory identity, preserving
  distinct installations on case-sensitive filesystems. Unavailable libraries or
  other game locations do not block accessible games; recorded path reservations
  still participate in overlap checks.
- Windows process/focus sampling, one active-stack entry per game, and persistent
  observed launch/close markers. Existing processes at monitor startup are not
  assigned invented launch times. Initial ordering uses stable game IDs, then focus.
- The catalog's plain-text game instructions are included in service state.

## External backup changes

Deleting or changing a backup retires its old generation. A recreated native copy
under the same name becomes a new Existing backup with fresh snapshot/history IDs.
The old action never restores the new folder. Repeated scans do not duplicate it.
Manual backups use folder modification time for ordering while `saved_at` stays null.
Existing databases migrate automatically without changing checkpoint/history IDs
or operation journal paths. Legacy manual checkpoints whose original directory was
not retained stay ineligible until verified by discovery at the configured directory.
Flush refreshes backup discovery before accepting the preview revision. Backups
added, removed, modified or replaced since the preview require a new confirmation.

Detection uses filesystem identity and a recursive metadata signature (entry names,
kinds, sizes, identities and modification times). It detects nested changes even
when the parent timestamp is unchanged; it is not a content checksum and cannot
detect an edit that deliberately preserves every inspected attribute. A failed or
incomplete inspection marks the generation temporarily unavailable, not removed.

Clients render per-game **`history_page.rows`**. Durable audit records are queried
separately from library summaries. Removed checkpoint rows are absent from the visible projection; a Loaded
row can remain if its independent recovery point survives. Launch/close markers
appear only for sessions containing surviving backup-backed actions. Sessions with
no remaining checkpoints disappear, and an empty backup history has no orphaned
session markers. Observation epochs prevent unobserved host downtime from joining
unrelated sessions.

SQLite commits update only changed records in one transaction. The host starts
with game configuration and current operation state; it reads old history and
operation journals by indexed queries. Checkpoint/history completion and recovery
transitions retain their atomic transaction boundaries. History pages include
exact action targets and availability, with 50 rows by default, a 200-row maximum,
and a 512 KiB row-data budget. The desktop keeps at most 200 loaded history rows.
Use Load older to browse; reopening history returns to recent entries.

Protocol v4 publishes library summaries, current progress and the most recent
terminal result per game. It does not broadcast accumulated history or journals.
History cursors expire on relevant changes to that game or host restart; unrelated
games and artwork updates do not invalidate them. Flush details are paginated and
remain bound to the confirmation revision. Existing databases migrate automatically
while preserving IDs, action references and recovery journals. When upgrading from
an older build, exit its host before starting the new package; rebuild and register
the matching Explorer extension using the documented scripts.

## Current limits

Windows core workflows and integrations are implemented. Proton resolution and
native macOS/Linux integrations remain outside this Windows milestone. Native copy
naming currently recognizes the documented English/French conventions; other
locales require explicit `--copy-label` configuration. Tray/notification delivery,
shortcut conflicts and focus behavior still need interactive Windows qualification,
including fullscreen games. The Explorer DLL is built and COM-tested separately;
the installer registers it by default and the portable package provides an opt-in
helper.

The Windows host embeds the approved WAV cues and plays Save/Load start and
completion sounds, failure knocks and rate-limited busy ticks independently of
the UI. A separate audio worker sequences fast cues without delaying file work.
Only committed operations play completion; retried accepted requests stay silent.
The desktop Play sounds checkbox and `sounds on|off` CLI command share one persisted
preference. `--no-audio` suppresses audio for an isolated host run without changing
that preference. Native audio on macOS/Linux is not implemented yet.

The monitor polls every 250 ms and can miss a process that starts and exits between
samples. Fullscreen/focus compatibility needs interactive desktop testing. Windows
is the tested platform; Unix transport and file-operation code is not yet qualified.
Automated tests use isolated hosts with OS integrations disabled; they never alter
the user's sign-in registration or install an Explorer extension.

Copies are best-effort while a game is running. Symlinks, junctions/reparse points
and special files inside save directories are rejected. The file copier preserves
contents and empty directories, not arbitrary ACLs, alternate streams or all file
metadata. Directory identity/change checks do not make file/database operations
atomic against arbitrary simultaneous external filesystem edits or power loss.
Progress reports phases and cumulative bytes, without a precomputed percentage.
State watch coalesces revisions into complete library summaries and has an 8 MiB
frame cap. Oversized individual items return an explicit error. Scanning actual
backup folders and collecting Flush paths still scale with that game's retained
files. See [history capacity measurements](docs/history-capacity.md) for the
metadata benchmark, its fixture scope and reproduction command.

See [PLAN.md](PLAN.md), [PLAN-INTEGRATION-TESTS.md](PLAN-INTEGRATION-TESTS.md) and
[protocol/README.md](protocol/README.md) for the target behavior and service contract.
