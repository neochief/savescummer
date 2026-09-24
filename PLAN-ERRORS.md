# SaveScummer failure and interruption catalog

Status: draft for review. This is the complete list of failure, interruption and notice
situations for the application, how each is detected, what the user sees, and how it is
tested.

- Host behavior, including how and what to test: [`PLAN-HOST.md`](PLAN-HOST.md).
- Main-window presentation and mockups: [`PLAN-UI.md`](PLAN-UI.md).

---

## 1. Principles

- **Deterministic recovery.** An interrupted operation is resolved by fixed rules, never
  by a user prompt. There is no manual recovery command; Retry only re-runs the same
  rules.
- **Change nothing you cannot verify.** When the state is uncertain, keep the current
  live data, retain every recovery path, and surface an explanation.
- **Never delete recovery material before the interruption is resolved.** Retained
  snapshots and staging directories survive until the ordinary Flush rules permit removal.
- **Per-game scope.** A blocked or failed game does not block other games. Sidebar rows
  remain selectable; only the affected game's operation controls are disabled.
- **One UI treatment.** Every error uses the same sticky block in the main window:
  what happened, the likeliest reason and suggested action, a Retry button where retrying
  can help, buttons that open the exact affected folders, and Details with raw paths and
  the technical error. No generic modal dialogs.
- **Open the exact folder.** Folder buttons launch the real path through the host:
  save folder, backups parent, a specific backup, or the recovery/retained folder. The
  user never has to type a path.
- **Automatic retry.** Blocked recoveries retry at each host start and when the user
  asks (Retry).

## 2. Interrupted-operation recovery rules

A restore replaces the live save directory in five steps:

1. copy the current directory to a pre-operation recovery snapshot;
2. stage the chosen checkpoint;
3. rename the current directory aside;
4. rename the staged copy into place;
5. commit the history entry and metadata.

An interruption can land in any of these windows. The host applies these rules at
startup, in this order:

| Rule | Condition | Result |
|---|---|---|
| R1 | Live directory untouched (steps 1–2, or the move never happened) | Clear the operation as failed. An interrupted save shows the one-line notice (E-A1); an interrupted restore is silent (E-A2). |
| R2 | Live directory missing and the retained original is verifiably intact (step 3) | Rename the original back. The operation never took effect. Log only. |
| R3 | Replacement verifiably installed but not committed (steps 4–5) | Finish the operation: commit the Save/Load/Revert history entry exactly as if it had completed. |
| R4 | Anything else: identity mismatch, live data present and unverifiable, retained data inaccessible | Keep the current live data exactly as it is. Mark the operation failed with no history entry. Retain all recovery material. Release the game when a coherent live directory exists; otherwise keep the game blocked and show the sticky error (E-B1). Retry automatically at each host start. |

Never blindly roll back over current data, never retry the requested operation
automatically, and never delete retained material to "clean up" an uncertain outcome.

## 3. Error block

Placed directly below the main action row, above the instructions block. It sticks until
the next action or game selection and never becomes a modal dialog.

```text
⚠ Couldn't replace the save folder
The game may still be using it. Close the game, then try again.

[ Retry ]   Open save folder  ·  Open recovery folder        Details ▸
```

- Line 1: what happened. Line 2: likeliest reason and suggested action.
- **Retry** appears only when retrying can plausibly help.
- Folder buttons: `Open save folder`, `Open backups folder`, `Open backup`,
  `Open recovery folder`, as applicable to the situation.
- **Details** expands the exact paths and the raw host error.
- Two transient forms exist: a success check on the initiating control, and a one-line
  sticky notice for an interrupted save that produced no checkpoint (E-A1).

## 4. Scenario catalog

### 4.1 Auto-healed — no UI

