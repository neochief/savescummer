# SaveScummer failure and interruption catalog

Status: draft for review. This is the complete list of failure, interruption and notice
situations for the application, how each is detected, what the user sees, and how it is
tested.

- Host behavior, including how and what to test: [`PLAN-HOST.md`](PLAN-HOST.md).
- Main-window presentation and mockups: [`PLAN-UI.md`](PLAN-UI.md).

Terms used below:

- **Save set:** everything a game's checkpoint covers; one or more targets.
- **Target:** a root folder plus a filter (what inside the root counts) and optional
  excludes. A plain folder, a single file and a glob are all targets.
- **Checkpoint store:** the central folder holding every checkpoint, one subfolder per
  target. Per-user app data by default; the user can move it to another drive.
- **Load** makes each target identical to the checkpoint, deleting matched files the
  checkpoint doesn't have. Revert works the same way.

---

## 1. Principles

- **Deterministic recovery.** An interrupted operation is resolved by fixed rules, never
  by a user prompt. There is no manual recovery command; Retry only re-runs the same
  rules.
- **Change nothing you cannot verify.** When the state is uncertain, keep the current
  live data, retain every recovery path, and surface an explanation.
- **Never delete recovery material before the interruption is resolved.** `.ssold`
  files, the recovery checkpoint and unpublished Save folders survive until the rules
  below or the ordinary Flush rules permit removal.
- **Reversible until the last stage.** A Load swaps files by renames in the same folder,
  so every failure before the final cleanup is undone by renaming back. The originals
  are back only if that undo succeeds; an undo that fails has its own recovery (R4,
  E-B1). The recovery checkpoint is for Revert, not for rollback.
- **Wait out brief interference, never retry the whole operation.** A file action that
  fails with a transient sharing or lock violation is retried within one budget of
  about a second per operation; rollback has its own budget (PLAN-HOST, *Interference
  from the running game*). Other errors fail at once.
- **Per-game scope.** A blocked or failed game does not block other games. Sidebar rows
  remain selectable; only the affected game's operation controls are disabled. The one
  exception is a missing checkpoint store, which affects every game (E-C14).
- **One UI treatment.** Every error uses the same sticky block in the main window:
  what happened, the likeliest reason and suggested action, a Retry button where retrying
  can help, buttons that open the exact affected folders, and Details with raw paths and
  the technical error. No generic modal dialogs.
- **Open the exact folder.** Folder buttons launch the real path through the host:
  the affected target's root, the game's checkpoints folder, a specific checkpoint, or
  the recovery checkpoint. The user never has to type a path. A missing folder opens
  its nearest existing parent, so a button never fails just because a folder is gone.
- **Retry sends the same command again,** with a new request ID, after an ordinary
  failure. Only a blocked game has its own retry command.
- **Automatic retry.** Blocked recoveries retry at each host start and when the user
  asks (Retry).

## 2. Interrupted-operation recovery rules

Why rules at all: the host can die or lose power at any moment, and the next start must
land every game in a coherent state without asking the user.

**Save** copies each target's matched files into a temporary folder in the checkpoint
store, publishes it under the checkpoint's name when complete, then registers it. A
half copy never becomes a checkpoint.

**Load and Revert** first make the recovery checkpoint (an ordinary Save), then apply
the checkpoint in four stages, each finished for all files before the next starts:

1. copy each checkpoint file next to its live counterpart as `name.ssnew`;
2. rename each live file in scope to `name.ssold`, including files the Load will
   delete;
3. rename each `.ssnew` to the real name;
4. delete the `.ssold` files, then commit the history entry.

The journal records which operation ran and which files it covered; the leftover
`.ssnew`/`.ssold` files show how far it got. The host applies these rules at startup, in
this order:

| Rule | Condition | Result |
|---|---|---|
| R1 | Live files untouched: a Save before publishing, or a Load/Revert before stage 2 (only `.ssnew` files exist) | Delete the temporary folder or the `.ssnew` files. Clear the operation as failed. An interrupted Save shows the one-line notice (E-A1); an interrupted Load or Revert is silent (E-A2). |
| R2 | Stage 2 or 3 incomplete: some covered file still exists only as `.ssnew`, and every `.ssold` is intact | Undo: rename restored files back to `.ssnew`, rename every `.ssold` back to its real name, delete the `.ssnew` files. The operation never took effect. Log only. |
| R3 | Result in place but not committed: a Save published but not registered, or a Load/Revert with every covered file swapped in (no `.ssnew` left) | Finish: register the Save, or delete the remaining `.ssold` files and commit the Load/Revert history entry exactly as if it had completed. |
| R4 | Anything else: files that don't match the journal, a real name occupied by a file the game created mid-Load, a file changed since the journal, a target's drive unavailable | Keep every file exactly as it is, including `.ssnew`/`.ssold`. Mark the operation failed with no history entry. Retain the recovery checkpoint. Release the game when every covered name has a live file; otherwise keep the game blocked and show the sticky error (E-B1). Retry automatically at each host start. |

Never blindly roll back over current data, never retry the requested operation
automatically (retrying a single failed file action within the budget is not a retry of
the operation), and never delete retained material to "clean up" an uncertain outcome.

## 3. Error block

Placed directly below the main action row, above the instructions block. It sticks until
the next action or game selection and never becomes a modal dialog.

```text
⚠ Couldn't replace the saves
The game has a save file open. Close the game, then try again.

[ Retry ]   Open save location  ·  Open recovery checkpoint        Details ▸
```

- Line 1: what happened. Line 2: likeliest reason and suggested action.
- **Retry** appears only when retrying can plausibly help.
- Folder buttons, as applicable to the situation:
  - `Open save location` (the affected target's root; the first affected one when
    several are);
  - `Open checkpoints folder` (this game's folder in the checkpoint store, also for
    errors about the whole store, where it falls back to the nearest existing folder);
  - `Open checkpoint`;
  - `Open recovery checkpoint`.
- **Details** expands the exact paths and the raw host error.
- Two transient forms exist: a success check on the initiating control, and a one-line
  sticky notice for an interrupted save that produced no checkpoint (E-A1).

## 4. Scenario catalog

### 4.1 Auto-healed — no UI

| ID | Situation | Behavior |
|---|---|---|
| E-A1 | Host killed or power lost during a Save copy | Live files untouched. The temporary folder in the store is deleted (R1). One-line sticky notice: "The last save was interrupted and wasn't completed." |
| E-A2 | Host killed or power lost during a Load or Revert before every file was swapped in (recovery checkpoint, stage 1, 2 or partway 3) | Only `.ssnew` files: deleted (R1). `.ssold` files present: renamed back (R2). Live saves end up as before the Load. Silent. |
| E-A3 | Killed after the result was in place (a Save published, or every Load/Revert file swapped in), before commit | Finished automatically (R3). No notice. |
| E-A4 | Disposal folder left behind by a failed checkpoint delete | Reserved name, never mistaken for a checkpoint; removed on the next scan or host start. |
| E-A5 | Checkpoint deleted or replaced externally | Generation retired; affected rows refresh or disappear silently. |
| E-A6 | Stage 4 can't delete some `.ssold` files within its short retry allowance (antivirus or the game holds one) | The Load stays applied and is committed. Leftover `.ssold` files are deleted on the next scan or host start. They carry a reserved suffix, so no filter, checkpoint or Load delete ever picks them up meanwhile. |

### 4.2 Clean failures — nothing on disk changed

Each of these fails only after the retry budget ran out, for errors that are retried at
all. A failure that needs an undo leaves the originals in place only when the undo
succeeds; otherwise E-B1. Details always carry the original error, its OS code and the
path.

