# SaveScummer integration tests

This sketches the integration tests for the behaviors described in [PLAN.md](PLAN.md). UI behavior is specified in [PLAN-UI.md](PLAN-UI.md) and the failure and interruption scenarios in [PLAN-ERRORS.md](PLAN-ERRORS.md). The aim is to establish that SaveScummer's actual implementation works correctly, responds quickly, and recovers safely when something fails. Process and window monitoring is one of several areas to test.

Keep the test machinery simple: ordinary tests using the project's standard test runner, temporary files and databases, and one small fake-game executable. Add scenarios as the corresponding app features are implemented. A separate dashboard, custom scenario language, or test orchestration platform is unnecessary.

## Approach

- Exercise the production components directly, using real OS processes, windows, files, and SQLite where those integrations are under test. These tests can begin before the main UI exists.
- Use one fake-game executable with a few options for the behaviors a test needs. Run copies from different paths to represent different games, and multiple instances of the same copy to represent one game with several processes.
- Start with a basic window and normal exit. Add delayed window creation, a launcher/child mode, crashes, and simple save-file writes or locks when scenarios need them. The fixture should stay small.
- The monitor must discover the fake game through the same OS mechanisms used for real games. Fixture output can establish readiness or expected behavior, but must not supply the monitor with detection events.
- Use a fresh temporary game directory, configuration, and database for each test. Operate only on test-owned processes and data, and clean them up even after failure. Run desktop tests that compete for focus or shortcuts sequentially.
- Wait for expected events or state changes with bounded timeouts. Use simple synchronization to reproduce races or interruption points instead of relying on lucky timing and long fixed sleeps.
- Keep results in normal test output: assertions, useful event logs on failure, and timings. Pure ordering and naming rules can also have small unit tests; they do not all need a running desktop.

## 1. Game launch, exit, activation, and active stack

Use the fake game to exercise the real monitor and its connection to the active stack and history.

- Start the monitor before the game, and start it while the game is already running. Include a launch or exit during monitor initialization.
- Detect normal exit, forced termination, crashes, and rapid launch/exit/relaunch cycles, including a process that exits before showing a window.
- Delay window creation and distinguish process existence from window activation. Closing a window alone must not count as process termination if the process continues running.
- Switch between two known games, between windows belonging to the same game, and between a game and an unrelated app. Check that the most recently activated running game remains the shortcut target when an unrelated app has focus.
- Minimize and restore the game, and exercise windowed and fullscreen behavior. Use representative real games for fullscreen coverage beyond what the fixture provides.
- Run several processes for one game. Keep one stack entry, retain it when one process exits, and remove it when the last matching process exits.
- Exercise a launcher that starts a separate game process and then exits. Verify executable matching, including an unrelated executable with the same filename at a different path.
- Restart the monitor and check that it reconstructs current state. Check resume from sleep and permission differences between the monitor and game in desktop compatibility runs.
- Verify one game-start/game-close marker per observed game transition, with no snapshots or duplicate markers caused by additional processes. Keep history across relaunches, and do not invent exact transition times for periods when monitoring was stopped.

Check the resulting stack and shortcut target as well as individual events. Record detection latency and missed or duplicate transitions during repeat runs. Finding the right current state after a rescan does not establish that every intervening transition was observed.

## 2. Platform, game, and checkpoint discovery

Use temporary platform/game layouts and the real discovery code. Supply test search locations through ordinary configuration or a small test seam; do not depend on a developer's installed games.

