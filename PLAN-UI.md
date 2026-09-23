# SaveScummer UI

This document is the authoritative plan for the main window UI. It replaces the UI
section that previously lived in `PLAN.md`.

- Core application behavior: [`PLAN.md`](PLAN.md).
- Failure, interruption and notice scenarios: [`PLAN-ERRORS.md`](PLAN-ERRORS.md).
- Integration test intent: [`PLAN-INTEGRATION-TESTS.md`](PLAN-INTEGRATION-TESTS.md).

---

## 1. Overall design goals

SaveScummer should feel like a **game-oriented desktop utility**, not a generic settings app and not an in-game fantasy interface.

The main preferences are:

- **Fast to understand.** The active game, its running state, and the Save/Load workflow should be obvious immediately.
- **History-first.** Checkpoints are unlimited, so the UI is built around a vertical newest-first chronological history rather than save slots.
- **Dense but not cramped.** It should scale to many games and thousands of history entries.
- **Game-like through presentation, not gimmicks.** Typography, icons, contrast, timeline treatment, and restrained motion can make it feel game-oriented without hurting usability.
- **Very little explanatory copy.** Prefer clear controls and state over instructions.
- **Conditional UI.** Do not show empty categories, irrelevant controls, or placeholder structure merely for consistency.
- **Stable layouts.** Controls should stay in predictable places and should not jump around unnecessarily as their state changes.
- **Minimal modal behavior.** Common actions should happen in place.
- **Action feedback stays local.** Whenever practical, the control that initiates an action should also communicate its temporary result.
- **Running games get priority.** A running game should naturally become the focus of the application.

The application has three important states:

1. **No games configured**
2. **Games configured, but no known game is running**
3. **At least one known game is running**

The UI should transition naturally between these states without feeling like separate applications.

---

## 2. Normal operational layout

When at least one game is configured, the application uses three major regions:

1. **Game sidebar** on the left
2. **Selected game / main content** on the right
3. **Persistent bottom control bar**

The main content is vertically split into:

- a compact game header with primary actions
- a large scrollable history feed below

Example when both running and installed games exist:

```text
┌──────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│  SaveScummer                                                                                           ─ □ × │
├──────────────────────────────┬───────────────────────────────────────────────────────────────────────────────┤
│                              │                                                                               │
│  GAMES                       │  ▣ VOID WAR                                                                   │
│                              │    Running                                                                    │
│  RUNNING                     │                                                                               │
│  ──────────────────────────  │  ┌───────────────────────────┐  ┌───────────────────────────┐  ┌───────┐     │
│                              │  │ ◆  CREATE CHECKPOINT      │  │ ↶  LOAD                   │  │  ···  │     │
│  ▌ ▣ Void War                │  │                           │  │    Checkpoint · 3 seconds ago  │  │       │     │
│                              │  └───────────────────────────┘  └───────────────────────────┘  └───────┘     │
│                              │                                                                               │
│  INSTALLED                   ├───────────────────────────────────────────────────────────────────────────────┤
│  ──────────────────────────  │                                                                               │
│                              │  HISTORY                                                                      │
│    ▣ XCOM 2                  │                                                                               │
│                              │  TODAY                                                                        │
│    ▣ Noita                   │                                                                               │
│                              │  3 seconds ago          ◆  Checkpoint saved                         [↶] [🗑] │
│    ▣ Battle Brothers         │  12:24:03                  Before entering the station                        │
│                              │                                                                               │
│    ▣ Darkest Dungeon         │  1 hour and 12 minutes ago ↶  Checkpoint loaded                       [↶] [🗑] │
│                              │  11:18:44                    From checkpoint · 10:47:10                        │
│                              │                                                                               │
│                              │  2 hours ago            ●  Game started                                      │
│                              │  10:04:12                                                                     │
│                              │                                                                               │
│                              │  YESTERDAY                                                                    │
│                              │                                                                               │
│                              │  Yesterday              ■  Game closed                                       │
│                              │  23:20:12                                                                     │
│                              │                                                                               │
│                              │  WEDNESDAY                                                                    │
│                              │                                                                               │
│                              │  Wednesday              ◆  Checkpoint saved                         [↶] [🗑] │
│                              │  12:23:22                  Before boss fight                                  │
│                              │                                                                               │
│                              │  ⋮                                                                            │
│                              │                                                                               │
│  +  Add custom game          │                                                                               │
│  ⟳  Scan games               │                                                                               │
├──────────────────────────────┴───────────────────────────────────────────────────────────────────────────────┤
│  Hotkeys  │  [Ctrl+F5] Save  │  [Ctrl+F9] Load        ☑ Play sounds                         ☑ Launch on startup │
└──────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

The exact visual treatment may vary, but the structural relationships should remain stable. In the mockups, `▣` marks the game's icon.

---

## 3. Sidebar

The sidebar is the game library and primary navigation.

It should remain relatively narrow and dense.

### Header

```text
GAMES
```

This is a small section heading, not a prominent page title.

### Game entries

Each game entry shows the game's icon followed by its name.

Icons use the game's cached Steam artwork, supplied by the host; while an icon is missing or loading, a neutral initials placeholder takes its place. The UI never downloads artwork itself. The application icon (`assets/icon.svg`) remains the window, taskbar and tray icon; game icons never replace it.

Avoid filling the sidebar with checkpoint counts, timestamps, descriptions, or other secondary metadata. The sidebar needs to remain usable with a large library.

The selected game gets a compact but unmistakable highlight, for example:

- accent stripe
- slightly different background
- brighter text

Avoid large cards.

---

## 4. Running and Installed categories

The sidebar has two possible game groups:

- `RUNNING`
- `INSTALLED`

However, the category headings should only be shown when they actually help distinguish two non-empty groups.

### Both groups contain games

If at least one game is running and at least one other configured game is not running, show both categories:

```text
RUNNING
────────────────────
▌ ▣ Void War

