# Save Scummer UI

This is the main window. Core behavior (SAVE, LOAD, REVERT, snapshots, operation safety) lives in [`PLAN.md`](PLAN.md); the error block's contents live in [`PLAN-ERRORS.md`](PLAN-ERRORS.md).

The app should feel like a game-oriented desktop utility: not a generic settings app, and not an in-game fantasy interface. It is **history-first**. There is no fixed number of save slots, so the main view is a timeline listing every event, newest first.


## PRINCIPLES

These decide the cases this document doesn't cover:

- **Controls report their own results.** The button that started an action shows its progress and its outcome. Don't add toasts, results panels or success dialogs. Why: feedback shows up where the user is already looking, and there is nothing to dismiss.
- **Only show what applies right now.** No empty groups, no headings when there is only one group, no controls without a target. Why: a list of games that repeats "INSTALLED" on every item, or hotkeys when there are no games, is noise.
- **Stable layout.** Controls stay in fixed places. Live updates (new history rows, ticking relative times, midnight regrouping) never move the user's scroll position or resize rows. Why: the user is often mid-game and glances at the window. Things that jump around look like errors.
- **Keep text short.** Use short state labels (`Running`, `Scanning…`, `No new games`), not sentences explaining what a button obviously does.
- **Running games get priority, but never take the view from the user.** Only an actual switch to a game's window changes what the app shows (see ACTIVE STACK).
- **Few dialogs.** Common actions happen in place. Only configuration, adding a game and Flush get dialogs.
- **Plan for thousands of history entries,** not a demo with five. Use virtualized rendering and compact rows. Don't use save cards.

Visual emphasis, from strongest to weakest:

1. Save
2. Load
3. The selected game's identity
4. Checkpoint, load and revert rows
5. Game started/closed rows
6. Library controls (Scan, Add custom game)
7. Preferences in the bottom bar

Settings should never draw the eye away from the checkpoint workflow.

To make it feel like a game, use presentation: dark layered surfaces, strong type, crisp icons, an accent color for checkpoints, timeline connectors, styled keycaps and satisfying button states. Avoid fake sci-fi panels, heavy neon, giant artwork, ornamental borders and role-playing words that hide ordinary actions. "Save", "Load" and "History" already sound like games.


## LAYOUTS

The layout depends on **visible games**, not on database records. Games confirmed uninstalled are hidden, and hidden records don't count. The window has three states, each growing out of the previous one.

### 1. No visible games

The main area takes the full width and there is no sidebar. It shows one compact, centered block:

```text
┌───────────────────────────────────────────────────────────────────────────────────────────┐
│ SaveScummer                                                                       ─  □  × │
├───────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                           │
│                                                                                           │
│                                                                                           │
│                                             ◇                                             │
│                                                                                           │
│                                 No supported games found                                  │
│                                                                                           │
│                                     [ ⟳ Scan games ]                                      │
│                                     + Add custom game                                     │
│                                                                                           │
│                                                                                           │
│                                                                                           │
├───────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                       ☑ Launch on startup │
└───────────────────────────────────────────────────────────────────────────────────────────┘
```

There's no explanatory paragraph and no illustration; the two controls are the explanation. The bottom bar contains only `☑ Launch on startup`, in the same right-aligned position it has in the other states.

### 2. Games visible, none running

When the first game becomes visible:

- Scan and Add custom game move from the center to the bottom of the sidebar.
- The sidebar slides in from the left (see MOTION).
- The bottom bar grows to its full set of controls.

Nothing is preselected. The sidebar is a flat list, and the main area stays quiet:

```text
┌───────────────────────────────────────────────────────────────────────────────────────────┐
│ SaveScummer                                                                       ─  □  × │
├────────────────────────┬──────────────────────────────────────────────────────────────────┤
│   ▣ XCOM 2             │                                                                  │
│   ▣ Noita              │                                                                  │
│   ▣ Battle Brothers    │                                                                  │
│   ▣ Void War           │                                                                  │
│                        │                                                                  │
│                        │                                                                  │
│                        │                   No known games are running.                    │
│                        │                                                                  │
│                        │                                                                  │
│                        │                                                                  │
│                        │                                                                  │
│                        │                                                                  │
│                        │                                                                  │
│                        │                                                                  │
│ + Add custom game      │                                                                  │
│ ⟳ Scan games           │                                                                  │
├────────────────────────┴──────────────────────────────────────────────────────────────────┤
│ Hotkeys │ [Ctrl+F5] Save │ [Ctrl+F9] Load     ☑ Play sounds           ☑ Launch on startup │
└───────────────────────────────────────────────────────────────────────────────────────────┘
```