| ID | Situation | Message (what happened / likeliest reason) | Buttons |
|---|---|---|---|
| E-C1 | Load/Revert stage 2: renaming a live file to `.ssold` fails. On Windows, typically a handle opened without delete sharing (the game or antivirus) or access denied; a handle that allows delete sharing doesn't cause it. Already-renamed files go back, `.ssnew` files are deleted. Logs and crash dumps are never in a target, so a log the game keeps open never causes this. | "Couldn't replace the saves. A save file is in use or access to it was denied, possibly by the game or antivirus. If the game is running, close it, load again, then start the game." | Retry · Open save location |
| E-C2 | File locked or unreadable while copying (a Save, the recovery checkpoint, or Load stage 1) | "Couldn't read part of the save. The game or antivirus may be using it." | Retry · Open save location |
| E-C16 | macOS/Linux, before Load/Revert stage 2: the open-file check found a game process holding a file the Load would replace or delete, and it stayed open for the whole wait. `.ssnew` files are deleted; nothing live changed. Details list the files and processes. | "The game has a save file open. Close the game, load again, then start the game." | Retry · Open save location |
| E-C3 | Checkpoint store's drive full (a Save or the recovery checkpoint) | "Not enough free space to create the checkpoint." | Retry · Open checkpoints folder |
| E-C4 | Permission denied writing to the store, or creating `.ssnew` files in a target | "Windows blocked writing to the checkpoints folder." / "Windows blocked writing to the save location." ("macOS blocked…" on a Mac, "The system blocked…" on Linux) | Retry · Open checkpoints folder / Open save location |
| E-C5 | A target's drive or share is unavailable, so its presence is unknown (E-N3) | "The drive with part of this game's saves isn't connected." | Retry · Open save location |
| E-C6 | Link, junction or special file inside a target | "The save location contains a link SaveScummer can't copy." | Open save location |
| E-C7 | Checkpoint changed or removed since it was shown | "This checkpoint was changed or removed outside SaveScummer." | Open checkpoints folder |
| E-C8 | Checkpoint belongs to a different save set (override, reset, catalog change, other Steam account) | "This checkpoint was made for a different set of save locations." | Open checkpoint · Open save location · Configure paths… |
| E-C9 | A target fails the safety recheck before an operation (became broad, overlapping or unresolvable since it was configured) | "The configured save location isn't valid or overlaps another game's." | Open save location · Configure paths… |
| E-C11 | Host is shutting down | "SaveScummer is shutting down." | Retry after restart |
| E-C12 | Delete target changed or points at live data | "This checkpoint no longer matches what SaveScummer recorded." | Open checkpoints folder |
| E-C13 | Load stage 1: a target's drive has no room for the `.ssnew` copies (the restored files need a second copy next to the live ones). `.ssnew` files are deleted. | "Not enough free space next to the save to load this checkpoint." | Retry · Open save location |
| E-C14 | Checkpoint store unavailable: it sits on, or was moved to, a drive that isn't connected (on macOS and Linux also when an empty folder is left where the drive was mounted). Save, Load and Revert are refused for every game; history rows are marked unavailable, not retired. | "The drive with your checkpoints isn't connected." | Retry · Open checkpoints folder |
| E-C15 | Load/Revert stage 3: renaming a `.ssnew` to the real name fails partway (the game created that name between stages, or antivirus holds the `.ssnew`). Swapped-in files go back to `.ssnew`, then stage 2 is undone. | "Couldn't put the restored saves in place. The game or antivirus may be using the save location." | Retry · Open save location |
| E-C17 | macOS: the game's save location (or install folder, or the store) is somewhere macOS guards, and the user hasn't allowed access yet (`access_needed`, with the category). Refused before anything is read; the game is listed but inactive. A hotkey in front of it plays the failure cue and repeats the notification. | "SaveScummer needs your permission to read <Documents / other apps' data / …>." When the user denied it: "macOS denied access. Allow SaveScummer in System Settings." | Allow access / Open System Settings |
| E-C18 | Save/Load file work made no progress for 30 s (`stalled`): most likely a read waiting on a macOS permission prompt nobody sees. Reported failed at once; the game stays locked until the stuck work ends, and its real outcome is recorded then. | "SaveScummer is waiting for the system to answer. macOS may be showing a permission prompt." | — (the game unlocks by itself) |

### 4.3 Blocked / uncertain — material retained

| ID | Situation | Message (what happened / likeliest reason) | Buttons |
|---|---|---|---|
| E-B1 | Rollback could not rename files back, even within its own retry budget (R4, or an E-C1/E-C15 undo that itself failed) | "The previous load was interrupted. The original save couldn't be put back automatically — the game may be holding it, or the drive is unavailable." | Retry · Open save location · Open recovery checkpoint |
| E-B2 | Load applied but the metadata commit failed | "The load was applied, but SaveScummer couldn't record it. The database may be full or busy." | Retry · Open save location |
| E-B3 | Delete could not finish | "Couldn't finish deleting this checkpoint. It will be cleaned up automatically." | Retry · Open checkpoint |
| E-B4 | Database unreadable or unwritable on startup | "SaveScummer couldn't open its database." | Restart app |

