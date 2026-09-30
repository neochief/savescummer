# Host-owned action policy

Status: implemented. This is the canonical scope for action availability and guidance.
It supersedes the earlier proposal that combined this work with game launching,
automatic exit checkpoints, and content-based Save/Load suppression.

The host remains the authority. Keep the existing operation executor, per-game
reservation, checkpoint format and recovery journal. Consolidate the decisions
around them instead of adding another state machine.

Related documents: [host and protocol](PLAN-HOST.md), [UI](PLAN-UI.md),
[operation failures and recovery](PLAN-ERRORS.md).

## Four separate facts

A single ordered list of “lockdowns” cannot describe both admission and presentation.
A running Delete must reserve the game without replacing a useful explanation of
why Save is unavailable. An interrupted Load must remain visible while Retry is
running. Report these independently:

| Fact | Question answered | Host field |
|---|---|---|
| Activity | Is work checking, accepted or running? | `busy`, pending deletions |
| Availability | Can this action start now? | `save`, `load`, `restore`, `delete`, `flush`, `configure`, `retry` |
| Guidance | What stable condition needs attention? | `guidance` |
| Result | What happened to the requested operation? | operation outcome, `last_result` |

Each availability has `available` and, when refused, a reason and structured failure.
Guidance contains one kind, whether it covers Save and/or Load, the relevant failure,
and an optional remedy. It ignores temporary activity. Clients render those fields;
they do not infer policy from process status, error text or missing checkpoints.

## REASONS

`apps/host/src/policy.rs` is the common policy for summaries and command admission.
The UI, CLI and hotkeys all enter the host executor.

Evaluate only the conditions relevant to the requested action:

1. Refuse during startup or shutdown.
2. Refuse when another operation owns the game or the store is moving. Delete may
   begin its existing countdown while another game operation runs; actual deletion
   waits for exclusive ownership. Duplicate countdowns for one checkpoint are refused.
3. Allow Configure after those checks. Configuration can repair a blocked game;
   recovery still uses the original journal paths.
4. An unresolved interruption blocks Save, Load, Restore, Delete and Flush. Retry
   alone is allowed to attempt recovery.
5. Check checkpoint-store access and reachability.
6. Retry requires a blocked game, known process observation, access to its recorded
   paths, and the exit rule. It does not use newly configured save locations.
7. Delete and Flush need no live-save location or exit check.
8. Save, Load and Restore require valid, accessible targets; then check the exit rule.
9. Save requires matching live data. Main Load requires a usable deliberate
   checkpoint. Row Restore depends on its particular checkpoint instead.

A command reserves its game before slow preflight work. This reservation appears as
`checking`, not `accepted`: acceptance still means the operation is durably recorded.
Target refresh and store checks run outside the shared state mutex. Before committing
acceptance, recheck policy under the reservation. A refusal releases it and publishes
fresh state. Other games remain usable during file checks and recovery.

Detailed file checks still belong to the executor. Availability is an observation,
not a promise that files cannot change before a command arrives. The executor checks
checkpoint identity, target compatibility, space and file safety at the request.

Hotkeys use the same result. A request rejected because another operation owns the
game produces the short busy cue. Lockdowns, including a running game, no game data
and no saves, produce the failure cue. A hidden-window failure can notify with the
reason. No action is queued for game exit.

## Exit rule and recovery

*Wait for the game to close before saving or loading* remains on by default, per game.
Save, Load, row Restore and Retry refuse while a matching process is observed. This
includes a game withheld from the active stack because macOS access is missing.
Delete, Flush, labels and Configure do not touch live saves and need no exit check.

Turning this setting off permits expert use while running; it does not disable the
existing open-file checks or rollback protections. Configure explains:

> While a game runs, Save may copy stale or partial progress, and the game may
> overwrite a Load. Turn this off only if you know when the game writes and reads
> its saves.

The rule observes configured game processes. It cannot prove that an unrecognized
helper, cloud service or newly launched process will not write later. There is no
arbitrary settling delay after observed exit.

At startup, observe processes for persisted games **before** resolving interrupted
operations. If recovery would touch live paths and the game is running, process
observation is unavailable, or a recorded path is inaccessible, preserve the journal
and files and report the game blocked. Safe recovery for other games continues.
After the problem clears, the user can Retry; startup also retries when eligible.

Retry is an ordinary tracked operation: reserve the game, preflight, durably accept,
run the existing recovery rules, publish an outcome and release ownership. Repeating
its request ID returns the same operation. Concurrent Save, Retry or Configure is
refused. A failed Retry retains the original interrupted operation and its material.
Its own tracking record must not become a replacement recovery journal.