### 3. A game is running

The running game gets its own group at the top of the sidebar. It is selected when nothing else is, or when the user switches to its window (see ACTIVE STACK). The main area shows its header, actions and history:

```text
┌───────────────────────────────────────────────────────────────────────────────────────────┐
│ SaveScummer                                                                       ─  □  × │
├────────────────────────┬──────────────────────────────────────────────────────────────────┤
│ RUNNING                │ ▣ VOID WAR                                                       │
│ ▌▣ Void War            │   Running                                                        │
│                        │                                                                  │
│ INSTALLED              │ [ ◆ SAVE ]  [ ↶ LOAD          ]  [ ··· ]                         │
│   ▣ XCOM 2             │             [  3 seconds ago  ]                                  │
│   ▣ Noita              │                                                                  │
│   ▣ Battle Brothers    ├──────────────────────────────────────────────────────────────────┤
│                        │ HISTORY                                                          │
│                        │                                                                  │
│                        │ TODAY                                                            │
│                        │ 3 seconds ago              ◆ Saved                       [↶] [✕] │
│                        │ 12:24:03                     Before entering the station         │
│                        │ 1 hour and 12 minutes ago  ↶ Loaded · 10:47:10           [↶] [✕] │
│                        │ 11:18:44                                                         │
│                        │ 2 hours ago                ● Game started                        │
│                        │ 10:04:12                                                         │
│                        │ YESTERDAY                                                        │
│                        │ Yesterday                  ◆ Saved                       [↶] [✕] │
│                        │ 23:20:12                     Add name…                           │
│                        │ Yesterday                  ■ Game closed                         │
│ + Add custom game      │ 22:58:40                                                         │
│ ⟳ Scan games           │                                                                  │
├────────────────────────┴──────────────────────────────────────────────────────────────────┤
│ Hotkeys │ [Ctrl+F5] Save │ [Ctrl+F9] Load     ☑ Play sounds           ☑ Launch on startup │
└───────────────────────────────────────────────────────────────────────────────────────────┘
```

`✕` stands for the trash icon. The error and instructions blocks, when present, sit between the action row and HISTORY.

## ACTIVE STACK, SELECTION AND FOCUS

The ACTIVE STACK holds the running games, ordered by when the user last switched to each game's window outside the app. Its top game is the **active game**: the first row under `RUNNING` and the target of global hotkeys while the app is unfocused. The game **selected** in the main view can be different, because the user may be browsing another game.

The key rule: **only an external focus change moves a game up the stack and selects it.** The user has necessarily left the app to switch to the game, so the view never changes while they're using it. Starting or closing a game in the background moves it between groups but never takes over the view. A newly started game appears under `RUNNING` but doesn't jump ahead of games with a more recent focus.

The main view changes automatically only when:

- an external focus change moves a running game to the top;
- the displayed game closes and something else has to be shown;
- nothing is selected yet.

Consequences:

- **At startup,** select the most recently focused running game if the order is known, otherwise the top running game. If no game is running, select nothing. Don't preselect a game just because it's installed. The main view then shows only the quiet line `No known games are running.`
- **When the active game closes,** the next game in the stack becomes active. If the closed game was on screen, the new active game replaces it. If no running games remain, show `No known games are running.` A manual selection of a different game is always kept.
- **When the user picks a game in the sidebar** (running or not), only the view changes, not the stack.


## SIDEBAR

The sidebar is the game library and the main way to navigate. It is narrow and dense, and each row shows only an icon and a name. Don't add counts, timestamps or descriptions, because the sidebar has to stay usable with a large library. The selected row gets a compact but unmistakable highlight, such as an accent stripe, a slightly different background or brighter text. Don't use cards.

