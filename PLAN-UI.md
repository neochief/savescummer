# Save Scummer UI

This is the main window, `SaveScummer.UI`. The host starts it when the user launches the app and it ends when closed; the tray, hotkeys and everything else that runs without a window belong to the host. Core behavior (SAVE, LOAD, REVERT, snapshots, operation safety) and the protocol the UI talks to live in [`PLAN-HOST.md`](PLAN-HOST.md); the error block's contents live in [`PLAN-ERRORS.md`](PLAN-ERRORS.md).

The app should feel like a game-oriented utility: not a generic settings app, and not an in-game fantasy interface. It is **history-first**. There is no fixed number of save slots, so the main view is an activity log listing every event, newest first.


## PRINCIPLES

These decide the cases this document doesn't cover:

- **Controls report their own results.** The button that started an action shows its progress and its outcome. Don't add toasts, results panels or success dialogs. Why: feedback shows up where the user is already looking, and there is nothing to dismiss.
- **Only show what applies right now.** No empty groups or controls without a target. Keep a heading for every non-empty sidebar group, even when it is the only group, to preserve alignment with the main actions. Why: a list of games that repeats "INSTALLED" on every item, or hotkeys when there are no games, is noise.
- **Stable layout.** Controls stay in fixed places. Live updates (new history rows, midnight regrouping) never move the user's scroll position or resize rows. Why: the user is often mid-game and glances at the window. Things that jump around look like errors.
- **Keep text short.** Use short state labels (`Running`, `Scanning…`, `No new games`), not sentences explaining what a button obviously does.
- **Running games get priority, but never take the view from the user.** Only an actual switch to a game's window changes what the app shows (see ACTIVE STACK).
- **Few dialogs.** Common actions happen in place. Only Settings, path configuration, adding a game and Flush get dialogs.
- **Plan for thousands of history entries,** not a demo with five. Use virtualized rendering and compact rows. Don't use save cards.

Visual emphasis, from strongest to weakest:

1. Save
2. Load
3. The selected game's identity
4. Checkpoint, load and revert rows
5. Game started/closed rows
6. Library controls (Scan, Add custom game)
7. Settings below Scan games

Settings should never draw the eye away from the checkpoint workflow.

To make it feel like a game, use presentation: dark layered surfaces, strong type, crisp icons, an accent color for checkpoints, subtle zebra rows, shortcut hints and satisfying button states. Avoid fake sci-fi panels, heavy neon, giant artwork, ornamental borders and role-playing words that hide ordinary actions. "Save", "Load" and "History" already sound like games.


## LAYOUTS

The layout depends on **visible games**, not on database records. Games confirmed uninstalled are hidden, and hidden records don't count. The window has three states, each growing out of the previous one.

### 1. No visible games

The main area takes the full width and there is no sidebar. It shows one compact, centered block:

```text
┌────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ SaveScummer                                                                              ─  □  ×   │
├────────────────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                                    │
│                                                                                                    │
│                                                                                                    │
│                                                 ◇                                                  │
│                                                                                                    │
│                                      No supported games found                                      │
│                                                                                                    │
│                                         + Add custom game                                          │
│                                          [ ⟳ Scan games ]                                          │
│                                             ⚙ Settings                                             │
│                                                                                                    │
│                                                                                                    │
│                                                                                                    │
└────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

There's no explanatory paragraph and no illustration. Settings sits directly below Scan games and opens the same dialog as in the sidebar. There is no status bar in any layout.

### 2. Games visible, none running

When the first game becomes visible, Add custom game, Scan games and Settings move from the center to the bottom of the sidebar. The sidebar slides in from the left (see MOTION).

Nothing is preselected. The sidebar shows the installed games under `INSTALLED`, even when there is only one game, and the main area stays quiet:

```text
┌────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ SaveScummer                                                                              ─  □  ×   │
├────────────────────────┬───────────────────────────────────────────────────────────────────────────┤
│ INSTALLED              │                                                                           │
│ ┌────────────────────┐ │                                                                           │
│ │ XCOM 2             │ │                                                                           │
│ └────────────────────┘ │                                                                           │
│ ┌────────────────────┐ │                                                                           │
│ │ NOITA              │ │                                                                           │
│ └────────────────────┘ │                    No known games are running.                            │
│ ┌────────────────────┐ │                                                                           │
│ │ BATTLE BROTHERS    │ │                                                                           │
│ └────────────────────┘ │                                                                           │
│ ┌────────────────────┐ │                                                                           │
│ │ VOID WAR           │ │                                                                           │
│ └────────────────────┘ │                                                                           │
│                        │                                                                           │
│ + Add custom game      │                                                                           │
│ ⟳ Scan games           │                                                                           │
│ ⚙ Settings             │                                                                           │
└────────────────────────┴───────────────────────────────────────────────────────────────────────────┘
```

### 3. A game is running

The running game gets its own group at the top of the sidebar. It is selected when nothing else is, or when the user switches to its window (see ACTIVE STACK). Its card supplies the game identity and running marker; the main area starts directly with actions, followed by history:

```text
┌────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ SaveScummer                                                                              ─  □  ×   │
├────────────────────────┬───────────────────────────────────────────────────────────────────────────┤
│ RUNNING                │           [Ctrl+F5]                      [Ctrl+F9]                        │
│ ┏━━━━━━━━━━━━━━━━━━━━┓ │ ┌────────────────────────────┐ ┌────────────────────────────┐             │
│ ┃ VOID WAR         ● ┃ │ │          ⚑ SAVE            │ │          ↶ LOAD            │  [ ··· ]    │
│ ┗━━━━━━━━━━━━━━━━━━━━┛ │ └────────────────────────────┘ └────────────────────────────┘             │
│                        │                                  Before boarding · 12:24:03               │
│ INSTALLED              │                                                                           │
│ ┌────────────────────┐ │ TODAY                                                                     │
│ │ XCOM 2             │ │ ⚑ 12:24:03  Saved                                               [↶] [✕]   │
│ └────────────────────┘ │             Before boarding                                               │
│ ┌────────────────────┐ │ ↶ 11:18:44  Loaded · 10:47:10                                    [↶] [✕]  │
│ │ NOITA              │ │                                                                           │
│ └────────────────────┘ │ ● 10:04:12  Game started                                                  │
│ ┌────────────────────┐ │                                                                           │
│ │ BATTLE BROTHERS    │ │ YESTERDAY                                                                 │
│ └────────────────────┘ │ ⚑ 23:20:12  Saved                                               [↶] [✕]   │
│                        │             Add label…                                                    │
│                        │ ■ 22:58:40  Game closed                                                   │
│                        │                                                                           │
│                        │                                                                           │
│ + Add custom game      │                                                                           │
│ ⟳ Scan games           │                                                                           │
│ ⚙ Settings             │                                                                           │
└────────────────────────┴───────────────────────────────────────────────────────────────────────────┘
```


`✕` stands for the trash icon. Each sidebar box is an art card: the capitalized name stands for the game's logo, the heavy border marks selection and `●` marks a running game. Save and Load share the card's height and align with its top and bottom edges. Their widths are equal, with a separate fixed-width area for `···`. The Load caption is attached to its bottom edge; the text mockup puts it on a separate line to show the attachment. It never shifts the centered icon and title. Shortcut hints sit immediately above each button, in the same horizontal band as the first sidebar group heading.

History entries alternate between two subtly different dark backgrounds across the full row, including its label or notes; the text mockup cannot show this dimmed zebra treatment. Date headings use the same font size as the main row text. There is no timeline line or separate timeline point column. The error and instructions blocks, when present, sit below the action area and its hints, before the first history date heading. There is no visible `HISTORY` title.


## ACTIVE STACK, SELECTION AND FOCUS

The ACTIVE STACK holds the running games, ordered by when the user last switched to each game's window outside the app. The **active game** is the game the user was last in: the top of the stack or, after that game closes, still that game until another running game is in front 5 seconds or more after the close (PLAN-HOST.md, MONITOR AND ACTIVE STACK). It's the target of global hotkeys while the app is unfocused. The game **selected** in the main view can be different, because the user may be browsing another game.

The key rule: **only an external focus change moves a game up the stack and selects it.** The user has necessarily left the app to switch to the game, so the view never changes while they're using it. Starting or closing a game in the background moves it between groups but never takes over the view. A newly started game appears under `RUNNING` but doesn't jump ahead of games with a more recent focus.

The main view changes automatically only when:

- an external focus change moves a running game to the top;
- nothing is selected yet.

Consequences:

- **At startup,** select the most recently focused running game if the order is known, otherwise the top running game. If no game is running, select nothing. Don't preselect a game just because it's installed. The main view then shows only the quiet line `No known games are running.`
- **When a game closes,** the view stays. The active game stays active and moves from `RUNNING` to the library, so the user can load it before relaunching; the view moves on when another game takes over as the active game.
- **When the user picks a game in the sidebar** (running or not), only the view changes, not the stack.


## SIDEBAR

The sidebar is the game library and the main way to navigate. It stays narrow, and each game is a small wide art card, like a game in the Steam library. Why: players recognize a game by its art faster than by its name, and the art gives the app the game feel the plain list lacked.

**Cards:**

- About 3:1 and roughly 76 px tall, the full width of the sidebar, with a small gap between cards.
- The background is the game's Steam hero art, cropped to fill the card.
- The game's logo sits on the left, fitted to the card height, over a dark gradient that fades out to the right. The gradient keeps light and dark logos readable on busy art.
- A small `●` in the top-right corner marks a running game. The marker keeps the running state visible on the card itself, including when its group heading has scrolled out of view.
- Nothing else goes on the card: no counts, timestamps or descriptions. The one exception is the **install tag** the host supplies when the same game is installed twice (`Steam`, `GOG`): a small tag in the bottom-right corner. Why: two cards with the same art are otherwise impossible to tell apart. Games with one install never show a tag.
- The game's name is always the card's accessible name and tooltip, even when only the logo shows it. With an install tag, both include it: `Dead Cells — GOG`. Per-game dialog titles include the install tag too.

**Selection:** the selected card gets an accent outline, and the other cards are slightly dimmed. Why: an outline alone gets lost on busy art. A hovered card is shown undimmed.

**Fallbacks.** Many games lack some art, and custom games have none. Each card uses the best art available:

- No logo: the game's name as text, in the logo's place.
- No hero art: the Steam header image (`header.jpg`), cropped the same way.
- No art at all, or still loading: a neutral background with the game's initials and its name as text.

**Where the art comes from.** The host supplies it; the UI never downloads artwork itself.

- The host provides each game's hero art, logo, header image and icon, already scaled down and stored locally, and says when new art arrives (where it gets them is in PLAN-HOST). Drawing the sidebar is a small local read.
- Steam's library places each logo at a position chosen per game, stored only in its binary `appinfo.vdf`. We don't use that; left-aligning the logo works on a small card.
- The app's own icon (`assets/icon.svg`) stays the window and taskbar icon (the host uses it for the tray).

Cards trade some density for recognition: about eight fit in a default-height window, and a large library scrolls.

**Groups:**

- Always show a heading for each non-empty group: `RUNNING` for running games and `INSTALLED` for the rest, with `RUNNING` on top when present. This applies even to a single game or a single group.
- The first group heading shares a fixed-height horizontal band with the shortcut hints above the main buttons. Do not collapse that band when there is only one group; the first card and the main buttons must keep matching top edges.
- There is no overall `GAMES` heading. The sidebar is obviously a game list, so it would only take space.
- Never show an empty group.

**Visibility:**

- Hide confirmed-uninstalled games, whether they are known games or custom games. A custom game whose executable disappears is hidden until it comes back.
- Keep installed games that haven't produced save data yet.

**Library controls** stay anchored at the bottom of the sidebar in this order: Add custom game, Scan games, Settings.

- **`+ Add custom game`** is for games that aren't recognized automatically or that need custom paths. Use this exact wording; "Add game" would suggest it's the normal way to add games. Custom games are kept forever, and the main window has no way to remove them. A future management screen may add that.
- **`⟳ Scan games`** adds discovered games immediately. There's no results screen and no confirmation. The button itself cycles through these states:

  ```text
  ⟳ Scan games → ◌ Scanning… → ✓ 3 games found | No new games → ⟳ Scan games
  ```

  The count includes only *newly* found known games, with the singular form for one game. Scan and Add custom game behave the same in the zero-games layout, so users learn them once.

  The button is a fallback, not the normal way games appear. The host also scans in the background: when the window is shown or focused, when a store reports an install, and periodically (PLAN-HOST.md, Known games and scanning). The window reports being shown or focused to the host with its focus report; the host decides whether a scan is due.

  Background scans are **silent**: the button keeps its idle state, and newly found games simply appear in their group. Only a scan the user started drives the `Scanning…` and result states. Why: focus scans happen on nearly every alt-tab, and a button that flickers `Scanning… → No new games` each time is noise. If the user presses the button while a background scan is running, the button shows `Scanning…` until the scan they asked for finishes, and counts games found since they pressed.


- **`⚙ Settings`** is a cog-and-text button directly below Scan games. It opens the Settings dialog for shortcuts, sound effects and launch on startup. It stays available with no selected game and while a game is busy.

## MAIN ACTIONS

There is no game name, icon or status header above the buttons. The selected sidebar card already identifies the target and shows whether it is running. Stopped games remain selectable and use the same action layout.

- **Alignment:** the action row starts at the same vertical position as the first sidebar game card, below the always-visible first group heading. The shortcut hints occupy the same band as that heading. In the default running-game view this is the selected game's card. Keep the action row in place when the user selects a lower card or scrolls the library; do not chase that card down the window.
- **Height:** Save and Load are both the same height as a sidebar game card (roughly 76 px), with matching top and bottom edges.
- **Width:** reserve a fixed-width area at the right for `···` and the gaps between controls. Divide all remaining main-view width equally between Save and Load: 50/50. They keep equal widths as the window resizes.
- **Contents:** center each button's icon-and-title group horizontally and vertically. The Save and Load groups share the same vertical center. Attach Load's smaller target caption absolutely to the button's bottom edge, outside the icon-and-title layout; the caption takes no layout space and never moves either main title. Reserve clearance below the buttons so the caption cannot overlap the content that follows.
- **Shortcut hints:** show the current Save shortcut centered immediately above Save and the current Load shortcut centered immediately above Load. These are quiet help text/keycaps, not extra buttons. Keep the gap to the buttons small and align their band with the first sidebar group heading.
- **Availability:** the host decides whether each action is available, separately from whether the game is running. When there is no game data to copy, Save is disabled and its tooltip and accessible name say `No game data yet`. Nothing else signals this.
- The order is fixed: Save, then Load, then `···`, which is always last.
- Below the buttons and the Load caption sit the **error block**, then the **instructions block**. The error block stays until the next action or game selection and never becomes a modal dialog.

### Save

This is the main action and has the strongest emphasis. It supports the core loop: reach an important moment, save, keep playing. The new entry goes to the top of the history.

### Load

The main Load button restores the **latest retained checkpoint**. Only checkpoints the app made count; copies the user makes by hand are not checkpoints.

What Load will restore is shown in a smaller, quieter caption attached absolutely to the button's bottom edge (see MAIN ACTIONS). Use the exact local time for today's save (`12:24:03`); include the day for earlier saves (`Yesterday · 23:20:12`, or `2026-09-20 · 12:24:03`). With no checkpoints, the button is disabled and its caption reads `No saves yet`. When the host reports no game data on disk, Load is disabled too, with the same `No game data yet` tooltip as Save; the caption still shows what it would restore. The history's Load and Revert buttons follow the same rule.

If that save has a label, the label comes first: `Before boarding · 12:24:03`. The button keeps its width, so a long label is shortened with `…` and the timestamp always stays visible. The full label is in the tooltip. The caption never changes the button's size or the vertical alignment of its icon and title.
A pending deletion doesn't change Load. It still targets the latest checkpoint, even if that checkpoint is counting down to deletion, and loading doesn't cancel the deletion. The host runs the two one after the other, and the latest checkpoint is recalculated only after the deletion succeeds.

Every successful Load, and every Revert, also creates a **recovery point** holding the state just before it. That is what the row's Revert restores. Load errors go to the error block.

Load always restores the whole checkpoint, every save location in it, exactly as it was: saves made after the checkpoint are removed. Why: many games continue from their newest file, so a Load that left newer saves behind would silently not restore. The removed saves are in the recovery point, and the Loaded row says how many were removed. Revert works the same way, so it is a true undo.

### `···` menu

Each command has its own icon:

```text
[       ⚑ SAVE       ] [       ↶ LOAD       ] [ ··· ]
                            12:24:03         ┌──────────────────────────────────┐
                                             │ ▤  Open checkpoints folder       │
                                             │ ✎  Configure paths…              │
                                             │ ✕  Flush checkpoints (2.4 GB)…   │
                                             └──────────────────────────────────┘
