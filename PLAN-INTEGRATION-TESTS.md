# SaveScummer integration tests

This sketches the integration tests for the behaviors described in [PLAN.md](PLAN.md). The aim is to establish that SaveScummer's actual implementation works correctly, responds quickly, and recovers safely when something fails. Process and window monitoring is one of several areas to test.

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
- Repeat scans without duplicating games. Detect a newly installed or removed game and apply configured path overrides. Trigger the periodic scan's actual handler without waiting fifteen minutes in each test.
- Discover manual sibling copies at startup, during refresh, when opening history, and when resolving the default LOAD target. Register each copy once as an Existing backup without renaming or modifying it.
- Recognize complete supported native duplicate names, including localized variants and numbered copies. Exclude unrelated folders sharing a prefix, recovery snapshots, and staging directories.
- Allocate a new saved or recovery snapshot name without overwriting an existing directory, including when a competing directory appears during allocation.
- Preserve the distinction between discovery time and save time. Keep externally deleted snapshots in history as unavailable, and disable actions that require those exact snapshots.

Use representative name fixtures for automated tests and a short check against the actual supported file manager. Windows comes first; each additional OS/file manager needs its own verified naming cases.

## 3. SAVE, LOAD, REVERT, and persistent history

Use real temporary directories and a real SQLite database. Compare file contents and directory structure, not just success responses or directory existence.

- SAVE a directory containing nested files, empty directories, and varied filenames. Verify that a completed snapshot has the expected contents and a single history entry, while older snapshots remain intact.
- SAVE with a missing DIR and LOAD with no available saved checkpoint must finish without fabricating a successful history action.
- Default LOAD chooses the latest eligible saved checkpoint, including manual copies, and excludes recovery snapshots. Explicit Restore uses exactly the selected snapshot.
- Before LOAD or REVERT changes DIR, preserve its current contents in a new recovery snapshot. The source snapshot remains unchanged after restoration.
- Exercise a sequence such as Save A, modify, Load A, modify, Revert that load, and Revert that revert. Verify every resulting DIR, recovery snapshot, history reference, and action target.
- Restore an older checkpoint without deleting later history. Use equal timestamps to verify that IDs and ordering still distinguish separate actions.
- Reject an unavailable explicit target without substituting a different snapshot or creating unnecessary recovery data. If the current DIR is missing, restoration must stop safely.
- Restart SaveScummer and verify that configuration, snapshots, history, and exact Restore/Revert relationships survive. Game exit must not clear them either.
- Flush history only after confirmation, preserve current DIR, and include manual saved copies and retained recovery data in the intended deletion scope. On partial deletion failure, retain records for what remains and report the failure.

## 4. Failures and interrupted operations

Exercise failures in the real operation sequence. Use actual file locks and permissions where practical, plus small controlled failure points where an OS failure would otherwise be hard to reproduce. Do not build a general fault-injection framework.

- Fail a SAVE partway through copying. An incomplete snapshot must not become a usable checkpoint or a completed Saved entry.
- Fail while copying current DIR into recovery, while preparing replacement data, during replacement, and while writing the database. Verify both files and operation records after each failure.
- Verify successful rollback after replacement fails. If rollback also fails, retain needed recovery/staging data, report Recovery needed, and block operations that could change or delete that game's data.
- Terminate a test host running the real operation code at meaningful boundaries, including after filesystem replacement but before the completion record is committed. Reopen the same database and directories through the real startup recovery path.
- Interrupted or failed work must not appear as a completed history action. Preserve recovery material, surface the interruption, and keep operations blocked while the outcome remains uncertain.
- Have the fake game write saves slowly, update multiple related files, hold a file open, or exit during copying. Check what is captured and how failure is reported; this also establishes the practical limits of copying a running game's files.

Ordinary file copying does not by itself establish a consistent game-level save across concurrent writes. These cases must inform the supported behavior; a test should not silently assume that consistency has been solved.

## 5. Operation entry points, shortcuts, and feedback

Check that UI actions, history actions, shortcuts, and Explorer requests reach the same operation handling and per-game lock. Most checks can call the real entry-point handlers; a few desktop checks must exercise actual key delivery and UI wiring.

- Ctrl+F5 and Ctrl+F9 operate on the correct active-stack game, including while SaveScummer is hidden. Exercise switching games immediately before a shortcut and having no running known game.
- Hold a shortcut and press it repeatedly. One held press must not repeatedly perform operations, and busy requests must be rejected without running later from a queue.
- Start an operation through one entry point, then request another through a different entry point for the same game. Also try Flush history and path changes while busy. Keep the lock through any required rollback.
- Check shortcut registration conflicts, fullscreen use, and permission differences in desktop compatibility runs.
- Verify the correct start, completion, failure, or busy sound is requested. Completion follows committed files and history; failure never produces a success cue. Busy feedback is rate-limited, and disabling shortcut sounds suppresses the cues.
- For very fast operations, preserve distinguishable cue ordering without delaying file work or extending the operation lock. Keep actual audibility and sound quality as a short manual check.
- Verify visible failure explanations and notifications when hidden.

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
- Start an operation while hidden, then open the window or history. Show current progress and disable every conflicting action, including the Load arrow, history actions, Flush history, and path changes.
- Keep progress active through copying and rollback. Do not show completion before files and history commit. Check recovery-needed blocking, error visibility, and accessibility state.
- Verify history action availability, exact targets, unavailable snapshots, and the separate availability of the Load button and history arrow. Explore opens the correct parent directory.

## Speed, repeatability, and compatibility

Record timings alongside these scenarios: launch/exit/focus detection, discovery, and file operations for representative small and large save directories. For repeat runs, report typical and tail latency, missed/duplicate events, and resource growth. Include an idle-monitor CPU check. Use independent test/fixture observations where possible and state what each timing actually measures; a launch request timestamp is not the process's exact start time.

Agree on useful latency and resource budgets after an initial baseline on a recorded machine and OS. Correctness tests need bounded timeouts, but tight performance thresholds should run separately so a temporarily busy machine does not obscure functional failures. Repetition should be an option of the normal test runner or a simple loop.

Run file/database cases without requiring desktop interaction. Run focus, shortcuts, tray, and Explorer cases in an interactive desktop session. Explicitly report cases that cannot run in the current environment. Keep sleep/resume, elevation, actual fullscreen games, and localized Explorer behavior in a short compatibility checklist until automating them is worthwhile.

Begin on Windows. Keep reusable scenarios when adding macOS or Linux, but verify each platform's actual monitoring, shortcuts, file operations, and supported file-manager integration independently. Fixture success alone does not establish compatibility with every real game.

## Decisions the tests should help resolve

Where PLAN.md leaves behavior open, agree on the expected result before turning it into a fixed assertion:

- How to order manual checkpoints whose original save time is unknown when choosing the latest LOAD target.
- Which launcher/helper processes belong to a game, and how to order several games already running when monitoring begins.
- What consistency can be promised while the game is actively writing, and what the user sees when that promise cannot be met.
- How users resolve Recovery needed after an interrupted operation with an uncertain outcome.
- What should happen when an Explorer request arrives while SaveScummer is not running.

## Starting order

1. Add the basic fake game and launch/exit/activation integration tests alongside the real monitor.
2. Add directory and SQLite tests alongside SAVE, LOAD, REVERT, and discovery; cover failure and restart recovery before trusting real saves.
3. Add entry-point and desktop checks as shortcuts, Explorer integration, and the UI become available.

The scenario list is the direction of travel. Each implementation step should add the small set of tests and fixture options it needs, using the same ordinary test runner throughout.