INSTALLED
────────────────────
  ▣ XCOM 2
  ▣ Noita
```

`RUNNING` is always above `INSTALLED`.

### Only installed games exist

If no known game is running, do **not** show either category heading. Show a flat game list:

```text
GAMES

  ▣ XCOM 2
  ▣ Noita
  ▣ Battle Brothers
```

Games that are confirmed uninstalled are not shown in the sidebar at all — neither known
nor custom. A custom game whose executable disappears is hidden until it is installed
again. Games that are installed but have not produced save data yet stay listed and
carry the `Not run yet` status.

There is no reason to label every item `INSTALLED` when that is the only possible group visible.

### Only running games exist

If every configured game currently belongs to the running group, likewise do not show empty or redundant category headings. Show the running games directly beneath `GAMES`.

### Category transition

Suppose several games are configured and none are running. The sidebar is initially flat.

If one of those games starts, the sidebar reorganizes into:

```text
RUNNING
────────────────────
▌ ▣ Newly running game

INSTALLED
────────────────────
  ▣ Remaining game
  ▣ Remaining game
```

The running group appears at the top.

The UI should never render an empty `RUNNING` or `INSTALLED` section.

---

## 5. Selection, active stack, and automatic focus

SaveScummer maintains an **active stack of running games**, ordered by most recent game-window focus.

The top of that stack is the active running game. It is shown at the top of the `RUNNING` group and is automatically selected in the main view.

### Application startup

If one or more known games are already running when SaveScummer starts, a running game should be selected automatically.

When a meaningful existing focus order is available, the most recently focused running game is selected. Otherwise the top running game becomes the initial active game.

### Game focus changes

When the user focuses a different known running game outside SaveScummer, that game is pushed to the top of the active stack.

The sidebar should then:

1. move that game to the top of the running list
2. select it
3. show its details in the main view

This selection change follows an actual external game-window focus change. The user has necessarily left SaveScummer to focus the game, so the main window does not unexpectedly switch while the user is interacting with it.

### Background lifecycle events do not steal the view

A game starting or closing in the background is not by itself a reason to replace what the user is looking at. The main view changes automatically only when:

- an external focus change promotes a running game (above),
- the game currently displayed in the main view closes, or
- nothing is selected yet.

Otherwise the sidebar, the running group and the active stack update, but the displayed game stays selected.

### A game starts while no known game is running

If no known game is currently running and a configured game starts, that game becomes the active game and moves to the running position in the sidebar. It is selected automatically only when nothing is currently selected.

### Active game closes

If the active running game closes, the next-most-recently-focused running game in the active stack becomes active automatically.

That game moves to the top of the running list. If the closed game was the one displayed in the main view, the newly active game becomes selected and its details replace the previous content. A manual selection of a different game is preserved.

If no known running games remain, the main pane returns to:

```text
No known games are running.
```

### Manual selection

Users can manually select any configured game from the sidebar, including a game that is not currently running.

Manual selection changes what is displayed in the main content, but it does **not** redefine the active running-game stack used by global hotkeys.

A later external focus change to a running game promotes that game to the top of the active stack and selects it automatically.

---

## 6. Configured games, but nothing is running

If games are configured but none are running, the sidebar remains visible so the user can browse and select them.

However, **no game should be preselected by default solely because it is installed**.

The default main content should instead show a lightweight neutral state such as:

```text
No known games are running.
```

This should be visually quiet and should not become another onboarding screen.

The user can still select any game in the sidebar to inspect its details and history.

If a configured game subsequently starts, it becomes selected automatically.

---

## 7. Sidebar game-management controls

At the bottom of the sidebar:

```text
+ Add custom game
⟳ Scan games
```

These controls remain anchored near the bottom rather than following the list contents.

### Add custom game

`Add custom game` opens the manual configuration flow.

This is the route for games that:

- are not recognized automatically
- need custom save locations
- need custom executable/process configuration
- otherwise require manual setup

The wording should remain explicit. `Add custom game` is preferable to simply `Add game`.

Custom games are stored indefinitely. There is no removal action in the main window; a
future management screen may add one.

### Scan games

`Scan games` automatically discovers supported games and **adds them immediately**.

There is no scan-results screen and no confirmation list.

The button itself communicates temporary status:

```text
⟳ Scan games
```

becomes:

```text
◌ Scanning…
```

and then briefly:

```text
✓ 3 games found
```

or:

```text
No new games
```

before returning to:

```text
⟳ Scan games
```

Any newly found games simply appear in the sidebar.

This is an important UI preference: **feedback should occur in the control that initiated the action rather than creating another panel.**

---

## 8. Selected game header and action area

The main pane starts with the selected game's name and current state.

```text
▣ VOID WAR
  Running