```

- **Open checkpoints folder** opens this game's folder in the checkpoint store, where each checkpoint is an ordinary folder named by its time and kind. Why here and not on history rows: rows already carry Load, Revert and Delete, and a fourth icon on thousands of rows is noise; the folder names make a checkpoint easy to find.
- There is no command to open the save location. Why: a save set can span several folders, so it would need a submenu and rules for patterns and shared folders, for little gain. Configure paths shows every location's path.
- **Configure paths…**
- **Flush checkpoints…**, the name used for this action everywhere. The menu item shows the size of what Flush would delete, as the host reports it: `Flush checkpoints (23 MB)…`. Why: the size is often the reason to flush. With nothing to flush the item is disabled and shows no size; when the size is unknown (the store isn't connected) it shows none either.

Sizes are rounded: one decimal under 10, whole numbers from 10 up (`840 KB`, `23 MB`, `2.4 GB`, `12 GB`), with a space before the unit. Units follow the OS file manager: 1024-based on Windows, 1000-based on macOS and Linux. Why: the number matches what the user sees when they check the folder.

App-wide preferences belong to the Settings dialog, opened by `⚙ Settings` below Scan games in the sidebar.

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

The history is a vertical, newest-first, effectively unlimited activity log. It should read as a run log, not as save slots or cards. Start directly with the first date heading; do not render a `HISTORY` title. Use dimmed zebra backgrounds to distinguish entries. Do not draw a timeline connector or separate timeline points.

**Empty history** shows one quiet line: `Saves will appear here.`

### Rows

Each row reads from left to right:

1. **An event icon** at the far left. It supplements the description and never replaces it; no separate timeline marker is drawn.
2. **One exact local time**, `HH:mm:ss`, on the first line. No relative age, `ago` wording, repeated day label or second timestamp line.
3. **The description**, with the save label or load notes beneath it when applicable.
4. **Actions** at the far right of the first line, when the row has any. They sit together in one line, with Delete always last.

Details that are easy to get wrong:

- Use the same font size for date headings, exact times and the main event descriptions. Labels and notes can remain quieter secondary text.
- Group rows under day headings: `Today`, `Yesterday`, the full localized weekday for recent days, and `yyyy-MM-dd` for older dates. Headings align with the row's left inset, before the icon column. They provide the date once for the whole group.
- Alternate two subtly different dark backgrounds across complete entries, including each entry's second line. These are continuous rows, not bordered cards. Day headings stay outside the striping. Base alternation on the row's position in its day group, not on which rows happen to be rendered in the viewport.
- The time column fits `HH:mm:ss` without truncation. Times never tick or change as an entry ages.
- At local midnight, recalculate the day groups without moving the scroll position or resizing rows.
- New entries go on top. If the user has scrolled down, keep their position; never pull them back to the top.

### Event kinds

| Icon | Event | Weight | Actions |
| --- | --- | --- | --- |
| ◆ | Saved | Significant | Load this save, Delete |
| ↶ | Loaded · <label or time of the save it loaded> | Significant | Revert this load, Delete |
| ↷ | Reverted · <time of the row it reverted> | Significant | Revert this revert, Delete |
| ● | Game started | Light | None |
| ■ | Game closed | Light | None |

The glyphs above are placeholders, but each event kind needs a stable icon that is easy to tell apart. Started and closed rows make play sessions visible without a separate session UI.

- **Saved** can carry a label (see below).
- **Loaded** names the save it loaded by that save's label, or by its exact `HH:mm:ss` when it has none: `Loaded · Before boss fight`, `Loaded · 10:47:10`. It follows the label live, so renaming a save renames its loads too, and it keeps the label even after that save is deleted. A long label is shortened with `…`, and the full label is in the tooltip.
- **A Loaded row's second line** carries short notes when the host reports them. The host owns these facts; the UI only shows them:
    - what the Load removed: `Removed 2 newer saves, kept in the recovery point`;
    - that Steam Cloud undid part of it: `Steam Cloud replaced the restored save`. The host can only tell at the next game launch after the Load, so this note appears later, in place. Why: without it the user would think Load failed, or not notice that the game started from a different save.

  Two notes share the line, separated by ` · `. The line is always there, so a note arriving later never resizes the row.
- **Load this save** loads *that* save instead of the latest one. It uses the same button style and icon family as Revert.
- **Revert this load** restores that load's recovery point, which is the state just before the load. Like a Load, it first keeps the current state as a new recovery point, so a revert never loses progress. The result is a `Reverted` row with its own Revert, which undoes the revert. Nothing is used up: the loaded row keeps its Revert too. Why: one rule for every restore is easy to trust, and any state the user leaves can be brought back.
- Revert changes game data; Delete destroys a checkpoint, so they must not get equal emphasis. Delete stays quiet until the row is hovered, focused or selected.

### Labels

A save's label goes on the row's second line, under `Saved`, for example `Before boss fight`. It is edited right there in the row. Why: the user names a save while it's fresh, without opening a dialog, and the history reads like a log of decisions instead of a column of identical `Saved` rows.

A new save has no label. Its second line shows a dimmed `Add label…` suggestion, which looks like a button when hovered:

```text
⚑ 12:24:03  Saved                                 [↶] [✕]
            Add label…