**Icons** are the game's cached Steam artwork, supplied by the host. While an icon is missing or loading, a neutral placeholder with the game's initials takes its place. The UI never downloads artwork itself. The app's own icon (`assets/icon.svg`) stays the window, taskbar and tray icon.

**Groups:**

- Show `RUNNING` and `INSTALLED` headings only when both groups are non-empty, with `RUNNING` on top.
- Otherwise, show a flat list with no heading.
- There is no overall `GAMES` heading. The sidebar is obviously a game list, so it would only take space.
- Never show an empty group.

**Visibility:**

- Hide confirmed-uninstalled games, whether they are known games or custom games. A custom game whose executable disappears is hidden until it comes back.
- Keep installed games that haven't produced save data yet.

**Library controls** stay anchored at the bottom of the sidebar:

- **`+ Add custom game`** is for games that aren't recognized automatically or that need custom paths. Use this exact wording; "Add game" would suggest it's the normal way to add games. Custom games are kept forever, and the main window has no way to remove them. A future management screen may add that.
- **`⟳ Scan games`** adds discovered games immediately. There's no results screen and no confirmation. The button itself cycles through these states:

  ```text
  ⟳ Scan games → ◌ Scanning… → ✓ 3 games found | No new games → ⟳ Scan games
  ```

  The count includes only *newly* found known games, with the singular form for one game. Scan and Add custom game behave the same in the zero-games layout, so users learn them once.


## GAME HEADER AND ACTIONS

```text
▣ VOID WAR
  Running
[ ◆ SAVE ]  [ ↶ LOAD                     ]  [ ··· ]
                         [ 3 seconds ago ]
```

- The icon and name come first. The status sits directly under the name and is only ever `Running` or `Stopped`. There are no badges and no separate readiness indicator.
- **The host decides whether each action is available, separately from the status.** When there is no game data to copy, Save is disabled and its tooltip and accessible name say `No game data yet`. Nothing else signals this.
- The order is fixed: Save, then Load, then `···`, which is always last.
- Directly below the row sits the **error block**, then the **instructions block**. The error block stays until the next action or game selection and never becomes a modal dialog.

### Save

This is the main action and has the strongest emphasis. It supports the core loop: reach an important moment, save, keep playing. The new entry goes to the top of the history.

### Load

The main Load button restores the **latest retained checkpoint**. Checkpoints imported from backups that already existed are treated like any other checkpoint and are never labeled differently in this window.

What Load will restore is shown *inside the button*, on a smaller, quieter second line: `3 seconds ago`, `Yesterday`, `2012-12-12`. This line uses the same age wording as the history, updates live, and replaces any separate "Last save" line. With no checkpoints, the button is disabled and its second line reads `No saves yet`.

A pending deletion doesn't change Load. It still targets the latest checkpoint, even if that checkpoint is counting down to deletion, and loading doesn't cancel the deletion. The host runs the two one after the other, and the latest checkpoint is recalculated only after the deletion succeeds.

Every successful Load also creates an **undo checkpoint** holding the state just before the load. That is what a loaded row's Revert restores. Load errors go to the error block.

### `···` menu

Each command has its own icon:

- **Open in File Explorer** opens the original save-data location. If the save source is a file, it opens the folder containing it; if the source is a directory, it opens that directory.
- **Configure paths…**
- **Flush checkpoints…**, the name used for this action everywhere.

There is no global Settings screen or gear icon. The few app-wide preferences are in the bottom bar.

### Busy state

While a save, load, revert, deletion or Flush is running for a game, every control that could start another operation *for that game* is disabled: Save, Load, the history row actions and the `···` commands. The sidebar and other games stay fully usable. A deletion countdown that is still pending does not make the game busy, and its Cancel stays usable even while another operation runs.

The control that started a Save, Load or Revert shows a spinner, whether it's the main button or a history row button. There is no progress bar, no "Saving…" caption and no percentage. On success, that control briefly shows a success state and then returns to normal. There is no success dialog.


## INSTRUCTIONS BLOCK

A game's catalog instructions (`info`, read-only) appear between the error block and the history. The block is left out entirely when the game has none.