```

or:

```text
▣ VOID WAR
  Stopped
```

The status goes **directly underneath the game name**.

Avoid badges or extra status cards unless a visual theme strongly benefits from them.

A game's status is one of `Running`, `Stopped`, or `Not run yet` (installed, but no save data has appeared yet). Confirmed-uninstalled games are not shown in the sidebar at all (section 4).

The game's icon precedes its name in the header, matching the sidebar entry (section 3).

Below it is one action row:

```text
[ ◆ CREATE CHECKPOINT ]   [ ↶ LOAD                         ]   [ ··· ]
                          [   Checkpoint · 3 seconds ago   ]
```

The order is fixed:

1. Create Checkpoint
2. Load
3. More (`…`)

The `…` action is always last.

Directly below the action row, in order:

1. A compact error block, when the game has an active error. It stays pinned there until the next action or game selection and never opens a generic modal dialog. Its contents and button set are specified in [`PLAN-ERRORS.md`](PLAN-ERRORS.md).
2. The game's instructions block, when the game has instructions (section 37).

While a save or restore is in progress for the displayed game, every main-content control that could start another operation for that game is disabled: Create Checkpoint, Load, the history row actions and the game's More-menu commands. The sidebar and other games remain fully usable. The control that started the operation shows a spinner; no progress bar and no status caption are added.

---

## 9. Create Checkpoint

This is the main action.

```text
◆ CREATE CHECKPOINT
```

It should normally have the strongest emphasis in the action row.

This reflects the common workflow:

> reach an important moment → create checkpoint → continue playing

Creating the checkpoint produces a new history entry.

### In-progress state

Saving can take noticeable time. While it is in progress:

- the Save/Create Checkpoint control shows a spinner on the button
- Load and the other main-content operation controls for this game are disabled
- repeated activation cannot start another Save operation

### Success state

On success:

- the new saved-checkpoint history entry is added at the top of the history
- the initiating control briefly shows a success icon/state
- no success dialog is shown
- the control then returns to its normal appearance

If the history is currently scrolled away from the top, adding the entry must not change the user's scroll position.

---

## 10. Load

The main Load button restores the latest saved checkpoint.

Its context belongs **inside the button itself**:

```text
↶ LOAD
  Checkpoint · 3 seconds ago