A blocked game keeps Save/Load and its history actions disabled. Other games are
unaffected. R4 retries automatically at each host start.

### 4.4 Deletion

| ID | Situation | Behavior |
|---|---|---|
| E-D1 | Checkpoint folder locked (rename to disposal name fails) | Nothing is touched. E-C1-style error; retry later. |
| E-D2 | Deletion fails partway (after the rename) | Contents are removed on the next scan or host start; the record is retained until deletion succeeds. E-B3 if it persists. |
| E-D3 | Store's drive removed during deletion | Records for undeleted data are retained; retry when the drive returns. |
| E-D4 | Checkpoints changed after the Flush preview was shown | Not an error. The preview isn't binding: on confirm the host finds every checkpoint again and deletes what exists then. |

Per-checkpoint deletion and Flush both delete through a reserved disposal name first, so
a locked folder is never partially deleted under its real name.

### 4.5 Links, locations and configuration

| ID | Situation | Behavior |
|---|---|---|
| E-L1 | A target root is a symbolic link or junction | Resolved to the real directory at Configure time and rechecked before every operation. All operations run against the real directory; the link is never renamed, replaced or copied. Not an error. |
| E-L2 | Link repointed, broken or no longer resolving | Validation fails: "This save location is a shortcut that changed or can't be resolved. Configure the real folder." Buttons: Configure paths… · Open save location. |
| E-L3 | Link inside a target | Copy/fingerprint rejects it; E-C6. |
| E-L4 | Checkpoint folder replaced by a link | Treated as changed externally (E-A5/E-C7); restoring from it is refused. |
| E-N1 | No target exists yet, or none matches anything | Save disabled with `No game data yet`; the status stays `Running`/`Stopped` and the game is provisional. Open save location (may not exist). |
| E-N2 | A target root that held data at Save time is missing at Load | Load of that checkpoint is refused: restoring only the other targets would be half a save, and roots are never recreated. Known game: the catalog resolves the save set again; if the set changes, old checkpoints are unavailable until it returns (E-C8). Override or custom game: unavailable until the folder returns or is reconfigured. |
| E-N3 | A target root on an unplugged drive or unreachable share, including a drive the host has seen whose mount point is now gone or an empty folder (macOS, Linux) | Presence is unknown, not missing: the target is not treated as absent and no generation is retired. Save, Load and Revert are refused with E-C5, because a checkpoint recorded without that target would later leave it alone as "absent". Retry when accessible. |
| E-N4 | Game uninstalled | Not shown in the sidebar (known and custom alike). History and checkpoints are retained. |
| E-N5 | UI disconnected from the host | Reconnecting state; no operation errors are produced. |
| E-N6 | Host/UI version mismatch | Explicit "upgrade required" message; no operation starts. |
| E-N7 | A target absent at Save time exists at Load | Left alone. Why: a folder that appears later is usually a setup change (Steam Cloud switched on, Proton, a new save path), and deleting inside Steam's `remote` can delete from the cloud. Not an error. |
| E-N8 | A target root existed at Save time but its filter matched nothing | A normal state, and it is restored: the Load deletes whatever the filter matches now. Those files are in the recovery checkpoint. |
| E-N9 | Checkpoint store on a different drive than a target | Not an error. Copies cross drives; the Load renames still happen inside the target's own folder, so they stay same-drive and atomic. Free space on the target's drive matters for stage 1 (E-C13). |
| E-N10 | Steam account switched between sessions | The account is resolved again at game start, so the save set follows the account now playing. Checkpoints made for another account belong to that account's set and are unavailable until it plays again (E-C8); the Load hotkey picks the newest checkpoint of the current set. |
| E-N11 | User moves the checkpoint store in settings | Every game is busy during the move: the host copies all checkpoints to the new location, verifies them, switches, then deletes the old copies. A failure or crash before the switch leaves the old store in use and nothing changed; the partial copy at the new location is cleaned up. If the new location later goes missing, E-C14. |

Configuration validation (Add custom game, Configure paths) shows errors below the
field's hint, not in the error block; nothing is saved until the location passes.