- **Collapsed:** about 100 px tall, fading to transparent at the bottom.
- **Expanded:** grows in place to show the full text, with compact paragraph and list spacing.
- A dedicated chevron at the edge switches between the two. Clicks inside the text never toggle the block, so that selecting and copying text works normally. The choice is remembered per game for the session.
- The Save and Load procedures are numbered separately, each starting at 1.


## HISTORY

The history is a vertical, newest-first, effectively unlimited activity log. It should read as a run log, not as save slots or cards. A subtle vertical line may connect the events.

**Empty history** shows one quiet line: `Saves will appear here.`

### Rows

Each row has:

1. **A time column** with two lines. The first line is relative: `4 seconds ago` and `1 hour and 12 minutes ago` for today, then `Yesterday`, the full localized weekday for recent days, and `yyyy-MM-dd` for older dates. The second line is always the exact local `HH:mm:ss` and never changes.
2. **An event icon and description.** The icon supplements the text and never replaces it.
3. **Actions** at the end of the row, when the row has any.

Details that are easy to get wrong:

- The column must fit the longest relative label or weekday name. It never abbreviates or truncates.
- Rows are grouped under lightweight day headings, and the time column lines up exactly with the heading's left edge.
- Relative labels update live, in place, without reordering rows. At local midnight, the day groups are recalculated so today's rows become yesterday's, and the scroll position is kept.
- New entries go on top. If the user has scrolled down, keep their position; never pull them back to the top.

### Event kinds

| Icon | Event | Weight | Actions |
| --- | --- | --- | --- |
| ◆ | Saved | Significant | Load this save, Delete |
| ↶ | Loaded · <time of the save it loaded> | Significant | Revert this load, Delete |
| ↷ | Load reverted | Significant | None |
| ● | Game started | Light | None |
| ■ | Game closed | Light | None |

The glyphs above are placeholders, but each event kind needs a stable icon that is easy to tell apart. Started and closed rows make play sessions visible without a separate session UI.

- **Saved** can carry a caption (see below). Imported backups use this same row with no special marking.
- **Load this save** loads *that* save instead of the latest one. It uses the same button style and icon family as Revert.
- **Revert this load** restores that load's undo checkpoint, which is the state just before the load. The result is a `Load reverted` row, which only records what happened: it has no actions and doesn't create another undo point. After a successful revert, the original loaded row loses its Revert button.
- Revert changes game data; Delete destroys a checkpoint, so they must not get equal emphasis. Delete stays quiet until the row is hovered, focused or selected.

### Save captions

A save's caption goes on the row's second line, under `Saved`, for example `Before boss fight`. It is edited right there in the row. Why: the user names a save while it's fresh, without opening a dialog, and the history reads like a log of decisions instead of a column of identical `Saved` rows.

A new save has no caption. Its second line shows a dimmed `Add name…` suggestion, which looks like a button when hovered:

```text
3 seconds ago   ◆ Saved                                 [↶] [✕]
12:24:03          Add name…
```

Clicking the caption, or `Add name…`, turns that line into a text field with an inline **Save** button on the right:

```text
3 seconds ago   ◆ Saved                                 [↶] [✕]
12:24:03          [Before boss fight_          ] [Save]
```

Changes save automatically, so the user never has to remember to confirm. A save happens on any of these:

- a short pause after typing, restarted by every keypress;
- the field losing focus;
- pressing the inline Save button.

Clearing the text removes the caption, and the line goes back to `Add name…`. A caption is only a label: editing it changes no game files and adds no history event.

### Deleting a single entry

Delete is deliberately not immediate. Deleting a saved row removes the checkpoint. Deleting a loaded row removes its undo state, and then the whole row. Pressing the trash button turns the row into a countdown:

```text
Deleting in 5 [Cancel]      (5 → 4 → 3 → 2 → 1, updated in place)
```

Why a countdown instead of a dialog: deleting a checkpoint should be quick, and a mistake needs to be undoable. A confirmation dialog repeated on every row gets clicked through without reading.

The host owns the countdown. The UI only sends delete and cancel requests and displays the host's state, and its local timer never triggers anything. This is a small in-memory collection in the host, not a job framework or a persistent queue. The core delete operation knows nothing about countdowns.