```

The secondary line uses the same concise age/date language as history timestamps and updates over time. Examples include:

```text
Checkpoint · 3 seconds ago
Checkpoint · 2 minutes ago
Checkpoint · 1 hour and 12 minutes ago
Checkpoint · Yesterday
Checkpoint · Wednesday
Checkpoint · 2012-12-12
```

There should not be a separate `Last checkpoint` line elsewhere in the header.

The secondary line should be smaller and visually quieter.

### Disabled state

If no saved checkpoint exists, Load is disabled and its secondary line reads `No checkpoints saved`.

While either Save or Load is in progress, the main-content operation controls for this game are disabled (section 8).

### Successful load

A successful Load:

- creates a history entry at the top
- creates an immediate undo checkpoint representing the state immediately before the Load
- briefly changes the Load button to a success icon/state
- then returns the button to its normal `LOAD` presentation
- does not show a success dialog

If the user is scrolled away from the top of history, the new history entry must not move their scroll position.

### Failure

A Load error is shown in the inline error area directly beneath the main action row.

---

## 11. More menu

The final action is:

```text
[…]
```

It opens a compact game-specific dropdown menu. The menu contains these commands, each with its own semantic icon:

```text
[folder]     Open in File Explorer
[cog]        Configure paths…
[trash can]  Flush checkpoints…
```

`Open in File Explorer` opens the relevant location directly.

Commands with trailing ellipses open separate dialogs. The contents, behavior, validation, and confirmation flows inside those dialogs are **out of scope for this main-window UI document**.

There is no global Settings screen or global gear icon.

Custom games are permanent hidden records; the main window exposes no way to remove one.

The application-level preferences that currently exist are exposed directly in the bottom bar.

---

## 12. History is the core of the main view

Below the header is:

```text
HISTORY
```

The history is:

- vertical
- **newest first**
- scrollable
- effectively unlimited
- designed for potentially thousands of events

SaveScummer should never visually suggest a finite number of checkpoint slots.

Conceptually, this is closer to an **activity timeline** than a traditional game save screen.

### Empty history

A configured game with no history yet should show a small quiet message such as:

```text
Saved checkpoints will appear here.
```

Do not fill the empty history area with onboarding copy or illustrations.

### Live updates

New entries are inserted at the top.

If the user is already at the top, the newest entry can appear naturally in view. If the user has scrolled down, preserve the current scroll position/anchor and do not jump them back to the newest item.

Relative timestamp labels update live as time passes.

---

## 13. History timeline and row structure

A subtle vertical rail can connect events.

Each row contains three conceptual parts:

1. a fixed-width two-line time column
2. a small semantic event icon plus description/content
3. contextual actions aligned at the end of the row when applicable

The history should read as a chronological run/activity log rather than a set of large cards.

The visual hierarchy should be:

**saved checkpoints, imported backups, loads, and reverts = significant state events**

**game started/closed = lightweight context**

Destructive controls should remain visually quiet until hover/focus/selection where practical.

---

## 14. Date groups and timestamp formatting

History uses lightweight day-group headings, and the time column of the rows beneath each heading must align exactly with the heading's left edge.

### Today's entries

For entries from today, the first line of the time column uses a concise relative age, using the same language as the secondary line beneath the main Load button.

Examples:

```text
4 seconds ago
2 minutes ago
1 hour and 12 minutes ago
```

The second line always shows the exact local time:

```text
HH:mm:ss
```

For example:

```text
4 seconds ago
12:23:22
```

### Older entries

For older entries, the first line uses one of:

```text
Yesterday
Wednesday
2012-12-12
```

Use:

- `Yesterday` for the previous calendar day
- the full localized weekday name for recent earlier days
- `yyyy-MM-dd` for older dates

The second line still shows exact local `HH:mm:ss` time.

Examples:

```text
Yesterday
23:20:12

Wednesday
12:23:22

2012-12-12
12:12:21
```

### Time-column sizing

The time column must be wide enough for the longest supported relative-age label or full localized weekday name.

Time text must never be abbreviated or ellipsized merely to fit the column.

### Live relative time

Relative labels should update automatically while the window is open so, for example, `59 seconds ago` eventually becomes `1 minute ago`. The exact `HH:mm:ss` line never changes.

---

## 15. Game started

Example row content:

```text
●  Game started
```

This is informational.

It has no action buttons and should be visually lightweight.

Its timestamp is supplied by the standard two-line time column described above.

---

## 16. Game closed

Example row content:

```text
■  Game closed
```

This is informational and has no actions.

Start and close events make play sessions naturally visible in the history without requiring a separate explicit session UI initially.

Its timestamp is supplied by the standard two-line time column described above.

---

## 17. Saved checkpoint

Example row content:

```text
◆  Checkpoint saved                                  [↶] [🗑]
   Before entering the station