- Resolve platform locations before expanding game patterns, then populate the known game's executable and data paths correctly. Cover multiple candidate locations, missing directories, and paths containing spaces or non-ASCII characters.
- Deduplicate Steam installation aliases using filesystem identity. Cover case aliases on insensitive filesystems, distinct case-only names on sensitive filesystems, and Unix symbolic-link aliases.
- An unavailable protected Steam library or another game's unavailable directory must not block an accessible game's Save/Load. Keep recorded overlap checks and reject operations on an unavailable target itself.
- Repeat scans without duplicating games. Detect a newly installed or removed game and apply configured path overrides. Trigger the periodic scan's actual handler without waiting fifteen minutes in each test.
- Reject identical, ancestor and descendant DIRs across configured games, including unavailable games. Cover both catalog candidates and user overrides. Verify that the error identifies the conflicting game and that an invalid override leaves the previous configuration unchanged.
- Exercise equivalent path spellings, filesystem case rules and directory aliases where supported. Reject overlaps after resolution, while accepting distinct sibling names such as Game and Game2. Validate an otherwise valid missing DIR without creating it, and recheck locations if an alias changes before an operation.
- Reject protected disk/share, user, system, application-data, Documents/Saved Games and shared installation/library roots and their ancestors. Use resolved-root fixtures, including redirected folders and Proton equivalents; no test should copy, replace or delete an actual system directory.
- Accept game-specific children of protected roots and valid shallow directories. Do not reject an unrelated folder merely because its name resembles a protected root. Verify that invalid discovered DIRs leave games visible with configuration errors and that core validation blocks file operations independently of UI controls.
- Discover manual sibling copies at startup, during refresh, when opening history, and when resolving the default LOAD target. Register each copy once as an Existing backup without renaming or modifying it.
- Recognize complete supported native duplicate names, including localized variants and numbered copies. Exclude unrelated folders sharing a prefix, recovery snapshots, and staging directories.
- Allocate a new saved or recovery snapshot name without overwriting an existing directory, including when a competing directory appears during allocation.
- Preserve the distinction between discovery time, estimated folder modification time and known save time. Default LOAD orders manual checkpoints by the modification-time estimate without inventing a SAVE timestamp.
- Delete all backups manually, refresh, then recreate some by copying and pasting folders under their original names. Verify new snapshot/history IDs, old generations marked removed and omitted from visible history, correct default LOAD selection, and rejection of stale explicit targets.
- Replace a directory between scans and while the host is stopped. Also modify a backup in place, including only a nested file or the root modification time. Retire the previous generation and import the changed saved folder exactly once. A changed recovery folder must invalidate its Revert target without becoming an ordinary checkpoint.
- Make a backup or its parent unreadable or temporarily unavailable. Do not retire the old generation or import a partially inspected replacement; restore access and verify normal discovery resumes.
- Replace, modify, delete or add a backup after a Flush preview. Reject its stale confirmation without deleting files; a fresh preview and confirmation must succeed and preserve live game data.

Use representative name fixtures for automated tests and a short check against the actual supported file manager. Windows comes first; each additional OS/file manager needs its own verified naming cases.

## 3. SAVE, LOAD, REVERT, and persistent history

Use real temporary directories and a real SQLite database. Compare file contents and directory structure, not just success responses or directory existence.

- SAVE a directory containing nested files, empty directories, and varied filenames. Verify that a completed snapshot has the expected contents and a single history entry, while older snapshots remain intact.
- SAVE with a missing DIR and LOAD with no available saved checkpoint must finish without fabricating a successful history action.
- Default LOAD chooses directly from checkpoint metadata, including manual copies, and excludes recovery, unavailable and removed checkpoints and checkpoints for another game or original data directory. Verify that timeline filtering and history ordering do not change the selected source, including a store fixture with eligible checkpoint metadata and no Saved/Existing backup row.
- For equal selection timestamps, use durable checkpoint registration order; verify the result survives restart and does not depend on history sequence. Keep known save time and estimated manual modification time distinct.
- Explicit Restore and Revert use the exact checkpoint IDs referenced by their history rows. Exercise stale, wrong-game, wrong-directory and wrong-kind targets through the same core validation path, and reject them before creating recovery data. A visible history row must not override checkpoint eligibility.
- Change a game's DIR from A to B: A's saved and recovery checkpoints remain recorded but cannot restore into B. Return to the same resolved A and verify both types become eligible again without new checkpoint/history IDs or duplicate imports. Cover equivalent aliases and filesystem case rules, including distinct case-sensitive directories.
- Before LOAD or REVERT changes DIR, preserve its current contents in a new recovery snapshot. The source snapshot remains unchanged after restoration.
- Exercise a sequence such as Save A, modify, Load A, modify, Revert that load, and Revert that revert. Verify every resulting DIR, recovery snapshot, history reference, and action target.
- Restore an older checkpoint without deleting later history. Use equal timestamps to verify that IDs and ordering still distinguish separate actions.
- Delete saved and recovery reset points individually through their exact checkpoint IDs. Verify the directories are removed, their rows disappear, unrelated points remain, and stale/wrong-game targets cannot delete anything.
- Keep launch/close markers only for observed sessions containing surviving saved or recovery-backed actions. Remove an old checkpoint and verify its now-empty session disappears, including a session between two still-relevant sessions. With no surviving points, visible history is empty. Recreated backups use their own time estimate and do not resurrect old action references or unrelated sessions.
- Retain a Loaded/Reverted row whose recovery point still exists after its original saved source is removed. Keep the internal source reference unchanged and allow only the exact surviving recovery generation as the Revert target.
- Reject an unavailable explicit target without substituting a different snapshot or creating unnecessary recovery data. If the current DIR is missing, ordinary LOAD or REVERT must stop safely; dedicated interrupted-operation recovery can recreate it as described below.
- Restart SaveScummer and verify that configuration, snapshots, history, and exact Restore/Revert relationships survive. Game exit must not clear them either.
- Convert existing persisted records when removing location UUIDs. Preserve checkpoint/history IDs, ordering and exact source/recovery relationships, and verify the resulting SQLite records and service schemas use checkpoint-owned original-directory paths without independent history location ownership.
- Flush checkpoints only after confirmation, preserve current DIR, and include manual saved copies and retained recovery data in the intended deletion scope. On partial deletion failure, retain records for what remains and report the failure.