| ID | Situation | Behavior |
|---|---|---|
| E-V1 | Relative path | Rejected: "Use a full path." |
| E-V2 | Broad folder as a whole, or a wildcard directly in one (the game's install folder, drive root, home, AppData root, Documents, Saved Games, Program Files, a Steam library root, `~/Library/Application Support`, and similar) | Rejected, because a Load would touch unrelated files. An exact name inside a broad folder is allowed (`Documents\mygame.sav`), since Load touches only that name. |
| E-V3 | Known bad pattern: a filter that would match the game's executable | Rejected. |
| E-V4 | Overlap with another game: roots equal, nested, or containing, unless both use distinct exact names | Rejected, naming the other game. |
| E-V5 | Filter names a reserved suffix (`*.ssnew`, `save.ssold`) | Rejected. |
| E-V6 | Filter like `save*` that would also match `save.ssnew`/`save.ssold` | Allowed. Reserved suffixes are excluded from every filter, recovery checkpoint and Load delete, so the pattern never sees them. |
| E-V7 | Path doesn't exist yet | Allowed; validation never creates anything. The game shows `No game data yet` (E-N1). |

The catalog builder applies the broad-folder rule (E-V2) to catalog targets too, so a
known game never ships a target that validation would reject.

### 4.6 Load removing newer saves

| ID | Situation | Behavior |
|---|---|---|
| E-M1 | Load removes saves newer than the checkpoint | Intended: a game that continues from its newest file would otherwise silently not restore. Every removed file is in the recovery checkpoint by construction. The Loaded history row says so: "removed 2 newer saves, kept in the recovery point". |
| E-M2 | Revert after a Load that brought back a deleted save | Revert makes the saves exactly as before the Load, so the brought-back save is removed again. |
| E-M3 | Two campaigns played in turn; loading an old checkpoint of one rolls the other back | Accepted: Load always restores the whole checkpoint. The newer state is in the recovery checkpoint; Revert brings it back. |

### 4.7 Steam Cloud

Steam compares each synced file with its own record; a restored file looks "changed
locally". SaveScummer never touches Steam's bookkeeping (`steam_autocloud.vdf`,
`remotecache.vdf` are excluded from every target), so the outcome is Steam's.

| ID | Situation | Behavior |
|---|---|---|
| E-S1 | Steam Cloud replaces a restored file, or re-downloads one the Load deleted | Not preventable. At the next game launch after a Load, the host compares the restored files with the checkpoint; if Steam changed them, the Loaded row says "Steam Cloud replaced the restored save". No error block. |
| E-S2 | Steam shows its conflict dialog after a Load (the cloud has newer progress from another device) | Steam's dialog; the user decides. Keeping the cloud version surfaces as E-S1 at that launch. |
| E-S3 | Cloud unchanged since the last sync | Steam uploads the restored file. Wanted; nothing shown. |

These behaviors, especially a Load deleting a file inside `remote`, are unverified; test on a
real machine before relying on them (5.2).

### 4.8 Not detectable by design

| ID | Situation | Notes |
|---|---|---|
| E-X1 | The game writes files during a copy | Copying is best-effort; the checkpoint may be internally inconsistent. No hint is shown during normal saving; the caveat lives in the docs. |
| E-X2 | Content-level corruption with unchanged metadata | The change signature covers paths, kinds, sizes, identities and modification times; it is not a content checksum and cannot detect attribute-preserving edits. |
| E-X3 | Load while the game is running, and the game later writes its in-memory state over the restored files | Load works while the game sits at the main menu; refusing just because the game runs is not an option. Only a Steam Cloud replacement is detected (E-S1). |
| E-X5 | The open-file check can't see everything, or the game opens a file after it | The check is best-effort and depends on permissions (other users, protected processes, `hidepid`, sandboxes). Unavailable or incomplete inspection proceeds as before and is logged, never reported as "no files open". A file opened after the check isn't noticed. The per-game instructions stay necessary. |
| E-X4 | The game looks at its folder during a Load | Accepted costs: `.ssnew` files are briefly visible during stage 1, and names are briefly missing between stages 2 and 3 (milliseconds). |

## 5. Testing

### 5.1 Levels and tooling

- **Unit (core):** `crates/core` runtime tests with fault-injected IO/repository/clock
  ports. Covers R1–R4 decisions, retry rules, delete semantics, record retention,
  presence (present / missing / unknown) and save-set membership of checkpoints.
- **Unit (snapshots):** `crates/snapshots` tests against real temporary directories.
  Covers target filters and excludes, reserved suffixes, the four Load stages and their
  rollback, deleting newer matched files, disposal-delete and reparse rejection.