```

Saved checkpoints have more visual weight than lifecycle events.

A checkpoint can optionally contain a user-facing label/note, for example:

```text
Before entering the station
Before boss fight
Before choosing faction
```

### Load an older checkpoint

Every retained saved-checkpoint row has a compact restore/load action.

It should use the same button treatment/icon family as the Revert action shown on loaded-checkpoint rows. Its tooltip/accessible label should make the row-specific meaning clear, for example `Load this checkpoint`.

Activating it loads that historical checkpoint rather than the latest checkpoint.

### Delete

Every retained saved-checkpoint row also has a trash action.

The trash icon should be visually restrained, particularly in long histories, and become clearer on hover/focus/selection.

Deleting is intentionally not immediate. Pressing trash transforms the row into an inline countdown state such as:

```text
Checkpoint will be deleted in 5…   [Cancel]
Checkpoint will be deleted in 4…   [Cancel]
Checkpoint will be deleted in 3…   [Cancel]
Checkpoint will be deleted in 2…   [Cancel]
Checkpoint will be deleted in 1…   [Cancel]
```

If `Cancel` is pressed, the normal checkpoint row is restored and nothing is deleted.

When the countdown completes, the checkpoint is deleted from disk and the entire history entry is removed. If the deletion fails (a lock, a disconnected drive), the sticky error block appears and the row returns.

---

## 18. Loaded checkpoint and Revert

Example row content:

```text
↶  Checkpoint loaded                                [↶] [🗑]
   From checkpoint · 11:47:10
```

Every successful Load creates an immediate undo checkpoint representing the state immediately before the Load.

A loaded-checkpoint history row therefore has two controls:

### Revert

Revert restores the immediate undo checkpoint and returns the game to the state that existed immediately before that Load.

The Revert control should use the same compact action-button treatment/icon family as the load action on saved-checkpoint rows. Its tooltip/accessible label should say what it does, for example `Revert this load`.

A successful revert is represented in history as its own semantic `Load reverted` event.

### Delete

The trash action removes the Load's retained undo state/history item using the same inline grace-period pattern as checkpoint deletion.

For example:

```text
Load will be deleted in 5…   [Cancel]
Load will be deleted in 4…   [Cancel]
Load will be deleted in 3…   [Cancel]
Load will be deleted in 2…   [Cancel]
Load will be deleted in 1…   [Cancel]
```

If canceled, the normal row returns unchanged.

After the countdown completes and the retained undo state is deleted from disk, the entire Load entry is removed from the history. If the deletion fails, the sticky error block appears and the row returns.

`Revert` is operational; `Delete` is destructive. They should not receive identical visual emphasis.

---

## 19. History event icon language

Every history description has a small semantic icon identifying its entry kind.

The required event kinds are:

```text
◆  Checkpoint saved
⇩  Imported backup
↶  Checkpoint loaded
↷  Load reverted
●  Game started
■  Game closed
```

The exact glyphs may change between visual themes, but each event kind must have a stable and clearly distinguishable semantic icon within a theme.

Icons supplement the text; they are not the only way the event kind is communicated.

---

## 20. History scalability and stability

The layout should be designed from the beginning for hundreds or thousands of entries.

Prefer:

- virtualized rendering
- lazy loading if needed
- compact entries
- no giant save cards
- minimal persistent action icons
- clear day separators
- a stable, non-truncating time column

Avoid designing around a demo history containing only five or ten events.

The feed is newest-first. New entries should not disturb the current scroll anchor when the user has scrolled away from the top.

Live timestamp updates should update text in place without reordering rows or causing visible layout jumps.

Possible future additions such as filtering/search are reasonable, but they should not be necessary to make the initial UI usable.

---

## 21. Bottom control bar when games exist

When at least one game is configured, the bottom bar spans the full application width:

```text
Hotkeys │ [Ctrl+F5] Save │ [Ctrl+F9] Load      ☑ Play sounds                    ☑ Launch on startup
```

This should look more like a compact desktop/game status strip than a settings form.

### Hotkeys

`Hotkeys` is a label.

The key combinations should visually resemble keyboard keycaps:

```text
[Ctrl+F5] Save
[Ctrl+F9] Load
```

The shortcut itself should be slightly more prominent than `Save` or `Load`.

While the SaveScummer window is focused, the hotkeys act on the selected game's Create Checkpoint and Load controls — including a stopped game.

When the window is not focused, they target the most recently focused running game, meaning the game at the top of the active running-game stack.

When the focused window has no game selected and no known game is running, the hotkeys have no target and are presented as unavailable.

While Save or Load is already in progress, the corresponding state-changing operations are unavailable until that operation finishes.

### Play sounds

```text
☑ Play sounds
```

This sits relatively near the hotkey controls.

### Launch on startup

```text
☑ Launch on startup
```

This sits toward the right edge.

That spatial separation helps distinguish an application behavior from immediate Save/Load controls.

---

## 22. Zero-games state

If SaveScummer has no configured games, **do not show an empty sidebar**.

The main content occupies the full width.

The empty state should be extremely minimal:

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ SaveScummer                                                             ─ □ ×│
│                                                                              │
│                                                                              │
│                              ◇                                               │
│                                                                              │
│                         No games found                                       │
│                                                                              │
│                         [ ⟳ SCAN GAMES ]                                     │
│                           + Add custom game                                  │
│                                                                              │
│                                                                              │
│                                                                              │
├──────────────────────────────────────────────────────────────────────────────┤
│                                                         ☑ Launch on startup  │
└──────────────────────────────────────────────────────────────────────────────┘
```