| ID | Situation | Behavior |
|---|---|---|
| E-A1 | Host killed during a save copy or staging | Live directory untouched. Cleared as failed (R1). One-line sticky notice: "The last save was interrupted and wasn't completed." |
| E-A2 | Host killed during a restore or revert before the move | Live directory unchanged. Cleared as failed (R1). Silent. |
| E-A3 | Killed after the result was in place (a save's copy renamed to its native name, or a load/revert replacement installed), before commit | Finished automatically (R3). No notice. |
| E-A4 | Disposal folder left behind by a failed delete | Reserved name, never mistaken for a backup; removed on the next scan or host start. |
| E-A5 | Backup deleted or replaced externally | Generation retired; affected rows refresh or disappear silently. |

### 4.2 Clean failures — nothing on disk changed

| ID | Situation | Message (what happened / likeliest reason) | Buttons |
|---|---|---|---|
| E-C1 | Live save folder locked; the aside-rename fails | "Couldn't replace the save folder. The game may still be running." | Retry · Open save folder |
| E-C2 | File locked or unreadable during copy | "Couldn't read part of the save folder. The game or antivirus may be using it." | Retry · Open save folder |
| E-C3 | Destination volume full | "Not enough free space to create the backup." | Retry · Open backups folder |
| E-C4 | Permission denied on destination | "Windows blocked writing to the backups folder." | Retry · Open backups folder |
| E-C5 | Drive or share unavailable | "The drive with the save folder isn't connected." | Retry · Open save folder |
| E-C6 | Link, junction or special file inside the save folder | "The save folder contains a link SaveScummer can't copy." | Open save folder |
| E-C7 | Backup changed or removed since it was shown | "This backup was changed or removed outside SaveScummer." | Open backups folder |
| E-C8 | Backup belongs to a different save folder | "This backup was made from a different save folder." | Open backup · Open save folder · Configure paths… |
| E-C9 | Save folder invalid or overlapping | "The configured save folder isn't valid or overlaps another game's folder." | Open save folder · Configure paths… |
| E-C11 | Host is shutting down | "SaveScummer is shutting down." | Retry after restart |
| E-C12 | Delete target changed or points at live data | "This backup no longer matches what SaveScummer recorded." | Open backups folder |

### 4.3 Blocked / uncertain — material retained

| ID | Situation | Message (what happened / likeliest reason) | Buttons |
|---|---|---|---|
| E-B1 | Rollback could not put the original back (R4) | "The previous load was interrupted. The original save couldn't be put back automatically — the game may be holding it, or the drive is unavailable." | Retry · Open save folder · Open recovery folder |
| E-B2 | Load applied but the metadata commit failed | "The load was applied, but SaveScummer couldn't record it. The database may be full or busy." | Retry · Open save folder |
| E-B3 | Delete could not finish | "Couldn't finish deleting this backup. It will be cleaned up automatically." | Retry · Open backup |
| E-B4 | Database unreadable or unwritable on startup | "SaveScummer couldn't open its database." | Restart app |

A blocked game keeps Save/Load and its history actions disabled. Other games are
unaffected. R4 retries automatically at each host start.

### 4.4 Deletion

| ID | Situation | Behavior |
|---|---|---|
| E-D1 | Backup folder locked (rename to disposal name fails) | Nothing is touched. E-C1-style error; retry later. |
| E-D2 | Deletion fails partway (after the rename) | Contents are removed on the next scan or host start; the record is retained until deletion succeeds. E-B3 if it persists. |
| E-D3 | Drive removed during deletion | Records for undeleted data are retained; retry when the drive returns. |
| E-D4 | Backups changed after the Flush preview was shown | Not an error. The preview isn't binding: on confirm the host finds every checkpoint again and deletes what exists then. |

Per-checkpoint deletion and Flush both delete through a reserved disposal name first, so
a locked folder is never partially deleted under its real name.

### 4.5 Links, junctions and configuration

| ID | Situation | Behavior |
|---|---|---|
| E-L1 | Save folder is a symbolic link or junction | Resolved to the real directory at Configure time. All operations run against the real directory; the link is never renamed, replaced or copied. Not an error. |
| E-L2 | Link repointed, broken or no longer resolving | Validation fails: "This save folder is a shortcut that changed or can't be resolved. Configure the real folder." Buttons: Configure paths… · Open save folder. |
| E-L3 | Link inside the save folder | Copy/fingerprint rejects it; E-C6. |
| E-L4 | Backup folder replaced by a link | Treated as changed externally (E-A5/E-C7); restoring from it is refused. |
| E-N1 | Save directory does not exist yet, or is empty | Save disabled with `No game data yet`; the status stays `Running`/`Stopped`. A missing directory also disables Load and Revert (they never recreate it); an empty one doesn't. Open save folder (may not exist). |
| E-N2 | Save directory moved or deleted | Known game: the catalog picks its DIR again, possibly a new folder; checkpoints from the old folder stay recorded but can't be restored into the new one. Override or custom game: Save, Load and Revert are unavailable until the folder returns or is reconfigured (E-C9). |
| E-N3 | Drive temporarily unavailable | Marked unavailable; the generation is not retired. Retry when accessible. |
| E-N4 | Game uninstalled | Not shown in the sidebar (known and custom alike). History and checkpoints are retained. |
| E-N5 | Desktop disconnected from the host | Reconnecting state; no operation errors are produced. |
| E-N6 | Host/desktop version mismatch | Explicit "upgrade required" message; no operation starts. |

### 4.6 Not detectable by design

| ID | Situation | Notes |
|---|---|---|
| E-X1 | The game writes files during a copy | Copying is best-effort; the backup may be internally inconsistent. No hint is shown during normal saving; the caveat lives in the docs. |
| E-X2 | Content-level corruption with unchanged metadata | The change signature covers paths, kinds, sizes, identities and modification times; it is not a content checksum and cannot detect attribute-preserving edits. |

## 5. Testing

### 5.1 Levels and tooling

- **Unit (core):** `crates/core` runtime tests with fault-injected IO/repository/clock
  ports. Covers R1–R4 decisions, retry rules, delete semantics and record retention.
- **Unit (snapshots):** `crates/snapshots` tests against real temporary directories.
  Covers copy, rename-exclusive, disposal-delete and reparse rejection.
- **Integration (Rust, Windows):** `tests/integration` with the `tests/fake-game`
  fixture and temp data. Covers real OS faults and durable phases.
- **Qt (fake service):** `apps/desktop/tests/desktop_test.cpp`. Covers error block
  content, button wiring, sticky behavior, countdown, instructions block and dialogs.
- **Manual (Windows compatibility):** antivirus interference, fullscreen focus,
  removable drives, Explorer integration.

### 5.2 Simulation recipes

| Fault | How to reproduce |
|---|---|
| Locked file/folder | Hold an exclusive handle from the test process or the fake game. |
| Permission denied | ACL deny on a temporary directory (not the read-only attribute). |
| Disk full | Small VHD or injected IO failure where mechanical reproduction is not worth it. |
| Kill at a durable phase | Existing `crash_worker` pattern: terminate the host between persisted phases. |
| Junction | `mklink /J` (no admin required). |
| Symlink | Windows: developer mode or `CreateSymbolicLinkW`; Linux/macOS: `ln -s`. |
| Drive unavailable | Detach a VHD or use an unreachable UNC path. |
| Version mismatch | Run a desktop built for a different protocol revision against the host. |

### 5.3 Coverage matrix

| Scenario | Level | Test target | Simulation |
|---|---|---|---|
| E-A1–E-A3 | core unit + integration | R1–R4 rules; kill between phases | crash_worker |
| E-A4 | integration | disposal cleanup on scan/start | rename-then-fail-delete |
| E-A5 | integration | generation retirement on external change | edit/delete backup |
| E-C1 | integration | locked live folder | exclusive handle |
| E-C2 | integration | locked source file during copy | exclusive handle |
| E-C3 | core unit | destination full | injected IO error |
| E-C4 | integration | ACL deny on backups parent | ACL |
| E-C5 | manual + integration | disconnected drive | VHD detach / bad UNC |
| E-C6 / E-L3 | snapshots unit + integration | reparse rejection | mklink /J, ln -s |
| E-C7 | integration | changed backup row | edit backup |
| E-C8 | core unit + Qt | wrong-directory checkpoint | fixture state |
| E-C9 / E-N2 | core unit + Qt | invalid/overlapping DIR | fixture state |
| E-D4 | integration | Flush deletes what exists at confirm time | mutate between preview/confirm |
| E-C11 | host test | shutdown admission | host shutdown |
| E-C12 | core unit | unsafe delete refusal | changed alias fixture |
| E-B1 | core unit + integration | failed rollback retained state | locked original |
| E-B2 | core unit | commit failure after replacement | injected storage error |
| E-B3 / E-D1–E-D3 | integration | delete failures and disposal retry | locks, detached drive |
| E-B4 | host test | unreadable database | corrupt/locked DB |
| E-L1 | integration | symlinked/junction DIR | mklink /J, ln -s |
| E-L2 | integration | repointed/broken link | recreate link |
| E-L4 | integration | backup replaced by link | mklink /J over backup |
| E-N1 | core unit + Qt | `No game data yet` for missing and empty DIR | fixture state |
| E-N3 | integration | temporarily unavailable volume | detached VHD |
| E-N4 | Qt | hidden uninstalled games | fixture state |
| E-N5 | Qt | disconnected state | fake service |
| E-N6 | integration | protocol version mismatch | mismatched fixture |
| E-X1 | manual | backup taken while game writes | real game |
| E-X2 | out of scope | documented limitation | — |

## 6. Recorded decisions

- Interrupted save copy shows the one-line sticky notice (E-A1); a killed restore that
  changed nothing stays silent.
- No hint is shown when saving while the game is running (E-X1).
- Per-checkpoint delete uses a 5-second inline countdown with Cancel.
- Recovery is fully automatic (R1–R4); the recovery prompt and the `Recovery needed`
  status are removed from the UI. There is no manual `recover` command.
- Links and junctions resolve to the real directory; operations never consume the link.
- Custom games are kept forever; there is no `Forget this game` in this version.
- The Flush preview isn't binding; a backup changing after the preview is not an error (E-D4).
- A label edit for a save that has since disappeared is dropped quietly, with no error.