- **Countdowns are independent.** Each has its own 5-second deadline and Cancel. The same entry never gets a second deletion request.
- **Operations for the same game run one at a time.** When a countdown expires, the deletion runs in request order through the same per-game coordination as Save, Load, Revert and Flush. A countdown doesn't reserve the game's turn. Save and Load are still rejected while the game is busy; clicks and hotkey presses are never stored to replay later.
- **Row states:**
    - `Deleting in N [Cancel]` while counting down.
    - `Waiting to delete… [Cancel]` if the game is busy when the countdown ends.
    - `Deleting…` with a spinner once the deletion runs, with no Cancel.
- **Cancel versus run is decided by the host, one request at a time.** A cancel the host accepts guarantees nothing is deleted. A cancel that arrives too late shows the real state instead of pretending to restore the row.
- **Outcome:** the row is removed only after the host confirms the files are deleted from disk. If the deletion fails, the normal row comes back and the error block shows the failure. There's no automatic retry; the user can press Delete again. Other deletions continue either way.
- **Leaving doesn't cancel anything.** Switching games, scrolling away, hiding the window or disconnecting the UI leaves accepted deletions running. When the user returns, the rows are rebuilt from the host's state.
- **If Flush removes an entry** that is waiting to be deleted, that deletion is dropped quietly, with no second attempt and no "not found" error.

There is no queue screen, no batch confirmation and no extra history event for deletions.


## BOTTOM BAR

```text
Hotkeys │ [Ctrl+F5] Save │ [Ctrl+F9] Load      ☑ Play sounds          ☑ Launch on startup
```

The bar should look like a compact game status strip, not a settings form. The shortcuts are styled as keycaps and stand out slightly more than their `Save`/`Load` labels. `Play sounds` sits next to the hotkeys. `Launch on startup` sits at the far right, apart from the others, because it controls how the app starts, not Save and Load.

**Which game the hotkeys act on:**