The preferred hierarchy is simply:

```text
small icon

No games found

[ Scan games ]
+ Add custom game
```

No explanatory paragraph is necessary.

The controls already explain the available paths.

---

## 23. Zero-games bottom bar

When there are no configured games, the Save/Load hotkeys and `Play sounds` control are not useful yet and should not be shown.

The bottom bar should contain only:

```text
☑ Launch on startup
```

This should remain aligned toward the right, in the same position it occupies once games exist.

As soon as at least one game becomes configured, the normal bottom-bar controls appear:

```text
Hotkeys │ [Ctrl+F5] Save │ [Ctrl+F9] Load      ☑ Play sounds                    ☑ Launch on startup
```

This follows the broader principle of not presenting controls before they have a meaningful target.

---

## 24. Zero-game composition

The onboarding block should be compact rather than spread vertically across the screen.

The entire unit can be centered in the available area, while its contents remain tightly grouped.

Avoid:

- multiple paragraphs
- large onboarding illustrations
- secondary help text
- repeated product descriptions
- instructions describing what `Scan games` obviously does

The empty state should feel calm and intentional.

---

## 25. Discovery controls should behave identically everywhere

The same two actions exist in both layouts:

```text
Scan games
Add custom game
```

### Zero games

They appear in the centered main-screen empty state.

### One or more games

They appear at the bottom of the sidebar.

Their behavior does not change.

This means users learn one interaction once.

---

## 26. Scan behavior in zero-game mode

Pressing:

```text
[ Scan games ]
```

temporarily changes the same button to:

```text
[ Scanning… ]
```

If games are found:

```text
[ ✓ 3 games found ]
```

appears briefly.

The discovered games are added immediately.

As soon as at least one game exists, the application transitions to the standard sidebar layout.

There is no intermediate results screen.

If nothing is found:

```text
[ No new games ]
```

appears briefly, then returns to:

```text
[ Scan games ]
```

Nothing else on the page needs to move.

---

## 27. Scan behavior in sidebar mode

Exactly the same interaction applies in sidebar mode.

```text
⟳ Scan games
```

becomes:

```text
◌ Scanning…
```

then either:

```text
✓ 2 games found
```

or:

```text
No new games
```

Any discovered games simply appear in the appropriate sidebar location.

No modal, toast, results pane, confirmation screen, or separate discovery workflow is necessary.

The initiating control provides sufficient feedback.

---

## 28. Relationship between zero-game and normal layouts

The zero-game screen and sidebar mode should feel like two forms of the same interface rather than separate applications.

Conceptually:

```text
ZERO GAMES

             Scan games
          + Add custom game

               ↓

GAMES EXIST

GAMES
...
...
+ Add custom game
⟳ Scan games
```

The game-management controls effectively move from the center of the screen into their permanent sidebar location.

The bottom bar expands from only `Launch on startup` to the full hotkey/sound/startup control set once games exist.

---

## 29. Copy preferences

The UI should use very little copy.

Prefer:

```text
No games found

[ Scan games ]
+ Add custom game
```

over:

```text
No supported games were detected on this PC.
SaveScummer can scan common game installation locations
or you can manually configure another game.
```

Similarly, operational state should usually be communicated through short labels:

```text
Running
Stopped
Scanning…
3 games found
No new games
No known games are running.
```

rather than explanatory sentences.

---

## 30. Control feedback preference

Whenever possible, **controls should communicate the result of their own action**.

For example:

```text
Scan games
↓
Scanning…
↓
3 games found
↓
Scan games
```

rather than:

```text
Scan games
↓
toast appears
↓
results dialog appears
↓
user dismisses it
```

This is a broader interaction principle worth applying elsewhere in the product when appropriate.