```

Clicking the label, or `Add label…`, turns that line into a text field with an inline check button (`✓`) on the right. Why an icon: a second button called "Save" would be confused with the main Save action.

```text
⚑ 12:24:03  Saved                                 [↶] [✕]
            [Before boss fight_          ] [✓]
```

Changes save automatically, so the user never has to remember to confirm. A save happens on any of these:

- a short pause after typing, restarted by every keypress;
- the field losing focus;
- pressing the check button, or Enter.

Clearing the text removes the label, and the line goes back to `Add label…`. A label is only a name: editing it changes no game files and adds no history event.

- **One line, up to 100 characters.** The field stops accepting text at the limit, and pasted line breaks become spaces. Leading and trailing spaces are dropped, so a label of only spaces is the same as none.
- **Editing works while the game is busy** and while the save is counting down to deletion, because it touches no files.
- **The label belongs to the save, not to the row.** Every place that names the save shows it: this row, the Loaded rows that loaded it, the Load button and the Flush dialog's Details.
- **Escape cancels the edit.** It puts back the label from before editing started and closes the field, undoing anything autosave already stored during this edit. Why: autosave is there so the user never has to confirm, not so a slip can't be taken back.
- **If the save disappears while its label is being edited** (deleted, flushed or changed outside the app), the edit is dropped along with the row, with no error.

Only saved checkpoints have labels. Recovery points don't: a Reverted row's second line stays empty, and a Loaded row's shows only the host's notes described above.

### Deleting a single entry

Delete is deliberately not immediate. Deleting a saved row removes the checkpoint. Deleting a loaded or reverted row removes its recovery point, and then the whole row. Pressing the trash button turns the row's action buttons into a countdown. It takes the buttons' place and grows leftwards from the row's right edge. The rest of the row stays where it is. Cancel brings the normal buttons back.

```text
Normal
⚑ 12:24:03  Saved                             [↶] [✕]
            Before entering the station