- **Integration (Rust, Windows):** `tests/integration` with the `tests/fake-game`
  fixture and temp data. Covers real OS faults and durable phases.
- **Qt (fake service):** `apps/ui/tests/ui_test.cpp`. Covers error block
  content, button wiring, sticky behavior, countdown, instructions block, the Loaded row
  notes and configuration validation messages.
- **Manual (Windows compatibility):** antivirus interference, fullscreen focus,
  removable drives.
- **Manual (Steam Cloud, real machine):** Isaac (cloud API), Slay the Spire
  (Auto-Cloud), Risk of Rain Returns (both).

### 5.2 Simulation recipes

| Fault | How to reproduce |
|---|---|
| Locked file/folder | Hold an exclusive handle from the test process or the fake game. |
| Game holds a save open | Windows: fake game keeps a save file open without delete sharing; stage 2 rename fails. macOS/Linux: fake game keeps it open (a plain open); the open-file check finds it. |
| Transient hold | Fake game holds the file for a set time shorter than the budget, then closes it. |
| Persistent hold | Fake game holds the file until told to stop. |
| Several holds | Fake game holds two affected files, each for most of the budget. |
| Incomplete inspection | Fake inspector reporting an uninspectable process (core); on Linux, a held file in a process of another user where the test machine allows it. |
| Retries during rollback | Stage 3 fails, then an undo rename hits a transient hold (Windows) or an injected transient error (core). |
| Permission denied | ACL deny on a temporary directory (not the read-only attribute). |
| Disk full | Small VHD or injected IO failure where mechanical reproduction is not worth it. |
| Kill at a durable phase | Existing `crash_worker` pattern: terminate the host between persisted phases and between the four Load stages, including partway through a stage. |
| Leftover `.ssnew`/`.ssold` | Fixture folders with each combination the rules name, plus inconsistent ones for R4. |
| Name created mid-Load | Fake game creates a file at a real name between stages 2 and 3. |
| Junction | `mklink /J` (no admin required). |
| Symlink | Windows: developer mode or `CreateSymbolicLinkW`; Linux/macOS: `ln -s`. |
| Drive unavailable | Detach a VHD or use an unreachable UNC path, for a target root or the store. |
| Store on another drive | Store on a VHD, targets on the system drive. |
| Account switch | Two fixture Steam accounts; change the active one between game starts. |
| Version mismatch | Run a UI built for a different protocol revision against the host. |
| Steam Cloud | Real machine: Load with the game closed, then launch; a Load that deletes a file; a Load while the cloud has newer progress from another device. |

### 5.3 Coverage matrix