---

## 31. Visual hierarchy

The main visual emphasis should roughly be:

1. **Create Checkpoint**
2. **Load**
3. selected/running game identity
4. history checkpoints/restores
5. ordinary lifecycle events
6. library-management controls
7. bottom-bar preferences

The UI should not let configuration compete visually with the checkpoint workflow.

When no game is selected because none are running, the main pane should remain visually quiet rather than trying to manufacture a focal point.

---

## 32. Game-like visual direction

The product can feel more like a game through:

- darker layered surfaces
- stronger typography
- crisp high-contrast icons
- accent color for checkpoints
- timeline connectors
- subtle glow or selected-state treatment
- keyboard-key styling
- satisfying button states
- restrained transitions

Avoid relying heavily on:

- fake sci-fi panels
- excessive neon
- giant game artwork
- ornamental borders everywhere
- role-playing terminology that obscures ordinary actions

`Save`, `Load`, `Checkpoint`, `Running`, and `History` are already understandable gaming language.

---

## 33. Layout stability

Controls should generally stay where the user expects them.

Examples:

- the selected game's status is always under its name
- `…` is always the final game action
- saved-checkpoint restore/delete actions are always at the end of that row
- loaded-checkpoint revert/delete actions are always at the end of that row
- scan feedback occurs inside the Scan control
- game-management controls stay at the bottom of the sidebar
- `Launch on startup` stays toward the far right of the bottom bar
- running games always appear above installed games when both groups exist
- the active running game is always at the top of the running stack/list
- new history entries do not pull a scrolled user back to the top
- live relative-time updates do not resize or reorder rows
- the error block stays directly under the action row, above the instructions block (section 8)
- a pending checkpoint deletion shows its countdown inside the same row

Avoid layouts where successful actions cause unrelated blocks of UI to jump around.

---

## 34. Motion and animation

Motion should be restrained and never required to understand state.

### Reduced-motion requirement

Animations should only run when the system indicates that the user has **not** requested reduced motion.

In web/CSS terminology, motion-enhanced behavior should be gated behind the equivalent of:

```text
prefers-reduced-motion: no-preference
```

If the system requests reduced motion, transitions should become immediate or use a non-motion state change.

### Empty screen → sidebar mode

When SaveScummer transitions from zero configured games to at least one configured game, the layout changes from the full-width empty state to sidebar mode.

When motion is allowed, the sidebar should animate in from the left while the main content settles into its new width.

The animation should be short and functional, not theatrical.

When reduced motion is enabled, the sidebar should simply appear in its final position with no slide animation.

### General motion preference

Use animation mainly to clarify structural changes or acknowledge meaningful actions.

Brief success flashes on Save/Load controls are acceptable when motion is allowed, but the success state must remain understandable without animation.

Avoid persistent decorative motion, looping effects, or motion that competes with the history feed.

No essential status or feedback should be communicated only through animation.

---

## 35. Window lifecycle, tray behavior, and sizing

### Close behavior

Closing the main window minimizes/hides SaveScummer to the system tray rather than terminating the application.

This preserves background game detection and global hotkeys.

The behavior of tray-menu commands and any explicit full-application exit flow is outside the scope of this main-window document.

### Resizing

The user may resize the main window above a reasonable minimum size.

The minimum should be large enough that:

- the sidebar remains usable
- the main Save / Load / More action row remains coherent
- the history time column does not truncate its supported labels
- row actions remain reachable
- the bottom control bar does not overlap itself

The main window does not need a separate narrow/mobile responsive layout below that minimum; resizing simply stops at the minimum supported dimensions.

### Accessibility basics

Icon-only controls such as More, historical Load/Revert, and Delete require meaningful accessible names/tooltips.

Keyboard focus order should follow the visual interaction order, and destructive actions must remain keyboard reachable.

---

## 36. Preferred interaction character

Overall, SaveScummer should feel:

**Immediate** rather than wizard-driven.  
**Compact** rather than dashboard-heavy.  
**Chronological** rather than slot-based.  
**Game-like** rather than corporate.  
**Functional** rather than decorative.  
**Adaptive** rather than showing empty or irrelevant structure.  
**Quiet when nothing is happening**, with stronger feedback only around meaningful actions.

The information architecture and interaction behavior described here should remain the common foundation across visual explorations. Themes may vary substantially in typography, color, borders, iconography, and atmosphere, but they should preserve these structural and behavioral rules.

---

## 37. Instructions block