Counting down (5 → 4 → 3 → 2 → 1, updated in place)
⚑ 12:24:03  Saved              Deleting in 5 [Cancel]
            Before entering the station

Countdown over, game busy
⚑ 12:24:03  Saved         Waiting to delete… [Cancel]
            Before entering the station

Deleting (no Cancel)
⚑ 12:24:03  Saved                         ◌ Deleting…
            Before entering the station
```

Loaded and reverted rows work the same way:

```text
↶ 11:18:44  Loaded · 10:47:10                 [↶] [✕]

↶ 11:18:44  Loaded · 10:47:10  Deleting in 3 [Cancel]
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
- **Outcome:** the row is removed only after the host confirms the files are deleted from disk. If the deletion fails, the normal buttons come back and the error block shows the failure. There's no automatic retry; the user can press Delete again. Other deletions continue either way.
- **Leaving doesn't cancel anything.** Switching games, scrolling away, hiding the window or disconnecting the UI leaves accepted deletions running. When the user returns, the rows are rebuilt from the host's state.
- **If Flush removes an entry** that is waiting to be deleted, that deletion is dropped quietly, with no second attempt and no "not found" error.

There is no queue screen, no batch confirmation and no extra history event for deletions.


## SHORTCUT HINTS

The current shortcuts appear immediately above their corresponding main buttons, in the same horizontal band as the first sidebar group heading. There is no bottom/status bar. Reassigning a shortcut in Settings updates its hint after the host accepts the change. Use the platform's key names; defaults are `Ctrl+F5` / `Ctrl+F9` on Windows and `⌥F5` / `⌥F9` on macOS.

**Which game the hotkeys act on:**

- **When the app is focused,** they act on the *selected* game, even a stopped one.
- **When the app is unfocused,** they act on the active game, even one that just closed.
- **With no target,** they do nothing. With no selected game there are no main buttons or hints; Settings remains accessible.
- While the target game is busy, conflicting operations and their hints appear unavailable. A pending deletion countdown alone changes neither whether the hotkeys work nor which game they target.
- While a shortcut field is capturing a replacement in Settings, those keypresses are input to the field and must not trigger Save or Load.

## DIALOGS

There are four dialogs: Settings, Add custom game, Configure paths and Flush checkpoints. All of them:

- size to their content and can't be resized;
- use text-only buttons;
- rely on the platform's standard behavior for focus, keyboard navigation, the default button and cancelling. Don't write custom Enter or focus handling; a deliberately focused button must still activate normally.

The title of a per-game dialog includes the game's name (`Configure paths · Void War`), so the target stays explicit even when the sidebar is covered. Include the install tag when needed to distinguish installs.

Deleting a single checkpoint is not a dialog; it uses the row countdown above.