| Scenario | Level | Test target | Simulation |
|---|---|---|---|
| E-A1–E-A3 | core unit + integration | R1–R4 rules; kill between and within stages | crash_worker, leftover fixtures |
| E-A4 | integration | disposal cleanup on scan/start | rename-then-fail-delete |
| E-A5 | integration | generation retirement on external change | edit/delete checkpoint |
| E-A6 | integration | stage 4 gives up quickly, leftover `.ssold` cleaned, Load stays committed | lock an `.ssold` |
| E-C1 | integration (Windows) | stage 2 fails after the retries, original error kept, rollback | persistent hold |
| E-C16 | integration (macOS, Linux) | refused before stage 2, no `.ssold` created, `.ssnew` deleted | persistent hold |
| E-C17 | integration (every OS, guarded table and answers from the test environment) | inactive in the background without asking, one notification per category, Allow access granted and denied, adding a game asks, a new build forgets, a hotkey in front fails | fixture folder marked guarded |
| E-C18 | integration | reported failed while the lock is held, real outcome recorded after | test delay at the copy |
| Retries | core unit + integration | transient hold clears and the operation succeeds; several held files share one budget; only the failed action is retried; non-retryable errors fail at once; rollback succeeds on its own budget after the forward one ran out; stage 4 stays short | transient, several, rollback holds; fake clock |
| E-X5 | core unit | unavailable or incomplete inspection proceeds and logs; a positive find still waits and refuses | fake inspector |
| Wait isolation | integration | other games operate and requests are answered while one game waits | held file + second game |
| E-C2 | integration | locked source file during copy | exclusive handle |
| E-C3 / E-C13 | core unit | store full; target drive full in stage 1 | injected IO error |
| E-C4 | integration | ACL deny on store and on a target | ACL |
| E-C5 / E-N3 | manual + integration | unknown presence refuses operations; a remembered drive detached, gone or left as an empty folder | VHD detach / bad UNC / disk image detach (macOS) |
| E-C6 / E-L3 | snapshots unit + integration | reparse rejection | mklink /J, ln -s |
| E-C7 | integration | changed checkpoint row | edit checkpoint |
| E-C8 / E-N10 | core unit + Qt | checkpoint of another save set | fixture state, account switch |
| E-C9 | core unit + Qt | safety recheck before an operation | fixture state |
| E-C14 / E-N11 | integration + Qt | store unavailable for all games | detached VHD |
| E-C15 | integration | stage 3 rollback | name created mid-Load |
| E-D4 | integration | Flush deletes what exists at confirm time | mutate between preview/confirm |
| E-C11 | host test | shutdown admission | host shutdown |
| E-C12 | core unit | unsafe delete refusal | changed alias fixture |
| E-B1 | core unit + integration | failed rollback retained state after its budget | lock during undo |
| E-B2 | core unit | commit failure after swap | injected storage error |
| E-B3 / E-D1–E-D3 | integration | delete failures and disposal retry | locks, detached drive |
| E-B4 | host test | unreadable database | corrupt/locked DB |
| E-L1 | integration | symlinked/junction target root | mklink /J, ln -s |
| E-L2 | integration | repointed/broken link | recreate link |
| E-L4 | integration | checkpoint replaced by link | mklink /J over checkpoint |
| E-N1 | core unit + Qt | `No game data yet` for missing and empty targets | fixture state |
| E-N2 | core unit | Load refused when a root with data is missing | fixture state |
| E-N4 | Qt | hidden uninstalled games | fixture state |
| E-N5 | Qt | disconnected state | fake service |
| E-N6 | integration | protocol version mismatch | mismatched fixture |
| E-N7 / E-N8 | snapshots unit | absent target left alone; empty-match target emptied again | fixture state |
| E-N9 | integration | cross-drive copies, same-folder renames | store on another drive |
| E-V1–E-V7 | core unit + Qt | validation rules and messages | fixture paths |
| E-M1–E-M2 | snapshots unit + Qt | newer saves removed, Revert exact; Loaded row text | fixture state |
| E-S1–E-S3 | manual | Steam Cloud outcomes and the Loaded row note | real machine |
| E-X1 | manual | checkpoint taken while game writes | real game |
| E-X2–E-X4 | out of scope | documented limitation | — |

## 6. Recorded decisions

- Interrupted save copy shows the one-line sticky notice (E-A1); a killed Load or Revert
  that ends with the live saves unchanged stays silent.
- No hint is shown when saving while the game is running (E-X1).
- Per-checkpoint delete uses a 5-second inline countdown with Cancel.
- Recovery is fully automatic (R1–R4); the recovery prompt and the `Recovery needed`
  status are removed from the UI. There is no manual `recover` command.
- Load rollback reverses renames and never needs the recovery checkpoint.
- Transient sharing and lock failures are retried per action within about a second per
  operation, with a separate budget for rollback and a short one for stage 4. No user
  setting. A retryable "access denied" is not treated as proof that the game holds the
  file.
- On macOS and Linux, a Load or Revert checks the game's processes for open handles on
  the files stage 2 will change, waits within the same budget, and refuses before stage
  2 if one stays open (E-C16). Unavailable or incomplete inspection keeps today's
  behavior and is logged. No filename heuristics and no Loaded-row note for this.
- When a game must be closed, the advice is close → Load → launch, never a blanket
  "restart after restoring": some games save on exit.
- `.ssnew` and `.ssold` are reserved suffixes everywhere.
- Unknown presence (unplugged drive) refuses operations instead of treating the target
  as absent (E-N3).
- A missing target root is never recreated; a checkpoint that needs it can't be loaded
  until it returns (E-N2).
- Steam Cloud outcomes are reported after the fact in the Loaded row, never prevented.
- Links and junctions resolve to the real directory; operations never consume the link.
- Custom games are kept forever; there is no `Forget this game` in this version.
- The Flush preview isn't binding; a checkpoint changing after the preview is not an
  error (E-D4).
- A label edit for a save that has since disappeared is dropped quietly, with no error.