Rollback within an already accepted Load remains part of that Load and keeps its
reservation. The exit rule does not interrupt rollback halfway through.

## Checkpoints and history

Every successful Save creates a deliberate checkpoint, even if its contents match
an older one. Main Load picks the newest usable deliberate checkpoint. For example:
Save A, Save B, load A, Save C → main Load restores C (A's contents). Do not silently
leave B as the default because C duplicates A.

Load and Revert keep their current recovery checkpoint behavior. Game started and
Game closed remain history markers. Steam's `steam_autocloud.vdf` and existing
reserved-file exclusions remain centralized in the existing snapshot walk.

History pages describe checkpoint-specific eligibility: the saved/recovery kind,
identity and target compatibility. Pending deletion comes from the live state. The current game's action gates
come from the live summary. Effective row Restore is the row's eligibility **and**
`game.restore.available`; effective Delete uses `game.delete.available` and the live pending-deletion list likewise.
Clients never recompute either component.

Busy, running and recovery transitions therefore take effect on already displayed
history without refetching every page. Changes to checkpoint identity or target
compatibility invalidate history normally. Permission/configuration changes refresh
relevant eligibility. A temporarily unavailable store does not retire checkpoints.

## LOCKDOWN PANEL

The host selects stable guidance in this order: unresolved recovery, store/location/
target problem, exit rule, then missing data/checkpoints. Activity never changes the
panel's identity. Store unavailability has an app-wide notice; ordinary busy work has
its existing control progress.

| Guidance | Covers | Remedy |
|---|---|---|
| Interrupted operation | Both | Retry; first allow access when recovery needs it |
| Access needed | Both | Allow access, or System Settings after denial |
| Save location unknown | Both | Configure |
| Invalid target | Both | Configure |
| Target unavailable | Both | Reconnect the drive/location |
| Game running with exit rule | Both | Save and exit the game |
| No live data and no checkpoints | Both | Play first |
| No live data, usable checkpoint exists | Save | Load remains usable when its other checks pass |
| Live data, no deliberate checkpoints | Load | Save remains usable when its other checks pass |

Use one neutral panel over the affected buttons; no warning styling or wait cursor
merely because guidance exists. Keep the other button usable when only one is covered.
History Restore controls retain their space while hidden for stable unavailability;
activity disables them in place. Delete keeps its separate host gate and countdown.

Configure respects the host's configure gate. Retry stays in the recovery panel and
shows its own progress; closing the game makes it available without replacing the
panel. Permission prompts and settings links go through the host. The host supplies
the category, denial state and settings URL. Invalid-target failures supply a cause
and paths, including the other game's identity for overlaps. The UI formats them;
it does not parse diagnostic strings to discover a condition.

An attempted operation's error remains a separate result. Do not recreate current
configuration or recovery guidance as an additional generic error block.

## Protocol and rollout

Protocol version 2 adds the action gates, guidance, structured failure context and
`checking` status, and changes Retry to the ordinary operation response. Host, CLI
and Tauri UI ship together; mismatched versions are rejected. No checkpoint data
migration or alternate checkpoint format is introduced.

The implementation sequence is policy and structured reasons, executor/Retry,
startup observation, history gates, then client presentation. Keep the executor and
journal as the sole mutation path throughout.

## Acceptance evidence

The repository tests cover these observable flows:

- A running game reports matching summary and command refusals; already rendered
  history also loses Restore without a page reload. Delete does not replace guidance.
- A delayed Retry owns its game: Save, Configure and a second Retry refuse, while
  another game saves successfully. The request ID replays the same operation.
- Restart after a Load crash while the fake game is running leaves live files intact
  and the game blocked. Exit then Retry resolves the original journal.
- A failed Retry keeps recovery material; making the recorded paths coherent permits
  Retry to release the game without deleting retained evidence.
- Save A, Save B, load A, Save C makes C the default and loading it restores A.
- A Load-only no-checkpoints panel leaves Save usable. Access buttons invoke the host;
  denied access opens host-selected settings. Retry shows operation progress.
- Existing checkpoint, permission, privacy, protocol, failure and shutdown suites
  remain part of the workspace quality checks.

## Deferred features

Game launching, automatic exit checkpoints and content manifests/deduplication are
separate product decisions. None is needed to make the current workflow predictable.
They introduce launch ownership, background scheduling/cancellation, storage growth
or checkpoint-selection semantics and require their own use case and acceptance flows.
There are no `closed` checkpoints, launch command, `unchanged` refusal or
`already_loaded` refusal in this implementation.