The selected game's catalog instructions (`info`) appear between the error block and the
history feed.

- **Collapsed presentation:** about 100 px tall, fading to transparent at the bottom,
  text still selectable, and the whole block clickable to expand.
- **Expanded presentation:** the block grows in place to show the full text with compact
  paragraph and numbered-list spacing; clicking again collapses it. The expanded or
  collapsed choice is remembered per game for the session.
- The block is omitted entirely when a game has no instructions.
- Instructions are read-only and come from the host's game read model.
- Number Save and Load procedures separately, each starting at 1.

```text
HISTORY
──────────────────────────────────────────────────────────────
  To save: exit to main menu, then press CREATE CHECKPOINT.
  1. Exit to Main Menu (the game saves data here).
  2. Press CREATE CHECKPOINT.
  3. Choose Continue in game.
  ░░░░░░░░░ (fades to transparent; click to expand)
```

## 38. Dialogs

Three dialogs remain. They were previously specified in `PLAN.md`; they now live with
the rest of the UI. Every dialog action row uses text-only buttons with no icons, sizes
to its content, is non-resizable, and keeps a fixed, violet-accented default button that
Enter activates regardless of where keyboard focus is.

### Add custom game

```text
Game executable: [................................] [Browse…]
Save location:   [................................] [Browse…]
Name:            [................................]

                                      [Add] [Cancel]
```

- Keep the fields in that order.
- Require a nonblank trimmed name and nonblank absolute executable and save-location
  paths.
- Browse uses a file picker for the executable and a directory picker for the save
  location; fields stay editable so expected paths that do not exist yet can be typed.
- Choosing an executable with Browse fills Name with the executable filename minus its
  final extension when Name is blank; never overwrite a nonblank Name.
- Show validation errors in the dialog, preserve entered values after rejection, create
  no partial entry. The host generates the stable ID.
- **Add** is the fixed, violet-accented default.

### Configure

```text
Game executable:     [...prefilled path...] [open] [Reset]
Game data dir (DIR): [...prefilled path...] [open] [Reset]

                                            [Save] [Cancel]
```

- For a custom game, also show an editable Name field, label DIR as Save location
  consistently with Add, and omit Reset.
- Changing and saving configuration updates the resolved game only after core validation
  succeeds. Show field and path errors in the dialog and preserve the previous committed
  configuration when validation fails, keeping entered values for correction.
- **Save** is the fixed, violet-accented default.

### Flush checkpoints

- Enable the action when any saved snapshots, recovery snapshots or history entries
  exist and the game is neither busy nor awaiting recovery. Recovery data retained after
  an interrupted operation is included only after that interruption is resolved.
- Confirmation copy: "Permanently delete all backups and clear this game's history?"
  with "Your current game data will be kept."
- Show separate counts for saved backups, recovery points and incomplete copies, plus
  their paths in **Details**; fetch longer path lists in pages bound to the confirmation
  revision.
- Refresh backup discovery before accepting the preview revision. External deletion,
  replacement, modification or discovery of a backup invalidates the confirmation and
  requires a new preview.
- On acceptance, remove the game's saved and recovery snapshots, including imported
  existing backups, and clear its history. Leave the current DIR untouched. Only clear
  records whose deletion succeeded; report failures.
- **Cancel** is the fixed, violet-accented default. Delete backups and Cancel are
  text-only, and focusing Delete backups does not promote it.

Per-checkpoint deletion is not a dialog; it uses the inline countdown described in
sections 17 and 18.

## 39. Testing

UI behavior tests run against a fake service that can produce busy, failure,
unavailable-snapshot and disconnected states:

- sidebar groups and conditional headings, including hidden uninstalled games;
- focus-driven selection, manual selection pinning, and close/start behavior;
- scan progress and zero/singular/plural post-scan feedback based only on newly found
  known games;
- custom-game validation and the custom-game dialog defaults;
- collapsed and expanded instructions, including games with no instructions;
- the sticky error block's content and buttons (catalog in `PLAN-ERRORS.md`);
- delete countdown cancel and confirm;
- fixed Enter/default actions in every dialog, unaffected by focus movement;
- keyboard reachability of destructive actions, tooltips and accessible names for
  icon-only controls.

## 40. Recorded decisions

- **Game icons remain.** Sidebar entries and the main header show the game's cached
  artwork with an initials placeholder, as before; the host artwork service stays in use.
- **Confirmed-uninstalled games are hidden**, and the `Uninstalled` status is removed.
- **`Flush checkpoints…`** is the operation name everywhere.