## 4. Failures and interrupted operations

Exercise failures in the real operation sequence. Use actual file locks and permissions where practical, plus small controlled failure points where an OS failure would otherwise be hard to reproduce. Do not build a general fault-injection framework.

- Fail a SAVE partway through copying. An incomplete snapshot must not become a usable checkpoint or a completed Saved entry.
- Fail while copying current DIR into recovery, while preparing replacement data, during replacement, and while writing the database. Verify both files and operation records after each failure.
- Verify successful rollback after replacement fails. If rollback also fails, retain needed recovery/staging data, report Recovery needed, and block operations that could change or delete that game's data.
- Terminate a test host running the real operation code at meaningful boundaries, including after filesystem replacement but before the completion record is committed. Reopen the same database and directories through the real startup recovery path.
- Interrupted or failed work must not appear as a completed history action. Preserve recovery material, surface the interruption, and keep operations blocked while the outcome remains uncertain.
- Interrupt before live DIR changes. On restart, verify that current data is untouched, retain the failed/interrupted record and recovery material, and allow ordinary operations without an unnecessary recovery prompt.
- Interrupt after moving the original DIR aside but before installing the replacement. With no UI attached, restart the host and verify automatic rollback restores the exact original data even though DIR is missing. Retain checkpoints and recovery snapshots, persist the resolution before unblocking, and report rollback rather than successful LOAD or REVERT.
- Interrupt after replacement but before recording completion, then write subsequent game progress into DIR before restarting the host. Verify that recovery does not automatically overwrite it and offers Keep current game data or Restore data from before the interrupted operation.
- Choose Keep current game data and verify that file contents remain unchanged, the choice is persisted, and ordinary operations resume. Disable and reject this choice if DIR is missing or inaccessible, including if it disappears between displaying the prompt and handling the command.
- Choose Restore data from before the interrupted operation with DIR present and with DIR missing. Restore the exact pre-operation data; when DIR exists, first preserve its current contents as a new recovery snapshot. If preservation fails, leave DIR untouched and keep recovery unresolved. Keep all earlier snapshots.
- Fail recovery with locks, permissions or unavailable recovery material. Verify the specific error, Open recovery folder and Retry recovery. Opening the folder must not clear the block. Repair the filesystem, then retry or accept the repaired current DIR without editing SQLite.
- While recovery is unresolved, reject ordinary SAVE, LOAD, REVERT, Flush checkpoints and path changes through every entry point. Permit dedicated recovery commands against the recorded original location, allow only one attempt at a time, and show the same status and choices after UI reconnection.
- Interrupt a recovery attempt, including after file restoration but before persisting its resolution. Restart and verify that retained data survives and recovery remains possible without fabricating a completed Loaded/Reverted entry or the interrupted operation's success cue.
- After the model conversion, recover an interrupted operation using its captured original live/source/recovery paths and generation checks. Recovery must not depend on a location UUID or on its history row being present in the visible timeline.
- Have the fake game write saves slowly, update multiple related files, hold a file open, or exit during copying. Verify that actual copy errors follow the failure and rollback rules. Successful copying may capture mixed game states; do not require concurrent-write detection, automatic retries or game suspension.

Copying is explicitly best-effort. Tests must distinguish filesystem success from game-level consistency and must not assert a consistent save while the game is writing.

## 5. Operation entry points, shortcuts, and feedback

Check that UI actions, history actions, shortcuts, and Explorer requests reach the same operation handling and per-game lock. Most checks can call the real entry-point handlers; a few desktop checks must exercise actual key delivery and UI wiring.

