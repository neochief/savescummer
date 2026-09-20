# Save Scummer

I want to create a Windows app that would help me save and load backups of game save files or data, mainly for roguelike games.

The app's data model should have a library of known games and the directory (we'll call it DIR) to back up for each game (basically, either the save game dir or the entire game data dir if the game is clever enough to prevent tampering with the saves).

Ideally, this app should be cross-platform and work on Windows, Linux and macOS.


## SCAN, GAME LIBRARY, KNOWN GAMES

When the app is launched, it should perform a quick scan for gaming platforms and games that are present on the user's computer. Found platforms and games are displayed in the main app window's list. The scan is performed each time the app starts and periodically (once every 15 minutes).

## Game platforms

### Steam

%STEAMAPPS% (for example: "C:\Program Files (x86)\Steam\steamapps\common")

This should be detected first, so that patterns like %STEAMAPPS% can be expanded when the scan for games is being run.

## Game library

Our library should have known data about each game that would let the app detect whether it's installed or running.

- Game name (e.g. "Void War")
- Game icon (path?)
- Patterns to look for the install location (e.g. "%STEAMAPPS%\Void War")
- Patterns to look for the save/data DIR (e.g. "%APPDATA%\Void War")

Any game that was successfully found should have a new entry in the KNOWN GAMES data structure that is filled with:

- Game name
- Game icon
- Path to the game executable
- Path to the game's data DIR

## MONITOR and ACTIVE STACK

After the app finishes the initial scan, it should track the launch, activation and termination of the KNOWN GAMES executables in the central data structure called ACTIVE STACK. It would help with displaying the relevant information in the game's widget in the app's UI and also communicate which game to operate on when the global shortcuts are used.

There should be a focus stack of the launched games, and the shortcuts should only work with the latest game in the stack. There should be just one element per launched game in the stack. Two processes for the same game count as one in the stack. If all game processes are terminated, the entry is removed from the stack. If the game is focused, it moves to the top of the stack.

For example, I have FTL launched. The stack contains just that game. Now I launch Void War and it gets focused. The app detects that and adds Void War to the stack. Now if I alt-tab to FTL, FTL moves to the top of the stack. If I exit FTL, it's removed from the stack. Void War is on top of the stack, even though it may currently be in the foreground.

Game launches and closes add history markers. These markers do not create snapshots and have no Restore or Revert action. Closing a game removes it from ACTIVE STACK but does not delete any snapshots or history. Relaunching the game continues the same history.


## SNAPSHOTS, HISTORY and STORAGE

Snapshots contain game files. History entries describe what happened and reference snapshots by stable IDs. Displayed timestamps are labels, not identifiers; actions in the same second must remain distinct.

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

- Saved snapshots follow the OS duplicate-directory naming conventions. Existing sibling copies matching those conventions are valid saved snapshots, regardless of whether the app created them. Import them as "Existing backup" entries without inventing SAVE events. Their import time must not be presented as the time they were saved.
- Recovery snapshots use a separate reserved naming convention with unique IDs, such as "Void_War.recovery-000001". Never overwrite an existing directory when allocating a new snapshot. Recovery snapshots are excluded from default LOAD selection.
- The app treats completed snapshots as read-only. Restoration copies their contents into DIR and leaves the source snapshot intact.
- A local SQLite database stores game configuration, snapshot IDs, kinds, paths and timestamps, and history entries. Game files remain in the sibling directories, not in the database.
- History entries have stable IDs and a stable chronological order, including when timestamps are equal. Saved and Existing backup entries reference their saved snapshots. Loaded and Reverted entries reference the target history entry, the snapshot restored, and the recovery snapshot captured immediately before the operation.
- SAVE, game exit and app restart never delete existing snapshots or history. Only an explicit, confirmed Flush history operation deletes them. There is no automatic expiry or single UNDO PATH.
- If a snapshot is deleted externally, retain its history entry, mark it unavailable and disable the affected action. Never silently substitute another snapshot.

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


## OPERATION SAFETY

The app restores files on disk. The user is responsible for making the game pick up the restored state, for example by reloading or restarting the game. This applies to both Restore and Revert.

All entry points (UI, global shortcuts and Explorer) use the same operation handling and per-game operation lock. Allow only one operation per game at a time, including Flush history and changes to configured paths. Reject additional requests while that game is busy; do not queue them for later execution. Disable conflicting UI actions and give brief, rate-limited busy feedback for shortcuts and Explorer requests. Holding a shortcut must not repeatedly trigger operations.

For every Restore or Revert:

1. Resolve the requested history entry and check that its snapshot is available before creating any new snapshot.
2. Copy the current DIR into a new recovery snapshot. If DIR is missing or the copy fails, stop with a clear error and leave DIR untouched.
3. Prepare the requested replacement in a separate staging directory before changing DIR. Incomplete copies must not appear as usable snapshots.
4. Replace DIR while retaining enough data to recover if replacement fails. Attempt rollback on failure; retain recovery and staging data needed for recovery if rollback cannot complete, and report the failure.
5. Mark the operation complete only after successful replacement. Keep both the source snapshot and the newly captured recovery snapshot.