### Settings

Opened by the cog-and-text `⚙ Settings` button directly below Scan games, including in the zero-games layout.

```text
┌──────────────────────────────────────────────────────────────┐
│ Settings                                                   × │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│ Save shortcut       [ Ctrl+F5                              ]  │
│ Load shortcut       [ Ctrl+F9                              ]  │
│                                                              │
│ ☑ Play sounds                                                │
│ ☑ Launch on startup                                          │
│                                                              │
│                                        [ Save ]  [ Cancel ]  │
└──────────────────────────────────────────────────────────────┘
```

- Focus a shortcut field and press the desired combination to replace it. Show the platform's key names and the currently saved bindings when opening the dialog.
- `Play sounds` enables or disables sound effects; `Launch on startup` enables or disables starting the host at sign-in. These preferences appear only here, not in the main window.
- Apply changes on **Save**, the default button, after the host validates and accepts them. Cancel or closing the dialog discards unapplied edits. Accepted settings persist across restarts; shortcut hints update immediately.
- A duplicate, unsupported or unavailable shortcut shows an inline error beside its field. Keep the dialog and entered values open for correction; rejected changes leave the saved configuration and previous bindings intact.
- The host owns shortcut registration, persistence, sound playback and startup integration. Shortcut reassignment requires extending the host's settings contract beyond its currently documented fixed defaults (PLAN-HOST.md, Hotkeys and protocol settings); the UI does not register a second independent set of global hotkeys.

### The save location field

Both dialogs below share one **Save location** field, for known games too. A save can be a whole folder, one file, or a set of files in a folder, so the field takes any of the three:

- **Browse** picks a folder, the most common case.
- The text stays editable, so the user can turn it into a file path or a pattern in the catalog's pattern syntax: `D:\Game\saves\*.sav`, `D:\Game\Profiles\C*\SGS*`.
- A muted hint is always shown under the field, not as a tooltip: `A folder, a file, or a pattern such as D:\Game\saves\*.sav`. Why: most users never guess that a field with a folder picker also takes a pattern, and a tooltip is found only by those who already suspect it.
- Errors about the save location appear below the hint, which stays visible.
- A path that doesn't exist yet is fine, such as a save folder the game hasn't created. Checking a path never creates anything.

A custom game has exactly one save location. Two separate locations for one custom game are not supported for now.

The host validates the save location and owns the rules. What the user can run into:

- the path isn't absolute;
- the location is too broad: a whole broad folder (a drive or home folder, Documents, Saved Games, AppData, Program Files, a Steam library and the like), or a pattern directly inside one, is rejected, while an exact file or folder name inside one is fine (`Documents\mygame.sav`), because Load only ever touches that name;
- the pattern is known to be dangerous: a wildcard directly in the game's install folder or another broad folder, a pattern that matches the game's executable, or names ending in the app's reserved `.ssnew` / `.ssold` suffixes;
- the location overlaps another game's. Two games may share a folder only when both name exact, different files in it.

Why so strict: Load can remove files inside the save location, so a location that is too broad or overlaps another game could delete saves that aren't this game's.

### Add custom game

```text
┌────────────────────────────────────────────────────────────────────────────────────┐
│ Add custom game                                                                  × │
├────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                    │
│  Game executable  [                                        ] [Browse…]             │
│  Save location    [                                        ] [Browse…]             │
│                   A folder, a file, or a pattern such as D:\Game\saves\*.sav       │
│  Name             [                                        ]                       │
│                                                                                    │
│                                                               [ Add ]  [ Cancel ]  │
└────────────────────────────────────────────────────────────────────────────────────┘
```

After Browse filled in Name, with a validation error:

```text
┌────────────────────────────────────────────────────────────────────────────────────┐
│ Add custom game                                                                  × │
├────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                    │
│  Game executable  [D:\Starsector\starsector.exe            ] [Browse…]             │
│  Save location    [saves                                   ] [Browse…]             │
│                   A folder, a file, or a pattern such as D:\Game\saves\*.sav       │
│                   ⚠ Save location must be an absolute path.                        │
│  Name             [starsector                              ]                       │
│                                                                                    │
│                                                               [ Add ]  [ Cancel ]  │
└────────────────────────────────────────────────────────────────────────────────────┘
```

- The fields stay in this order. The name must be non-blank after trimming, and both paths must be non-blank.
- Browse opens a file picker for the executable and a folder picker for the save location. Both fields stay editable.
- When the user picks an executable with Browse and Name is blank, Name is filled with the file name minus its final extension. A name that's already filled in is never overwritten.
- Errors appear in the dialog and the entered values stay. Nothing partial is created. The host generates the stable ID.
- **Add** is the default button.

### Configure paths

Known game:

```text
┌────────────────────────────────────────────────────────────────────────────────────┐
│ Configure paths · Slay the Spire                                                 × │
├────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                    │
│  Game executable  [C:\…\common\SlayTheSpire\SlayTheSpire.exe] [Open] [Reset]       │
│  Save location    C:\…\common\SlayTheSpire\saves                                   │
│                   C:\…\common\SlayTheSpire\preferences                             │
│                   C:\…\common\SlayTheSpire\runs                                    │
│                   C:\…\common\SlayTheSpire\betaPreferences                         │
│                   [                                        ] [Browse…] [Reset]     │
│                   A folder, a file, or a pattern such as D:\Game\saves\*.sav       │
│                                                                                    │
│                                                              [ Save ]  [ Cancel ]  │
└────────────────────────────────────────────────────────────────────────────────────┘
```