- Ctrl+F5 and Ctrl+F9 operate on the correct active-stack game, including while SaveScummer is hidden. Exercise switching games immediately before a shortcut and having no running known game. When the desktop is focused, verify that a shortcut acts on the selected game's Save or Load control, including a stopped game, and that it has no target when nothing is selected and no game is running.
- Hold a shortcut and press it repeatedly. One held press must not repeatedly perform operations, and busy requests must be rejected without running later from a queue.
- Start an operation through one entry point, then request another through a different entry point for the same game. Also try Flush checkpoints and path changes while busy. Keep the lock through any required rollback.
- Check shortcut registration conflicts, fullscreen use, and permission differences in desktop compatibility runs.
- Verify the correct start, completion, failure, or busy sound is requested. Completion follows committed files and history; failure never produces a success cue. Busy feedback is rate-limited, and disabling shortcut sounds suppresses the cues.
- For very fast operations, preserve distinguishable cue ordering without delaying file work or extending the operation lock. Keep actual audibility and sound quality as a short manual check.
- Verify visible failure explanations and notifications when hidden; the scenario catalog and expected messages are in `PLAN-ERRORS.md`.

## 6. Explorer integration

Use temporary game directories for operation tests and a small desktop check for the actual context menu.

- Show Save for a configured DIR and Load for an ordinary saved copy, including a discovered manual copy. Do not offer these actions for unrelated directories or expose recovery snapshots as ordinary Load targets.
- Pass the exact selected path through to SaveScummer, including spaces and non-ASCII characters. Load must restore the clicked backup, even when another backup is newer.
- Exercise requests while SaveScummer is visible, hidden, busy, or not running. Requests must use the normal recovery/history rules and must not execute twice because of app startup or request delivery.
- Check installation and removal of the integration in a disposable user profile or test environment, including cleanup of its registration.

## 7. App lifecycle and visible state

Keep a few full-app checks for behavior that component tests cannot establish. Use short manual checks initially where desktop automation would add more machinery than value.

- Normal launch shows the main window; --minimized starts in the tray. Closing the main window keeps monitoring active, tray actions reopen it, and Exit actually stops the app.
- Check the start-with-system setting and its actual effect in a test environment.
- Show discovered games and active-stack ordering correctly, including the empty-library state.
- Start an operation while hidden, then open the window. Show the operation state for the correct game and disable every conflicting main-content action, including history actions, Flush checkpoints and path changes.
- Keep the busy state through copying and rollback. Do not show completion before files and history commit. Check automatic recovery resolution and error-block visibility against `PLAN-ERRORS.md`, and the accessibility state.
- Verify history action availability, exact targets, unavailable snapshots, and Load button availability. Explore opens the correct parent directory.

## 8. Incremental persistence and paginated history

Use real temporary SQLite databases and the ordinary core/service entry points. Generate large metadata fixtures separately from realistic filesystem-copy fixtures so database scaling and file-copy costs can be measured independently.

- Inspect actual SQL effects using test-only tracing or audit triggers. Saving for game A must not write game B's records or unchanged old records for A. A settings change writes settings/revision metadata only; an operation-phase change updates the affected operation and required revision metadata.
- Fail midway through a multi-record transaction and verify that no partial changes are visible in memory or after reopening the database. Cover admission, checkpoint/history completion, external generation replacement, Flush and recovery resolution. Preserve the existing crash/restart scenarios at each durable file-replacement boundary.
- Verify schema migrations and durable ordering counters preserve IDs, ordering, request idempotency, original-directory ownership and exact source/recovery relationships. Lazy reads must still find old request IDs, unresolved operations, uncached checkpoints and retained path reservations.
- Grow unrelated games' histories and verify that ordinary game-operation updates and unchanged watch ticks do not scan, clone or serialize those histories. Artwork/progress-only changes must not rebuild the visible-history projection. Check live-directory availability updates independently of metadata events.
- Page a game's timeline with equal timestamps, manual-copy time estimates, and sessions crossing page boundaries. At a stable revision, every visible row appears exactly once in the defined order, with the same action targets and session-marker visibility as the complete core projection.
- Invalidate cursors after relevant same-game changes, including backup deletion/replacement, availability changes, Configure A→B→A and Flush. Unrelated-game, artwork and progress-only changes preserve the cursor. Reject cursors for another game or an earlier host instance. Exercise changes during page reads and projection rebuilds without mixing revisions.
- Verify removed snapshots disappear, surviving recovery-backed rows remain, empty sessions stay hidden, and pagination never changes default LOAD selection. Execute an action from an old page after its checkpoint changes and confirm normal core validation rejects it.
- Use data whose full audit state exceeds 8 MiB. Connect, watch, page history, query an exact old operation and inspect paginated Flush details successfully. Assert row/byte caps, explicit oversized-item errors and revision-bound Flush confirmation. Routine summary size must not grow with retained historical records.
- Exercise desktop paging, day grouping, bounded page/widget caches, stale responses after game changes or popup closure, cursor reload with a surviving scroll anchor, busy/recovery updates, and reconnect without command replay. History-arrow availability must work before any history page is fetched.
- Verify CLI page cursors and bounded-memory streaming, including explicit failure on invalidation. Check shared Rust/Qt fixtures and clear version-mismatch handling for the desktop, CLI and Explorer bridge.