Filesystem changes and database changes cannot share one transaction. Persist a pending/completed/failed operation record so startup can identify interrupted operations. Preserve their recovery files and surface the interruption instead of silently treating it as a successful load or deleting recovery data. Failed and pending operations must not appear as completed history actions.

SAVE also publishes a snapshot and its history entry only after its copy succeeds. Flush history reports deletion failures and retains records for remaining snapshots instead of claiming that cleanup completed.


## SAVE

When save is triggered, I want the app to:

1. Check if the game DIR exists. If not, finish the SAVE operation.

2. Create a copy of that dir as a new sibling folder following the OS conventions for duplicate dirs (for example, for Windows, it's "Void_War - Copy", "Void_War - Copy (2)" and so on). There can be multiple copies of the DIR; this is expected.

3. After the copy succeeds, register the saved snapshot and append a Saved history entry. Keep all existing saved snapshots, recovery snapshots and history entries.


## LOAD

When load is triggered, I want the app to:

1. Select the latest available saved snapshot, including valid imported existing backups, unless a specific saved snapshot was requested. Recovery snapshots must never become the default LOAD target. If no saved snapshot is available, do nothing and finish LOAD.

2. Preserve the current DIR and restore the selected snapshot using the OPERATION SAFETY steps.

3. Append a Loaded [target] history entry referencing the selected saved entry and the new recovery snapshot. Its Revert action restores the state from before this particular load.

The Restore action on a Saved or Existing backup history entry runs LOAD with that entry as the explicit target.


## REVERT

When Revert is selected for a Loaded or Reverted history entry:

1. Select the recovery snapshot captured immediately before that specific operation. If it is unavailable, stop with a clear error.
2. Preserve the current DIR as a new recovery snapshot and restore the selected recovery snapshot using the OPERATION SAFETY steps.
3. Append a Reverted [target] history entry referencing the operation being reverted and the new recovery snapshot. Its own Revert action restores the state from before this revert.

Keep all earlier snapshots and history entries, including the recovery snapshot just restored.


## Global shortcuts

Ctrl+F5 (SAVE operation)
Ctrl+F9 (LOAD operation)

### Shortcut sounds

SAVE and LOAD triggered through global shortcuts have separate start and completion cues. The start cue means the request was accepted and the operation has started; the completion cue means the file operation and its history record have successfully committed.

| Event | Sound |
| --- | --- |
| SAVE started | Two short, soft ascending notes |
| SAVE completed | A brighter ascending resolution |
| LOAD started | Two short, soft descending notes |
| LOAD completed | A rounded descending resolution |
| Operation failed or could not start | A distinct, low double knock |
| Request rejected because the game is busy | One quiet, dry tick |

Start cues should be approximately 100 ms and completion cues approximately 200 ms. If an accepted operation fails, play the failure cue instead of completion. If it cannot start, play only the appropriate failure or busy cue. Rate-limit busy cues to avoid audio spam.

For fast operations, sequence the start and result cues so both remain distinguishable; audio timing must not delay file operations or extend the operation lock. Do not loop sounds during copying. Failures also leave a visible explanation in the game widget and produce a notification when the app is hidden.

Provide one app-wide "Shortcut sounds" setting, enabled by default. It controls these cues without per-sound configuration.


## Explorer extension

I also want to register two Explorer extensions that would show up in the Explorer context menu:

Save - it should only appear if I right-clicked on DIR; it should do the save operation.
Load - it should only appear if I right-clicked on an ordinary saved copy of DIR; it should run LOAD using that backup, including preserving the current state and recording history. Recovery snapshots are accessed through their Revert actions in the app history.


## UI

Keep the UI slick, minimal and compact. Use spacing, restrained emphasis and control states to communicate routine activity. Avoid redundant headings, status captions and implementation details. Preserve clear action labels and show concise error text when user action is needed.

### Main window

It should show a scrollable list of widgets, one per game, for games that are in our library and that are detected on this computer. If there are no installed games, the list is empty and shows the text "No known games installed on this computer." The list shows launched games in the ACTIVE STACK first in the proper order, and then all installed known games. The idea is to have the launched active games on top. If the game is closed, it leaves ACTIVE STACK. If a scan finds that the game is no longer installed, the entry should disappear.

Each game widget should show the following:

```text
-----------------------------------------------
ICON + NAME (Running)

|----| |---------------|---| |---------|
|Save| |      Load     | ↓ | | Explore |
|----| [ 4 seconds ago ]---| |---------| [...]
-----------------------------------------------
```


The bottom row contains buttons.

### Busy state and progress

Whenever SAVE, LOAD or REVERT runs for a game, immediately disable both its Save and Load buttons, including the Load dropdown arrow. Keep their normal labels and subtly dim the disabled controls. Show one thin shared progress bar integrated along the bottom of the Save/Load control group, without expanding the widget or shifting its layout. Apply this state regardless of whether the operation was started from a button, history, a shortcut or Explorer. If the window was hidden, opening it must show the current busy state and progress.

Do not add routine status captions such as "Saving...", "Loading..." or "Finishing...", percentage text, or "disabled" labels. The control state and progress bar provide the visible feedback. Expose the operation and progress through accessibility properties. Show measured progress when the amount of work is known, covering all required copies rather than only the first copy. Use an indeterminate bar while calculating the work or during phases without measurable progress. Do not fill the bar completely or announce completion until the files and history record have successfully committed.

```text
[Icon] Void War                                  Running

[ Save ]  [ Load  v ]  [ Explore ]  [ ... ]
━━━━━━━━━━━━────────
```

In this mockup, Save and Load are visually dimmed and inactive; the thin line is the progress bar. No extra visible label is added.

Restore and Revert actions in any already-open history, Flush history, and changes to configured paths are also disabled for that game while busy. Backend locking enforces the same restriction for every entry point; disabling buttons alone is insufficient.

Keep the busy state until the operation and any required rollback have finished. After success or a safely handled failure, remove the progress bar and re-enable controls according to snapshot availability. Show failures as an error, not as a completed progress bar. If rollback cannot finish or an interrupted operation has an uncertain outcome, show "Recovery needed", retain the recovery files, and keep operations that could change or delete the game's data blocked until recovery is resolved.


### Save

Save is a button that does the SAVE operation.


### Load

Load is a split button. The main button runs the default LOAD operation using the latest available saved snapshot. The arrow opens the game's history, newest first, grouped by day.

- Saved and Existing backup rows offer **Restore**.
- Loaded [target] and Reverted [target] rows offer **Revert**.
- Game started and Game closed rows are compact, visually subdued markers without actions. Repeated launches and closes remain in the same daily timeline rather than creating separate session panels.
- Unavailable snapshots are clearly marked, with their affected actions disabled.

References such as [19:25] identify the target history entry. Include the date or additional detail when needed to distinguish targets; use stable IDs internally. Selecting Restore or Revert runs that action for the selected row immediately and appends the resulting event after success.

The latest available saved snapshot's time appears below the main Load caption in smaller, subtler type. Relative labels can use the following formats:

- 4 seconds ago
- 2 minutes ago
- 1 hour and 12 minutes ago
- yesterday, 23:20:12
- Wednesday, 12:23:22
- 2012-12-12, 12:12:21

History rows use explicit times within their day groups so that operations and their targets can be distinguished. The history list scrolls when it exceeds the dropdown's available height.

When the game is idle, disable the main Load button if no saved snapshot is available. Keep the history arrow available whenever history exists, even if only recovery points or session markers remain. During an operation, the busy-state rules disable both parts of the control. Revert actions live in history; there is no separate single-undo button.

Example with several relaunches in one day (the history dropdown is open):

```text
[Icon] Void War                                  Running

[ Save ]   [     Load      v ]   [ Explore ]   [ ... ]
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


### Explore

Should open the dir containing the DIR in the OS file explorer.


### ...

Should open a dropdown with miscellaneous actions:

- Configure

    Should open a small popup where you would see:

Game executable: [...prefilled path...][open icon] [Reset]
Game data dir (DIR): [...prefilled path...][open icon] [Reset]

[Save] [Cancel]

Changing the path and saving would update the entry in KNOWN GAMES.


- Flush history...

    Enable this action when any saved snapshots, recovery snapshots or history entries exist, including recovery data retained after interrupted operations.

    Show a confirmation with separate counts: "This will permanently delete X saved backups and Y recovery points, and clear this game's history. Current game data will be kept." Include any retained incomplete recovery copies in the deletion scope and confirmation.

    On confirmation, remove the game's saved and recovery snapshots, including imported existing backups, and clear its history. Leave the current DIR untouched. Only clear records for snapshots whose deletion succeeded; report any failures. This is the only app action that deletes retained snapshots and history.


## Autolaunch

Above the main window list, there should be a checkbox:  [X] Start with the system

The app can be launched minimized with the "--minimized" flag, in which case it starts minimized to the tray.

By default, if you launch the app, the main window is shown and focused. When you close the app, it's minimized to the tray.

Clicking on the tray icon shows the main window.

The tray icon's context menu has two items:

- Main window
- Exit


# OS specifics

## Naming conventions for directory copies

Ordinary saved snapshots follow the conventions below. Recovery snapshots use the separate reserved "<DIR name>.recovery-<unique ID>" convention on every platform, with filesystem-safe names and collision handling.

- Windows: "Void_War", "Void_War - Copy", "Void_War - Copy (2)"
- macOS: unknown
- Linux: unknown

## Global shortcuts

- Windows: possible
- macOS: unknown
- Linux: unknown


## File explorer, Finder extension or variants

- Windows: possible to extend the Explorer context menus
- macOS: unknown
- Linux: unknown