- **When the app is focused,** they act on the *selected* game, even a stopped one. Why: when the user is working inside the app, they mean the game they're looking at.
- **When the app is unfocused,** they act on the active game, the top of the ACTIVE STACK.
- **With no target** (the app is focused with nothing selected, or it's unfocused with no game running), the hotkeys are shown as unavailable.
- While the target game is busy, conflicting operations are unavailable. A pending deletion countdown alone changes neither whether the hotkeys work nor which game they target.


## DIALOGS

There are only three dialogs. All of them:

- size to their content and can't be resized;
- use text-only buttons;
- rely on the platform's standard behavior for focus, keyboard navigation, the default button and cancelling. Don't write custom Enter or focus handling; a deliberately focused button must still activate normally.

Deleting a single checkpoint is not a dialog; it uses the row countdown above.

### Add custom game

```text
Game executable: [...........................] [Browse…]
Save location:   [...........................] [Browse…]
Name:            [...........................]
                                        [Add] [Cancel]
```

- The fields stay in this order. The name must be non-blank after trimming, and both paths must be non-blank absolute paths.
- Browse opens a file picker for the executable and a folder picker for the save location. The fields stay editable, so the user can type a path that doesn't exist yet, such as a save folder the game hasn't created.
- When the user picks an executable with Browse and Name is blank, Name is filled with the file name minus its final extension. A name that's already filled in is never overwritten.
- Errors appear in the dialog and the entered values stay. Nothing partial is created. The host generates the stable ID.
- **Add** is the default button.

### Configure paths

```text
Game executable:     [...prefilled path...] [open] [Reset]
Game data dir (DIR): [...prefilled path...] [open] [Reset]
                                           [Save] [Cancel]
```

- For custom games, Name is editable, the DIR label reads "Save location" (as in the Add dialog) and there's no Reset.
- Changes are applied only after the core validates them. If validation fails, the saved configuration doesn't change and the entered values stay for the user to fix.
- **Save** is the default button.

### Flush checkpoints

This is the only bulk delete, so it is built to prevent mistakes:

- **When it's available:** there are saved checkpoints, recovery checkpoints or history entries, and the game is neither busy nor waiting for recovery. Recovery data left by an interrupted operation is included only after that interruption is resolved.
- **Text:** "Permanently delete all backups and clear this game's history?" followed by "Your current game data will be kept."
- **Counts:** separate counts for saved backups, recovery points and incomplete copies. Their paths go under **Details**, loaded in pages that belong to this particular confirmation.
- **The confirmation must match the disk.** Before showing it, rescan the backups. If a backup is deleted, replaced, changed or newly found outside the app, the confirmation becomes invalid and a fresh one is required. Why: the user must never delete something they weren't shown.
- **On confirm:** delete every saved and recovery checkpoint, imported ones included, and clear the history. DIR is not touched. Only the records whose files were actually deleted are cleared, and any failures are reported.
- **Cancel** is the default button.


## MOTION

Motion clarifies structural changes and confirms meaningful actions. It never carries information on its own.

- Animations run only when the system hasn't asked for reduced motion (the equivalent of `prefers-reduced-motion: no-preference`). Otherwise, changes happen instantly.
- When the zero-games layout changes to the sidebar layout, the sidebar slides in from the left and the main area settles into its new width, briefly and without drama. With reduced motion, the sidebar simply appears.
- Short success flashes on buttons are fine, but the success state must be clear without them.
- No looping or decorative motion, and nothing that competes with the history.


## WINDOW AND TRAY

Closing the window hides the app to the tray instead of quitting. Game detection, global hotkeys and pending deletions keep running, and hiding the window doesn't shorten any countdown.

Deletions on shutdown:

- **The UI closing or disconnecting** never waits for deletions. The host keeps processing them. A failure doesn't block closing, doesn't reopen the window and isn't retried; the entry stays in the history for the user to retry.
- **A normal host shutdown** ends the remaining countdowns early and runs the accepted deletions through the usual per-game coordination before stopping, without keeping the UI open. Failures don't block shutdown.
- **After a crash or forced kill,** deletions that hadn't started are dropped, since countdowns aren't saved to disk. Deletions that had started follow the core rules for interrupted operations.

The tray menu and the full-exit flow are outside the scope of this document.

The window can be resized down to a minimum size. At that size:

- the sidebar is still usable;
- the Save, Load and `···` row still holds together;
- the time column doesn't truncate;
- the row actions are reachable;
- the bottom bar doesn't overlap itself.

There is no separate narrow layout.

Accessibility: icon-only controls (`···`, the history row Load, Revert and Delete buttons) need accessible names and tooltips. Keyboard focus follows visual order, and destructive actions stay reachable by keyboard.


## TESTING

UI tests run against a fake service that can simulate being busy, failing, missing snapshots, pending deletions and a disconnected host. They focus on the rules that are easy to break:

- **Sidebar:**
    - headings only when both groups are non-empty;
    - hidden uninstalled games;
    - the switch to the zero-games layout based on *visible* games.
- **Selection:**
    - external focus moves a game up and selects it;
    - a manual selection is kept when games start or close;
    - the fallback when the active game closes.
- **Scan:** the zero, singular and plural messages, counting only newly found known games.
- **Status and actions:** `Running`/`Stopped` shown separately from host-provided availability (no game data, no checkpoints). Imported backups count as ordinary checkpoints for Load.
- **History rows:** spinner and success states on the Load and Revert buttons, and `Load reverted` rows with no actions.
- **Captions:** the `Add name…` placeholder; the autosave pause restarting on every keypress; saving on focus loss and on the Save button; clearing a caption.
- **Deletion countdowns:**
    - they run independently;
    - Cancel works, including while the game is busy;
    - the waiting and deleting states;
    - the main Load button doesn't change;
    - rows are rebuilt after navigating away or reconnecting;
    - a failure restores the row with no retry.
- **Midnight:** rows regroup without moving the scroll position.
- **Dialogs:** standard keyboard, focus, default-button and cancel behavior; the custom-game dialog's validation and name autofill.
- **Other blocks:** collapsed and expanded instructions, including games with none, and the error block's content.
- **Accessibility:** accessible names and keyboard access for icon-only and destructive controls.

Host coordination tests use a controllable clock and cover:

- cancel versus run;
- operations for the same game running one at a time;
- independent countdowns;
- continuing after a failed deletion;
- Flush dropping deletions it made unnecessary;
- closing the UI without interrupting accepted deletions;
- a normal shutdown finishing accepted deletions without reopening the UI or retrying failures.