## Speed, repeatability, and compatibility

Record timings alongside these scenarios: launch/exit/focus detection, discovery, and file operations for representative small and large save directories. For repeat runs, report typical and tail latency, missed/duplicate events, and resource growth. Include an idle-monitor CPU check. Use independent test/fixture observations where possible and state what each timing actually measures; a launch request timestamp is not the process's exact start time.

Agree on useful latency and resource budgets after an initial baseline on a recorded machine and OS. Correctness tests need bounded timeouts, but tight performance thresholds should run separately so a temporarily busy machine does not obscure functional failures. Repetition should be an option of the normal test runner or a simple loop.

Include histories of 100, 10,000 and 100,000 rows, both concentrated in one game and spread across many games, with proportional terminal journals and retired checkpoints. Measure startup time and peak memory, rows/bytes written per transaction, commit latency, idle watch CPU and payload size, and first/subsequent history-page latency. Measure per-game discovery and projection rebuilds separately from cached page reads. Use deterministic structural assertions for unchanged-row writes, bounded responses and complete pagination in normal tests; report timing distributions separately and establish a supported capacity from recorded measurements.

Run file/database cases without requiring desktop interaction. Run focus, shortcuts, tray, and Explorer cases in an interactive desktop session. Explicitly report cases that cannot run in the current environment. Keep sleep/resume, elevation, actual fullscreen games, and localized Explorer behavior in a short compatibility checklist until automating them is worthwhile.

Begin on Windows. Keep reusable scenarios when adding macOS or Linux, but verify each platform's actual monitoring, shortcuts, file operations, and supported file-manager integration independently. Fixture success alone does not establish compatibility with every real game.

## Decisions the tests should help resolve

Where PLAN.md leaves behavior open, agree on the expected result before turning it into a fixed assertion:

- Manual checkpoint ordering is decided: use folder modification time as an estimate, then durable checkpoint registration order for ties; keep the original save time unknown. History sequence is only for timeline ordering.
- Which launcher/helper processes belong to a game, and how to order several games already running when monitoring begins.
- What should happen when an Explorer request arrives while SaveScummer is not running.

## Starting order

1. Add the basic fake game and launch/exit/activation integration tests alongside the real monitor.
2. Add directory and SQLite tests alongside SAVE, LOAD, REVERT, and discovery, including incremental writes and indexed per-game access; cover failure and restart recovery before trusting real saves.
3. Add bounded service-query, history-pagination, entry-point and desktop checks as the protocol, shortcuts, Explorer integration, and UI become available. Measure growing-history fixtures alongside their storage and read-model implementations.

The scenario list is the direction of travel. Each implementation step should add the small set of tests and fixture options it needs, using the same ordinary test runner throughout.

## Windows core implementation checks (2026-09-21)

The automated suite now covers active-stack shortcut selection with no UI client,
idempotent shortcut retries after focus changes, shared busy admission, and startup
registration/persistence rollback through an injected adapter. File-manager entry
points are checked for exact generations, stale menus, unrelated folders, and
exclusion of recovery checkpoints. Discovery tests cover registry/portable source
deduplication, multiple data choices, invalid-path isolation, persistent choices,
external catalogs, default resets, and confirmed uninstallation without deleting
history. Qt tests also consume the final graceful-shutdown event.

`scripts/build-explorer.ps1` builds the independent native COM DLL and checks COM
factory/interface lifetime, unloading, and empty-selection menu behavior without
registering it. The host uses `--no-integrations` in ordinary automated tests;
none of these checks changes sign-in settings or installs a shell extension.

Interactive qualification remains the explicit list in sections 6–7: actual
Ctrl+F5/Ctrl+F9 delivery (including held keys/fullscreen/elevation), tray reopen and
focus, notification delivery and Explorer restart, sign-in launch, and extension
registration/removal in a disposable profile. The classic Explorer extension
currently hides its menu if the default host is absent or fails its bounded query;
it does not start a daemon on arbitrary folder context-menu requests.