Custom game:

```text
┌────────────────────────────────────────────────────────────────────────────────────┐
│ Configure paths · Starsector                                                     × │
├────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                    │
│  Name             [Starsector                              ]                       │
│  Game executable  [D:\Starsector\starsector.exe            ] [Open]                │
│  Save location    [D:\Starsector\saves                     ] [Browse…]             │
│                   A folder, a file, or a pattern such as D:\Game\saves\*.sav       │
│                                                                                    │
│                                                              [ Save ]  [ Cancel ]  │
└────────────────────────────────────────────────────────────────────────────────────┘
```

- **Known games** list the catalog's save locations, as the host resolved them on this machine, read-only: one line or several. Why several: a game's save can be split across folders, or kept both locally and in Steam's cloud folder, and all of them are backed up together.
- The editable field under the list is empty by default. Typing a location there replaces *all* the catalog's locations with that one, and the list is dimmed to show it no longer applies. **Reset** clears the field and returns to the catalog's locations.
- **Custom games** have an editable Name, the save location field directly (no catalog list) and no Reset.
- **Open** next to the executable opens the folder holding the saved executable, through the host like every other folder. Why: it's the quickest way to check which install the app watches (Steam's or GOG's) or to reach the game's files. While the field differs from the saved value, Open is disabled with the tooltip `Save to open this folder`. Why not open the typed path: clients never pass the host a path to act on, and opening the old folder instead would look like a bug.
- Changing the save location changes which checkpoints apply. Checkpoints made with the old location stay on disk but can't be loaded until the game uses that location again.
- Changes are applied only after the host validates them. If validation fails, the error appears in the dialog like in Add custom game, the saved configuration doesn't change, and the entered values stay for the user to fix.
- **Save** is the default button.

### Flush checkpoints

This is the only bulk delete, so it shows the user what will go before they confirm:

```text
┌────────────────────────────────────────────────────────────────────────┐
│ Flush checkpoints · Void War                                         × │
├────────────────────────────────────────────────────────────────────────┤
│                                                                        │
│  Permanently delete all backups and clear this game's history?         │
│  Your current game data will be kept.                                  │
│                                                                        │
│    Saved backups        42                                             │
│    Recovery points       2                                             │
│    Incomplete copies     1                                             │
│    Total                 2.4 GB                                        │
│                                                                        │
│  ▸ Details                                                             │
│                                                                        │
│                                                 [ Flush ]  [ Cancel ]  │
└────────────────────────────────────────────────────────────────────────┘
```

With Details expanded:

```text
│  ▾ Details                                                             │
│  ┌────────────────────────────────────────────────────────────┐        │
│  │ SAVED BACKUPS                                              │        │
│  │   …\2026-09-24 19.25.03 saved  Before entering the station │        │
│  │   …\2026-09-24 18.02.47 saved  Boss fight                  │        │
│  │   …\2026-09-23 22.40.10 saved                              │        │
│  │   Show more (39)                                           │        │
│  │ RECOVERY POINTS                                            │        │
│  │   …\2026-09-24 19.31.10 recovery                           │        │
│  │   …\2026-09-24 19.02.55 recovery                           │        │
│  │ INCOMPLETE COPIES                                          │        │
│  │   …\2026-09-24 19.40.02 partial                            │        │
│  └────────────────────────────────────────────────────────────┘        │
```

- **When it's available:** there are saved checkpoints, recovery checkpoints or history entries, and the game is neither busy nor waiting for recovery. Recovery data left by an interrupted operation is included only after that interruption is resolved.
- **The dialog is only a preview.** Before opening it, the app checks which files would be affected and shows them. Nothing from the preview is passed back to the host. Why: simplicity. The dialog tells the user what the action does; it isn't a contract.
- **Text:** "Permanently delete all backups and clear this game's history?" followed by "Your current game data will be kept."
- **Counts:** separate counts for saved backups, recovery points and incomplete copies, with zero counts left out, then the total size, the same number as the menu item, rounded the same way. One total, not a size per kind: the menu brought the user in with the size, and the dialog confirms it. Left out when the size is unknown. The paths, all inside the checkpoint folder, go under **Details**, loaded in pages. The host names checkpoint folders by time and kind, inside a folder per game. A labeled saved backup shows its label after the path, shortened with `…` when needed, so the user can recognize saves they'd miss.
- **On confirm:** the dialog closes and the UI sends a plain Flush request for the game. The host does the whole job again from scratch: it deletes every saved and recovery checkpoint of the game and clears the history. The game's save locations are not touched. Only the records whose files were actually deleted are cleared. While it runs, the game is in the normal busy state, and any failures go to the error block.
- **Cancel** is the default button.


## MOTION

Motion clarifies structural changes and confirms meaningful actions. It never carries information on its own.

- Animations run only when the system hasn't asked for reduced motion (the equivalent of `prefers-reduced-motion: no-preference`). Otherwise, changes happen instantly.
- When the zero-games layout changes to the sidebar layout, the sidebar slides in from the left and the main area settles into its new width, briefly and without drama. With reduced motion, the sidebar simply appears.
- Short success flashes on buttons are fine, but the success state must be clear without them.
- No looping or decorative motion, and nothing that competes with the history.


## WINDOW

There is one window at a time, and the host decides when it's needed:

- **Closing the window ends the UI.** The host stays in the tray, so game detection, global hotkeys and pending deletions keep running, and closing doesn't shorten any countdown.
- **When the host asks the UI to come to the front** (the user launched the app again, or picked Main window in the tray), the UI restores and focuses its window.
- **If the host goes away** (it crashed, or was stopped), the UI shows its reconnecting state and starts a new host the same way the CLI does, without a second window. A host that says it's shutting down is not restarted: the UI closes.

Deletions on shutdown:

- **The UI closing or disconnecting** never waits for deletions. The host keeps processing them. A failure doesn't block closing, doesn't reopen the window and isn't retried; the entry stays in the history for the user to retry.
- **A normal host shutdown** ends the remaining countdowns early and runs the accepted deletions through the usual per-game coordination before stopping, without keeping the UI open. Failures don't block shutdown.
- **After a crash or forced kill,** deletions that hadn't started are dropped, since countdowns aren't saved to disk. Deletions that had started follow the core rules for interrupted operations.

The tray, its menu and the full-exit flow belong to the host (PLAN-HOST.md, PROCESSES).

The window can be resized down to a minimum size. At that size:

- the sidebar is still usable;
- the Save, Load and `···` row still holds together;
- the time column doesn't truncate;
- the row actions are reachable;
- equal-width action buttons, their bottom-attached Load caption and shortcut hints do not overlap;
- Add custom game, Scan games and Settings stay reachable.

There is no separate narrow layout.

Accessibility: icon-only controls (`···`, the history row Load, Revert and Delete buttons, the label check button) need accessible names and tooltips. Keyboard focus follows visual order, and destructive actions stay reachable by keyboard.


## TESTING

UI tests run against a fake service that can simulate being busy, failing, missing snapshots, pending deletions and a disconnected host. They focus on the rules that are easy to break:

- **Sidebar:**
  - card art fallbacks: no logo, no hero art, no art at all, and custom games;
  - the running marker, including a library where every game is running;
  - the install tag on the card, tooltip and per-game dialog title, only for games installed twice;
    - headings for every non-empty group, including one game, all running and none running; no heading for an empty group;
    - hidden uninstalled games;
    - the switch to the zero-games layout based on *visible* games;
    - first-card/button alignment stays unchanged when the library moves between one and two non-empty groups.
- **Selection:**
    - external focus moves a game up and selects it;
    - a manual selection is kept when games start or close;
    - the view and the active game staying put when the active game closes.
- **Scan:** the zero, singular and plural messages, counting only newly found known games; background scans leave the button idle while their games appear; pressing Scan during a background scan shows `Scanning…` until the requested scan finishes; showing or focusing the window sends the focus report.
- **Actions:** game identity and running state appear only in the sidebar; Save and Load availability still comes from the host (no game data, no checkpoints). Verify card-height buttons, equal widths after reserving `···` and gaps, centered icon/title groups, bottom-attached Load captions and shortcut hints immediately above the buttons without overlap, including at minimum window size and with long labels. Open next to the executable is disabled while the field is edited. The Flush item's size: rounding at each boundary, the OS's units, and no size when there's nothing to flush or the size is unknown; the dialog's total matching it.
- **History rows:**
    - no visible History title; leftmost event icons, one exact time per row, no relative ages or timeline decorations, date headings and times matching the main text size;
    - dimmed zebra backgrounds spanning complete entries, consistent when scrolling a virtualized list;
    - spinner and success states on the Load and Revert buttons;
    - Reverted rows that can themselves be reverted and deleted;
    - a Loaded row's notes: removed newer saves, Steam Cloud replacing a restored save, both together, and the Steam Cloud note arriving later without resizing the row.
- **Labels:**
    - the `Add label…` placeholder;
    - the autosave pause restarting on every keypress, and saving on focus loss, on the check button and on Enter;
    - clearing a label, a spaces-only label, the 100-character limit and pasted line breaks;
    - a label shown on Loaded rows, the Load button and Flush Details, updating live, with `…` and a tooltip when too long;
    - a Loaded row falling back to the time when the save has no label, and keeping the label after the save is deleted;
    - editing while the game is busy, and the save disappearing mid-edit;
    - Escape restoring the label from before the edit, even after an autosave.
- **Deletion countdowns:**
    - they run independently;
    - Cancel works, including while the game is busy;
    - the waiting and deleting states;
    - the main Load button doesn't change;
    - rows are rebuilt after navigating away or reconnecting;
    - a failure restores the row with no retry.
- **Midnight:** rows regroup without moving the scroll position.
- **Dialogs:**
    - Settings opens below Scan games in all layouts; there is no status bar;
    - reassign both shortcuts, save and verify the hints and actual bindings, reopen and verify persistence; Cancel leaves settings unchanged;
    - duplicate, unsupported and unavailable shortcuts show inline errors without losing the old bindings; capturing a shortcut never runs an operation;
    - toggle sounds and launch on startup, save, reopen and verify the accepted values;
    - standard keyboard, focus, default-button and cancel behavior;
    - the custom-game dialog's validation and name autofill;
    - the save location hint always visible in both dialogs, with errors below it and the hint kept;
    - a folder, a file and a pattern all accepted in the save location field, and Browse still picking a folder;
    - host rejections shown in place: a relative path, a broad folder, a dangerous pattern, an overlap with another game;
    - known games: one and several read-only catalog locations, the list dimmed while the field overrides it, and Reset returning to the catalog.
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
