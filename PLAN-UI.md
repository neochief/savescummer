# Save Scummer UI

This is the main window, `SaveScummer.UI`. The host starts it when the user launches the app and it ends when closed; the tray, hotkeys and everything else that runs without a window belong to the host. Windows packages the Tauri/WebView2 UI from `apps/ui` in the installer, macOS packages the Tauri/WebKit UI in the DMG, and Linux packages the Tauri/WebKitGTK UI in x86_64 and aarch64 AppImages. Core behavior (SAVE, LOAD, REVERT, snapshots, operation safety) and the protocol the UI talks to live in [`PLAN-HOST.md`](PLAN-HOST.md); the error block's contents live in [`PLAN-ERRORS.md`](PLAN-ERRORS.md).

The app should feel like a game-oriented utility: not a generic settings app, and not an in-game fantasy interface. It is **history-first**. There is no fixed number of save slots, so the main view is an activity log listing every event, newest first.


## TAURI AND WEB UI

The UI is a separate `SaveScummer.UI` process using **Tauri 2, Vite, React, TypeScript and CSS** in `apps/ui`. Tauri hosts the desktop window and bundled web assets; React renders the stateful view and dialogs. The implemented UI in [`apps/ui/`](apps/ui/) is the visual and interaction reference. A packaged UI loads local bundled assets without a development server.

Keep the existing process boundary:

- The Rust host launches or raises one UI window. Closing that window ends the UI process while the host continues monitoring games, handling hotkeys and finishing operations.
- The Tauri Rust layer is a **thin client** of the existing versioned local JSON protocol. Reuse `savescummer-ipc` where it fits. It connects, watches host events, sends requests, reports window focus, and handles reconnect/start-host behavior as the CLI does. It owns no game rules, database, catalog resolution or checkpoint files.
- The web frontend calls a small typed bridge in the Tauri layer and subscribes to state/events. It never opens the host socket or receives arbitrary filesystem access. Keep Tauri capabilities scoped to that bridge and the window features actually used; do not enable general shell or file access in the webview.
- The host remains authoritative for busy rejection, deletion deadlines, operation results and current game state. The frontend may format time, manage selection/editing, animate and scroll, but never invents a successful operation or executes one on a local timer.
- Game artwork is supplied by the host and exposed to the webview through a narrowly scoped local asset path or bridge response, without letting web content address arbitrary files.

**Cross-platform checks:** Verify the one-window lifecycle, tray and Dock behavior, local protocol reconnection, keyboard and screen-reader access, packaged artwork/fonts, and the minimum window layout on each platform. Check CSS view transitions and scroll-driven fades on each engine; provide an equivalent static/fade-free state when an engine lacks a feature. Measure cold open time and memory with the UI open and closed. macOS activation and bundle identity follow the [platform guidance](PLAN.md#platforms).


## VISUAL DECISIONS

These accepted decisions began in the 2026-09-27 interactive prototype. They are recorded here so the retired prototype is no longer needed. Keep related sections below in sync when these decisions change.

### Icons
- **Font Awesome Pro 7.3.1 Whiteboard Semibold**, single tone, for every app-owned icon. Mapping: Save `flag`; Load, Revert, Loaded and Reverted rows `rotate-left`; Scan for games and every busy spinner `arrows-rotate` (rotated with CSS; Whiteboard has no spinner); success `check`; label `tag`; delete and Flush `trash`; Add custom game `plus`; Settings `gear`; Open checkpoints folder `folder`; Configure paths `sliders`; More `ellipsis`; Game started `circle-play`; Game closed `circle-stop`; badge time `clock`; expand/collapse `angle-down`/`angle-up`; close `xmark`.
- The one exception: the "jump to checkpoint" hover uses Duotone `crosshairs` (Whiteboard has none).
- History rows **do** have leading event icons (18 px), all the same quiet color (`#cfc4b8`). The em dash between event and chip is removed. Game started and Game closed use the same white text as the other event words.

### Surfaces and layout
- Window `#1c1918`. The sidebar has no fill of its own: the logo area and the RUNNING group sit on the window color, so the running game belongs to the main view.
- The RUNNING group has a tinted shape (`#2c2623`, 12 px top corners, inset 8 px left/right) that is solid behind the heading and fades to transparent down the card.
- INSTALLED and the library controls sit on a panel `#2c2623`, 12 px radius on all corners, inset 8 px from the window's left edge, the main view and the bottom. It starts 16 px below the running card.
- **No cutout for installed selections.** A selected installed game shows the 2 px red inset border, full-color art and 100% scale inside the unbroken panel.
- Sidebar content and the main view both use **16 px side padding**. Cards are 216 px wide. Hover backgrounds of library controls and popup rows sit 8 px inside their container edges.
- History rows: background `#221d1b`, **no border**, 8 px radius, 8 px padding, min-height 56 px, contents vertically centered. The age sits directly under the time; when a row has no age, the time centers vertically. Load notes start at the event icon's left edge (no indent).
- History scrolls on its own; the header area (title bar, Save/Load row) stays fixed. The history fades out 40 px at the top/bottom only when it can scroll in that direction (CSS scroll-driven animation).

### Typography
- **No letter-spacing adjustments anywhere.**
- RUNNING and INSTALLED are centered over the cards, 12 px/600 uppercase, text 7 px from the top of the 40 px band. Day headings use the same uppercase style (weekday names within the past week, `yyyy-MM-dd` alone for older), with the date in quieter text, and stay aligned with INSTALLED.
- App title: `assets/icon.svg` at 40 px, 2 px gap, then **SaveScummer** in Chakra Petch 700 at 25 px: "Save" `#faf8ee`, "Scummer" `#ff1e29`, with a 2.5 px `#0d0707` stroke painted behind the fill.

### Sidebar
- No running marker on the card. Instead a **red dot pulses slowly before RUNNING** (the only looping animation; disabled with reduced motion).
- Installed, unselected cards: 95% scale, `grayscale(1) sepia(.55) brightness(.62)`. Hover, focus and selection restore 100% scale; selection also restores full color.
- Button wording: **Scan for games**. In the no-games layout the order is Scan for games, Add custom game, Settings.

### Main actions
- **Load caption is a separate badge hanging on Load's bottom edge**, not a band inside the button. The icon and LOAD are centered on the full face, so SAVE and LOAD line up.
  - Ivory `#f3eee7`, 1 px `#cfc2b3` border, text `#76675b`. Line 1: clock, time, age (age in a fixed 54 px slot so the badge never changes width). Line 2 (only with a label): tag and label, separated by a hairline. Single-line badges straddle the edge; two-line badges sit slightly higher so they never reach the first heading.
  - Time format: today `HH:mm:ss`; yesterday `Yesterday HH:mm:ss`; older `yyyy-MM-dd HH:mm:ss`.
  - The badge is its own control. Clicking it jumps to the latest checkpoint in history. Hovering or focusing it shows the expanded badge in place with the full label wrapped, and swaps the tag icon for crosshairs. It never triggers or highlights Load.
- **Disabled Load (no saves):** drawn as a 2 px `#3a322e` outline with muted icon and text, and no shortcut tab, under the lockdown panel, with no badge (PLAN-LOCKDOWN: **No checkpoints yet**, or **Play first** when there's no game data either).
- Shortcut tabs: text sits 4 px from the tab's top (visually centered in the exposed part).
- **Busy:** the initiating control shows a spinner and a progressive label: SAVING, LOADING, and in history LOADING/REVERTING; then a brief check. Controls are disabled only while busy; the success state is already clickable again. The cursor is `wait` over the window while busy.
- History LOAD/REVERT buttons are **104 × 30 px**; the action column is **144 px** minimum and grows leftward for the delete countdown.

### History behavior
- **Every entry created by a user action** (Save, Load, row Load, Revert) slides in and glows red for about 3 s; the history scrolls to the top. Background updates still never move the scroll position.
- Loaded and Reverted chips are clickable: they jump to the checkpoint they refer to (scrolling only if it isn't fully visible) and flash it; the tag icon becomes crosshairs on hover.
- Relative ages update in **5-second steps** under a minute.
- Delete countdown is **3 seconds**: `Deleting in 3 → 2 → 1 [CANCEL]`, then `Waiting to delete… [CANCEL]` if busy, then `Deleting…`.
- Each game has its own history; a game with none shows `Saves will appear here.`.

### Motion
- All animation is CSS (keyframes or view transitions); script only starts a view transition and scrolls.
- The root view transition is disabled, and view-transition pseudo-elements ignore pointer events, so switching games stays instant and clickable.
- **Game switch:** only the history animates, sliding up or down according to the new game's position in the sidebar.
- **Layout switch** (no games ↔ games): the sidebar slides in/out, clipped to the window; the main view settles to its new width.

### Cursor and selection
- Pointer cursor on every interactive element (buttons, menu rows, cards, chips, badge); arrow elsewhere. Text isn't selectable except in inputs.

## PRINCIPLES

These decide the cases this document doesn't cover:

- **No OS notifications.** Visual errors and permission guidance appear only inside the app UI when its window is open.
- **Controls report their own results.** The button that started an action shows its progress and its outcome. Don't add toasts, results panels or success dialogs. Why: feedback shows up where the user is already looking, and there is nothing to dismiss.
- **Only show what applies right now.** No empty groups or controls without a target. Keep a heading for every non-empty sidebar group, even when it is the only group, to preserve alignment with the main actions. Why: a list of games that repeats "INSTALLED" on every item, or hotkeys when there are no games, is noise.
- **Stable layout.** Controls stay in fixed places. Background updates and midnight regrouping preserve the user's scroll position and row sizes. User-triggered Save, Load and Revert actions scroll the history to the new entry. Why: the user is often mid-game and glances at the window.
- **Keep text short.** Use short state labels (`Running`, `Scanning…`, `No new games`), not sentences explaining what a button obviously does.
- **Running games get priority, but never take the view from the user.** Only an actual switch to a game's window changes what the app shows (see ACTIVE STACK).
- **Few dialogs.** Common actions happen in place. Only Settings, path configuration, adding a game and Flush get dialogs.
- **Plan for thousands of history entries,** not a demo with five. Use virtualized rendering and compact rounded row boxes with consistent geometry.

Visual emphasis, from strongest to weakest:

1. Save
2. Load
3. The selected game's identity
4. Checkpoint, load and revert rows
5. Game started/closed rows
6. Library controls (Scan, Add custom game)
7. Settings below Scan for games

Settings should never draw the eye away from the checkpoint workflow.

To make it feel like a game, use presentation: dark layered surfaces, strong type, crisp icons, an accent color for checkpoints, quiet rounded history rows, shortcut hints and satisfying button states. Avoid fake sci-fi panels, heavy neon, giant artwork, ornamental borders and role-playing words that hide ordinary actions. "Save", "Load" and "History" already sound like games.

### Typography, icons and surfaces

- Use **Chakra Petch** for the main Save and Load controls, their shortcut hints, and the small Load and Revert buttons in history. Use **Inter** for the main content body, descriptions, editable labels, dates, notes, library controls and dialog content. Game logos keep their own lettering.
- Use **Font Awesome Pro 7.3.1, Whiteboard Semibold** single-tone icons for app-owned UI controls. The jump-to-checkpoint hover uses Duotone `crosshairs`, the one exception. The app's identity icon and game artwork remain separate assets.
- Main Save is red and Load is ivory, with rounded corners and a shallow raised bottom edge. History action buttons use a compact version of this button family, subordinate to the main controls.
- Render the title bar seamlessly with the window: continue the underlying dark surfaces into it, without a separate colored strip, divider or shadow. Keep the app title quiet and the platform's window controls usable. The title bar must retain normal dragging and window behavior (see WINDOW).
- Checkpoint labels use a leading Font Awesome tag icon inside an inline chip. Saved chips are editable; Loaded and Reverted chips are read-only. Other text editable in place uses an always-visible trailing pencil within the same edit target. Ordinary form fields need neither affordance.

### Main-window visual mockup

The implemented UI in [`apps/ui/`](apps/ui/) is the visual reference for the main window.

This is the visual reference for the main window and its button family. It uses the app's actual identity icon and locally cached Steam artwork for Void War, XCOM 2 and Noita. The text mockups below describe the other window states and control placement. The behavior sections remain authoritative for interaction details.

- **Spacing:** use one base unit, `u = 8 px`. Sidebar content and the main view each have 16 px side padding. History rows have 8 px inner padding and 8 px gaps. Section gaps are 16 px. Keep zero internal padding on game artwork and keep selection inside the card bounds.
- **Shared alignment:** keep the 56 px title bar and 40 px heading/hotkey band, with no extra top spacer. Game cards and main actions share a 96 px outer height. The installed panel begins 16 px below the running card. The sidebar is approximately 248 px wide, with 216 px cards. Day headings align with `INSTALLED`, and the history scrolls below the fixed title and action area.
- **Body typography:** Inter at 16 px with a 24 px line height. The separate time column uses 13 px tabular numerals; notes and the Load badge use 12 px, and library controls use 14 px / 24 px. Checkpoint names use white 13 px / 24 px text in an inline tag chip after the event word, with no em dash. Relative ages use 11 px / 16 px text beneath exact times, updating in 5-second steps below a minute. Day headings use centered 12 px / 600 uppercase text with a quieter date; older headings show only `yyyy-MM-dd`. Do not adjust letter spacing.
- **App identity:** show `assets/icon.svg` at 40 px, 2 px from `SaveScummer` in 25 px Chakra Petch 700. Color `Save` `#faf8ee` and `Scummer` `#ff1e29`, with a 2.5 px `#0d0707` stroke painted behind the fill. Use actual game artwork and lettering in the sidebar; initials are only the missing-art fallback.
- **Stopped games:** unselected installed cards use 95% scale and `grayscale(1) sepia(.55) brightness(.62)`. Hover, keyboard focus and selection restore 100% scale, and selection restores full color. The running state is shown by the pulsing red dot before `RUNNING`, not by a card marker.
- **Surfaces:** the window is `#1c1918`, with no separate sidebar fill. The `RUNNING` heading has a `#2c2623` tint that fades down the card. The `INSTALLED` panel is `#2c2623` with 12 px corners. History boxes are `#221d1b`, without borders, with 8 px corners and 8 px gaps.
- **Save:** red face (`#f51e2b`), ivory icon and label, approximately 12 px corners and a 4 px darker bottom edge. **Load:** ivory face (`#f3eee7`), dark icon and label, matching corners and a muted brown bottom edge. Both controls are 96 px tall and equal in width, matching the game cards. Include the raised edge inside these bounds so it does not alter alignment or consume the next section's gap.
- **Action typography:** bold Chakra Petch, approximately 36 px for the main labels, with 32 px Font Awesome icons and a 16 px gap. Keep the two icon-and-title groups centered at the same height. Load's separate ivory badge hangs from the button's bottom edge without shifting its icon or title.
- **Shortcut tabs:** brown keycaps centered behind the buttons, with rounded top corners and 18 px semibold Chakra Petch text on a 24 px line. Keep the 40 px band aligned with the first sidebar heading. The text sits 4 px from the tab top. Lower the tabs by 8 px so they extend behind the button faces; pressing a button lowers its face 4 px while retaining overlap. Keep the key text fully above the face.
- **More:** a separate 48 px square brown raised button with an ellipsis, vertically centered beside the main buttons. Reserve its width before dividing the remaining space equally between Save and Load.
- **History buttons:** Load and Revert share a 104 × 30 px footprint and use visible 12 px Chakra Petch labels with the same Whiteboard `rotate-left` icon. Delete uses an unfilled target separated by 8 px. Reserve a 144 px minimum action column that grows leftward for the delete countdown. Rows have a 56 px minimum height with vertically centered contents; the age sits under the time when present, and the time centers when absent. Loaded notes begin at the event icon's left edge.
- **Library items:** game artwork fills the card edge to edge, clipped to its rounded corners, with zero internal padding and no surrounding artwork mat. Draw the selected border over the artwork inside the existing card bounds; selection must not grow the card, move the artwork or protrude into its surrounding spacing. Keep the 8 px gaps between cards. Library footer controls and popup commands share the same row geometry described below.
- **Shared icon rows:** bottom-left library controls and popup commands use the same component geometry: 8 px padding on all sides, a 24 × 24 px Font Awesome icon box, an 8 px icon-to-label gap, and 14 px Inter text on a 24 px line. The resulting row is 40 px tall (`8 + 24 + 8`). Both containers have 8 px outer padding, so icons begin 16 px from the container edge and text begins 48 px from it, excluding the border. Center each SVG in that same 24 px box; do not size footer icons independently from popup icons. Consecutive rows have no extra margin.
- **States:** a small brightness change on hover; the face moves down and the raised edge shrinks while pressed. Busy rotates the initiating button's `arrows-rotate` icon and changes its label to SAVING, LOADING or REVERTING, then briefly shows a check on success without changing its footprint. Respect reduced-motion preferences.

Use Whiteboard Semibold single-tone icons for app-owned controls. Load and Revert use the same `rotate-left` icon at the same size; their labels distinguish the actions.

| Control or state | Font Awesome icon |
| --- | --- |
| Save | `fa-flag` |
| Load, Revert, Loaded and Reverted rows | `rotate-left` |
| More options | `fa-ellipsis` |
| Checkpoint label / confirm edit | `fa-tag` / `fa-check` |
| Other editable text | `fa-pencil` |
| Delete / Flush checkpoints | `trash` |
| Add custom game | `fa-plus` |
| Scan for games / busy | `arrows-rotate` (rotate with CSS while busy) |
| Settings | `fa-gear` |
| Open checkpoints folder | `folder` |
| Configure paths | `fa-sliders` |
| Expand / collapse | `angle-down` / `angle-up` |
| Success | `check` |
| Game started / closed | `circle-play` / `circle-stop` |
| Load badge time / jump to checkpoint | `clock` / Duotone `crosshairs` on hover |
| Close | `xmark` |

Native window controls keep platform rendering and behavior. Unicode symbols in the text mockups stand for these icons, not literal control glyphs.


## LAYOUTS

The layout depends on **visible games**, not on database records. Games confirmed uninstalled are hidden, and hidden records don't count. The window has three states, each growing out of the previous one.

### 1. No visible games

The main area takes the full width and there is no sidebar. It shows one compact, centered block:

```text
┌────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ SaveScummer                                                                              ─  □  ×   │
│                                                                                                    │
│                                                                                                    │
│                                                                                                    │
│                                                                                                    │
│                                                 ◇                                                  │
│                                                                                                    │
│                                      No supported games found                                      │
│                                                                                                    │
│                                        [ ⟳ Scan for games ]                                        │
│                                         + Add custom game                                          │
│                                             ⚙ Settings                                             │
│                                                                                                    │
│                                                                                                    │
│                                                                                                    │
└────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

The diamond stands for the existing app identity asset. There's no explanatory paragraph and no illustration. Scan for games comes first, then Add custom game, with Settings directly below them; Settings opens the same dialog as in the sidebar. There is no status bar in any layout.

### 2. Games visible, none running

When the first game becomes visible, Add custom game, Scan for games and Settings move from the center to the bottom of the sidebar. The sidebar slides in from the left (see MOTION).

Nothing is preselected. The sidebar shows the installed games under `INSTALLED`, even when there is only one game, and the main area stays quiet:

```text
┌────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ SaveScummer                                                                              ─  □  ×   │
│                                                                                                    │
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
│ ⟳ Scan for games       │                                                                           │
│ ⚙ Settings             │                                                                           │
└────────────────────────┴───────────────────────────────────────────────────────────────────────────┘
```

### 3. A game is running

The running game gets its own group at the top of the sidebar. A slowly pulsing red dot before `RUNNING` identifies the group; the card itself has no marker. The game is selected when nothing else is, or when the user switches to its window (see ACTIVE STACK). The running group tint fades down its card. The unbroken `INSTALLED` panel begins 16 px below it and contains the stopped games and library controls.

The main view starts with Save, Load and More. Save and Load have equal-width 96 px faces with centered icon-and-title groups. Load's separate ivory badge hangs from its lower edge. The title and actions stay fixed while history scrolls independently below them. The first day heading aligns with `INSTALLED`; each history row has an event icon, exact time, optional age, event word, optional chip and its own action controls. The history has no visible `HISTORY` title. The behavior rules below cover the other states and interactions.

## ACTIVE STACK, SELECTION AND FOCUS

The ACTIVE STACK holds the running games, ordered by when the user last switched to each game's window outside the app. The **active game** is the game the user was last in: the top of the stack or, after that game closes, still that game until another running game is in front 5 seconds or more after the close (PLAN-HOST.md, MONITOR AND ACTIVE STACK). It's the target of global hotkeys while the app is unfocused. The game **selected** in the main view can be different, because the user may be browsing another game.

The key rule: **only an external focus change moves a game up the stack and selects it.** The user has necessarily left the app to switch to the game, so the view never changes while they're using it. Starting or closing a game in the background moves it between groups but never takes over the view. A newly started game appears under `RUNNING` but doesn't jump ahead of games with a more recent focus.

The main view changes automatically only when:

- an external focus change moves a running game to the top;
- nothing is selected yet.

Consequences:

- **At startup,** select the top running game; the host restores the stack in last-focused order. If no game is running, select nothing. Don't preselect a game just because it's installed. The main view then shows only the quiet line `No known games are running.`
- **When a game closes,** the view stays. The active game stays active and moves from `RUNNING` to the library, so the user can load it before relaunching; the view moves on when another game takes over as the active game.
- **When the user picks a game in the sidebar** (running or not), only the view changes, not the stack.


## SIDEBAR

The sidebar is the game library and the main way to navigate. It stays narrow, and each game is a small wide art card, like a game in the Steam library. Why: players recognize a game by its art faster than by its name, and the art gives the app the game feel the plain list lacked.

**Cards:**

- About 216 × 96 px, filling the sidebar content width, with a small gap between cards.
- Each card has zero internal artwork padding and an 8 px gap to the next card. Its layout bounds stay identical for running, stopped, selected and hovered cards; the unselected installed artwork is drawn at 95% scale within those bounds.
- The background is the game's Steam hero art, cropped to fill the entire card, without an inset or letterbox mat.
- The game's logo sits on the left, fitted to the card height, over a dark gradient that fades out to the right. The gradient keeps light and dark logos readable on busy art.
- A slowly pulsing red dot before the `RUNNING` heading marks the running group. Cards have no running marker.
- Nothing else goes on the card: no counts, timestamps or descriptions. The one exception is the **install tag** the host supplies when the same game is installed twice (`Steam`, `GOG`): a small tag in the bottom-right corner. Why: two cards with the same art are otherwise impossible to tell apart. Games with one install never show a tag.
- The game's name is always the card's accessible name and tooltip, even when only the logo shows it. With an install tag, both include it: `Dead Cells — GOG`. Per-game dialog titles include the install tag too.

**Running state and selection:** running games keep full-color artwork. Installed cards are shown at 95% scale with `grayscale(1) sepia(.55) brightness(.62)` when unselected. Hover, keyboard focus and selection restore 100% scale; selection restores full color. Stopped games remain clickable and keyboard-selectable, and their accessible names and tooltips include `Not running`. A selected installed game has a 2 px red inset border within the unbroken installed panel. Starting or stopping a game updates this treatment without changing the user's selection.

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

**Order:** both groups are sorted by when the user last switched to the game, newest first; `RUNNING` is the ACTIVE STACK. Games never played go last in `INSTALLED`, by name. The host supplies the order (PLAN-HOST, Library order), and it survives restarts. Why: savescumming is quitting, loading and relaunching, so the game just quit moves from `RUNNING` to the top of `INSTALLED`, right where the user's eye already is, instead of dropping to wherever its name sorts. The game played last is also the one most likely wanted next time. A game starting in the background doesn't move it; only switching to it does.

**Visibility:**

- Hide confirmed-uninstalled games, whether they are known games or custom games. A custom game whose executable disappears is hidden until it comes back.
- Keep installed games that haven't produced save data yet.

**Library controls** stay anchored at the bottom of the sidebar in this order: Add custom game, Scan for games, Settings.

- **`+ Add custom game`** is for games that aren't recognized automatically or that need custom paths. Use this exact wording; "Add game" would suggest it's the normal way to add games. Custom games are kept forever, and the main window has no way to remove them. A future management screen may add that.
- **`⟳ Scan for games`** adds discovered games immediately. There's no results screen and no confirmation. The button itself cycles through these states:

  ```text
  ⟳ Scan for games → ◌ Scanning… → ✓ 3 games found | No new games → ⟳ Scan for games
  ```

  The count includes only *newly* found known games, with the singular form for one game. Scan and Add custom game behave the same in the zero-games layout, so users learn them once.

  The button is a fallback, not the normal way games appear. The host also scans in the background: when the window is shown or focused, when a store reports an install, and periodically (PLAN-HOST.md, Known games and scanning). The window reports being shown or focused to the host with its focus report; the host decides whether a scan is due.

  Background scans are **silent**: the button keeps its idle state, and newly found games simply appear in their group. Only a scan the user started drives the `Scanning…` and result states. Why: focus scans happen on nearly every alt-tab, and a button that flickers `Scanning… → No new games` each time is noise. If the user presses the button while a background scan is running, the button shows `Scanning…` until the scan they asked for finishes, and counts games found since they pressed.


- **`⚙ Settings`** is a cog-and-text button directly below Scan for games. It opens the Settings dialog for shortcuts, sound effects and launch on startup. It stays available with no selected game and while a game is busy.

## MAIN ACTIONS

There is no game name, icon or status header above the buttons. The selected sidebar card already identifies the target, and the `RUNNING` group identifies running games. Stopped games remain selectable and use the same action layout.

- **Alignment:** the action row starts at the same vertical position as the first sidebar game card, below the always-visible first group heading. The shortcut hints occupy the same band as that heading. In the default running-game view this is the selected game's card. Keep the action row in place when the user selects a lower card or scrolls the library; do not chase that card down the window.
- **Height:** Save and Load are both the same height as a sidebar game card (roughly 96 px), with matching top and bottom edges.
- **Width:** reserve a fixed-width area at the right for `···` and the gaps between controls. Divide all remaining main-view width equally between Save and Load: 50/50. They keep equal widths as the window resizes.
- **Contents:** center each button's icon-and-title group at the same height. Hang Load's separate ivory badge from its bottom edge. The badge never moves the main title and is its own interactive control.
- **Shortcut hints:** centered keycaps sit behind their buttons. Lower the tabs to overlap the button faces by 8 px at rest and 4 px when pressed; no background gap may appear. Keep the text above the faces, within the first sidebar heading's 40 px band.
- **Availability:** the host decides whether each action is available, separately from whether the game is running, and gives one reason when it isn't. The separate host `guidance` field selects the panel and its Save/Load coverage (PLAN-LOCKDOWN, LOCKDOWN PANEL). The UI does not select guidance by ranking availability reasons. Row actions combine checkpoint eligibility with the current host gate.
- The order is fixed: Save, then Load, then `···`, which is always last.
- Below the buttons and the Load caption sit the **error block**, then the **instructions block**. The error block stays until the next action or game selection and never becomes a modal dialog.

### Save

This is the main action and has the strongest emphasis. It supports the core loop: reach an important moment, save, keep playing. The new entry goes to the top of the history.

### Load

The main Load button restores the **latest retained checkpoint**. Only checkpoints the app made count; copies the user makes by hand are not checkpoints.

What Load will restore is shown in the separate ivory badge hanging on its bottom edge (see VISUAL DECISIONS). The first line shows a clock, exact local time and relative age, with a fixed 54 px slot for the age: today `12:24:03`, yesterday `Yesterday 23:20:12`, or older `2026-09-20 12:24:03`. Clicking the badge jumps to the latest checkpoint without triggering Load; hover or focus expands a long label in place and changes its tag icon to crosshairs. With no checkpoints, Load has a muted 2 px outline and no shortcut tab, and no badge: the lockdown panel covers it. Missing live game data alone does not prevent Load: a usable checkpoint can be restored when targets are valid and accessible and the other host checks pass.

If that save has a label, the badge adds a second line with a tag icon and the label, separated from the time line by a hairline. Long labels truncate in the resting badge and wrap in the expanded hover/focus state. The badge never changes the button's size or the vertical alignment of its icon and title.

A pending deletion doesn't change Load. It still targets the latest checkpoint, even if that checkpoint is counting down to deletion, and loading doesn't cancel the deletion. The host runs the two one after the other, and the latest checkpoint is recalculated only after the deletion succeeds.

Every successful Load, and every Revert, also creates a **recovery point** holding the state just before it. That is what the row's Revert restores. Load errors go to the error block.

Load always restores the whole checkpoint, every save location in it, exactly as it was: saves made after the checkpoint are removed. Why: many games continue from their newest file, so a Load that left newer saves behind would silently not restore. The removed saves are in the recovery point, and the Loaded row says how many were removed. Revert works the same way, so it is a true undo.

### `···` menu

Each command has its own Whiteboard icon. The menu contains Open checkpoints folder, Configure paths… and Flush checkpoints… in that order.

The popup is anchored to the More button, right edges aligned, with an 8 px gap below the trigger. Use a 320 px width, 8 px outer padding and 12 px corners. Reuse the library footer's icon-row geometry exactly: 40 px height, 8 px padding on all sides, a 24 × 24 px icon box, an 8 px gap, and 14 px Inter text on a 24 px line. Thus icons and labels have the same offsets from their containing surface in both places. Put a quiet separator before Flush with 8 px margin on all sides inside the padded popup. Flush's icon follows its text color. Keep the popup inside the window at smaller sizes.

- **Open checkpoints folder** opens this game's folder in the checkpoint store, where each checkpoint is an ordinary folder named by its time and kind. Why here and not on history rows: rows already carry Load, Revert and Delete, and a fourth icon on thousands of rows is noise; the folder names make a checkpoint easy to find.
- There is no command to open the save location. Why: a save set can span several folders, so it would need a submenu and rules for patterns and shared folders, for little gain. Configure paths shows every location's path.
- **Configure paths…**
- **Flush checkpoints…**, the name used for this action everywhere. The menu item shows the size of what Flush would delete, as the host reports it: `Flush checkpoints (23 MB)…`. Why: the size is often the reason to flush. With nothing to flush the item is disabled and shows no size; when the size is unknown (the store isn't connected) it shows none either.

Sizes are rounded: one decimal under 10, whole numbers from 10 up (`840 KB`, `23 MB`, `2.4 GB`, `12 GB`), with a space before the unit. Units follow the OS file manager: 1024-based on Windows, 1000-based on macOS and Linux. Why: the number matches what the user sees when they check the folder.

App-wide preferences belong to the Settings dialog, opened by `⚙ Settings` below Scan for games in the sidebar.

### Busy state

While an operation is checking or running, controls follow the host's action gates. Save, Load, Restore, Retry, Flush and Configure cannot compete for the same game. Delete can begin a countdown while another operation runs; actual deletion waits for ownership. Labels remain editable. The sidebar and other games stay fully usable. A deletion countdown that is still pending does not make the game busy, and its Cancel stays usable even while another operation runs.

Lockdowns (the game running, no saves, a save location problem and the rest) aren't busy states: the lockdown panel covers the buttons, with no wait cursor (PLAN-LOCKDOWN, LOCKDOWN PANEL).

The control that started a Save, Load or Revert shows a rotating `arrows-rotate` icon and a progressive label (`SAVING`, `LOADING`, or `REVERTING`), whether it's a main or history-row button. There is no progress bar or percentage. The cursor is `wait` over the window while busy. On success, the control briefly shows a check and becomes clickable again. There is no success dialog. Retry shows `Recovering…` in the stable recovery panel and follows its tracked outcome. Save always creates a new deliberate checkpoint; there is no content-equality refusal.


## INSTRUCTIONS BLOCK

A game's catalog instructions (`info`, read-only) appear between the error block and the history. The block is left out entirely when the game has none.

- **Collapsed:** about 100 px tall, fading to transparent at the bottom.
- **Expanded:** grows in place to show the full text, with compact paragraph and list spacing.
- A dedicated chevron at the edge switches between the two. Clicks inside the text never toggle the block. The choice is remembered per game for the session.
- The Save and Load procedures are numbered separately, each starting at 1.


## HISTORY

The history is a vertical, newest-first, effectively unlimited activity log. It scrolls independently below the fixed title and action area, with a 40 px fade only at an edge that can scroll farther. Each entry is a compact borderless `#221d1b` box with an 8 px radius and an 8 px gap. Start directly with the first date heading; do not render a `HISTORY` title, zebra striping, timeline connector or timeline points.

**Empty history** shows one quiet line: `Saves will appear here.`

### Rows

Each row uses three aligned columns: time, description and actions.

1. **The time** sits in a fixed left column and shows the event's exact local `HH:mm:ss` in tabular numerals. The date appears only in the section heading. Beneath the time, show a quiet relative age only when the event belongs to Today or Yesterday and is less than 24 hours old: `5s ago`, `35m ago`, `2h ago`. Under a minute, floor the age to 5-second steps; then floor minutes below an hour and hours below 24 hours. Future events and events aged 24 hours or more have no relative age. The time centers vertically when no age is shown; rows keep a 56 px minimum height.
2. **The description** occupies the flexible middle column. Every row has an 18 px leading event icon in `#cfc4b8`, followed by a 16 px event word. Action rows have an inline chip with a leading `tag` icon and smaller 13 px white text: `Saved [L Add label…]`, `Loaded [L My label]`. There is no em dash. Saved chips are editable; Loaded and Reverted chips link to the checkpoint they refer to and flash it. Do not add a trailing pencil or a separate checkpoint-name line.
3. **A small action button** sits in the right column, at least 144 px wide, aligned vertically with the time and description: `↶ LOAD` for a saved checkpoint and `↶ REVERT` for a recovery point. Both buttons have the same 104 × 30 px footprint, followed by Delete. Use a Font Awesome icon and a visible text label. The quieter Delete button follows it, always last. The column grows leftward for deletion states. Rows without actions keep the column empty, without placeholder controls.
4. **Load notes**, when applicable, occupy a reserved line below the event/chip line, starting at the leading event icon's left edge. Non-actionable entries show their time, icon and description, with no chip or action placeholders.

Details that are easy to get wrong:

- Use the same font size for day headings and main descriptions. Times, section dates and notes use smaller, quieter Inter text with readable contrast.
- Keep day headings for scanning: `Today 2026-09-27`, `Yesterday 2026-09-26`, the full localized weekday plus date for recent days, and `yyyy-MM-dd` alone for older dates. Use centered 12 px/600 uppercase headings aligned with `INSTALLED`; the date is quieter. Show the date once per section, never in its event rows. Retain full timestamps in the data and associate each row with its section heading for accessibility.
- Give every entry the same borderless background, 8 px radius, 8 px inner padding and 8 px gap. Day headings remain outside the boxes. Virtualization must preserve these dimensions.
- The time column fits all eight characters without truncation. Exact event times never change as entries age; only the optional age line updates. Long chips truncate before the fixed action column while keeping the tag icon visible. Labels remain white in every state. Separate dates, labels and times with spacing; do not insert decorative middle dots.
- Only the row of the latest checkpoint, the one the main Load restores, keeps full contrast and the hover background at rest; every other row is muted and gets the background only on hover. Only a deliberate Save produces that checkpoint: Loaded and Reverted rows, and their recovery points, are never highlighted, even when they are newer.
- At local midnight, recalculate the day groups without moving the scroll position or resizing rows.
- User-created Save, Load, row Load and Revert entries slide in and glow red for about 3 seconds, and the history scrolls to the top. Background entries and midnight regrouping preserve the user's scroll position.

### Event kinds

| Event | Inline chip | Weight | Actions |
| --- | --- | --- | --- |
| Saved | Tag icon plus editable label or `Add label…` | Significant | Load this save, Delete |
| Loaded | Tag icon plus read-only label or time of the save it loaded | Significant | Revert this load, Delete |
| Reverted | Tag icon plus read-only time of the row it reverted | Significant | Revert this revert, Delete |
| Game started | None | Light | None |
| Game closed | None | Light | None |

The event words and their leading icons distinguish the row kinds. Started and closed rows use the same white event text as other rows and make play sessions visible without a separate session UI; both are plain markers with no checkpoint actions. An actionable row temporarily disabled while the game is busy keeps its buttons in place.

- **Saved** can carry a label (see below).
- **Loaded** names the save it loaded in the inline chip after `Loaded`, using the checkpoint label or exact `HH:mm:ss` when unnamed. It follows the label live and keeps it after that save is deleted. Truncate long labels with `…` and show the full label in the tooltip. Clicking the chip jumps to and flashes that checkpoint when it remains available.
- **A Loaded row's reserved note line, below the event and chip,** carries short notes when the host reports them. The host owns these facts; the UI only shows them:
    - what the Load removed: `Removed 2 newer saves, kept in the recovery point`;
    - that Steam Cloud undid part of it: `Steam Cloud replaced the restored save`. The host can only tell at the next game launch after the Load, so this note appears later, in place. Why: without it the user would think Load failed, or not notice that the game started from a different save.

  Two notes share the line, separated by a semicolon and a space. The line is always there, so a note arriving later never resizes the row.
- **Load this save** loads *that* save instead of the latest one. It uses the same Whiteboard `rotate-left` icon as Revert, at the same size; the button labels distinguish the two operations.
- **Revert this load** restores that load's recovery point, which is the state just before the load. Like a Load, it first keeps the current state as a new recovery point, so a revert never loses progress. The result is a `Reverted` row with its own Revert, which undoes the revert. Nothing is used up: the loaded row keeps its Revert too. Why: one rule for every restore is easy to trust, and any state the user leaves can be brought back.
- Revert changes game data; Delete destroys a checkpoint, so they must not get equal emphasis. Delete stays quiet until the row is hovered, focused or selected.

### Labels

A save's label is an inline chip after `Saved`, for example `[L Before boss fight]`, with white 13 px Inter text and a leading Font Awesome tag icon. The chip is one keyboard-accessible edit target named `Edit checkpoint label`. It has no trailing pencil. Editing replaces the chip in place without moving the event, time or actions.

A new save has no label. Its chip shows the same white `Add label…` placeholder and leading tag icon; hovering highlights the editable area:

```text
12:24:03  ⚑ Saved [L Add label…]                           [↶ LOAD] [del]
2s ago
```

Clicking the chip or activating it by keyboard replaces it with a text field and inline check button (`✓`). The time column, event and row actions stay in place. The check confirms the label without adding another button named Save.

```text
12:24:03  ⚑ Saved [Before boss fight_       ] [✓]           [↶ LOAD] [del]
2s ago
```

Changes save automatically, so the user never has to remember to confirm. A save happens on any of these:

- a short pause after typing, restarted by every keypress;
- the field losing focus;
- pressing the check button, or Enter.

Clearing the text removes the label and restores `[L Add label…]`. Finishing an edit restores the leading tag icon and white label. Editing a label changes no game files and adds no history event.

- **One line, up to 100 characters.** The field stops accepting text at the limit, and pasted line breaks become spaces. Leading and trailing spaces are dropped, so a label of only spaces is the same as none.
- **Editing works while the game is busy** and while the save is counting down to deletion, because it touches no files.
- **The label belongs to the save, not to the row.** Every place that names the save shows it: this row, the Loaded rows that loaded it, the Load button and the Flush dialog's Details.
- **Escape cancels the edit.** It puts back the label from before editing started and closes the field, undoing anything autosave already stored during this edit. Why: autosave is there so the user never has to confirm, not so a slip can't be taken back.
- **If the save disappears while its label is being edited** (deleted, flushed or changed outside the app), the edit is dropped along with the row, with no error.

Only Saved chips are editable. Loaded and Reverted chips are read-only links to their referenced checkpoints; they scroll only when the target is not fully visible and flash it. Their tag icon becomes crosshairs on hover. Both show the event time on the left. Loaded notes start beneath the event icon.

### Deleting a single entry

Delete is deliberately not immediate. Deleting a saved row removes the checkpoint. Deleting a loaded or reverted row removes its recovery point, and then the whole row. Pressing the trash button turns the row's action buttons into a countdown. It takes the buttons' place and grows leftwards from the row's right edge. The rest of the row stays where it is. Cancel brings the normal buttons back.

```text
Normal
12:24:03  ⚑ Saved [L Before the station]                    [↶ LOAD] [del]
2s ago

Counting down (3 → 2 → 1, updated in place)
12:24:03  ⚑ Saved [L Before the station]              Deleting in 3 [Cancel]
2s ago

Countdown over, game busy
12:24:03  ⚑ Saved [L Before the station]           Waiting to delete… [Cancel]
2s ago

Deleting (no Cancel)
12:24:03  ⚑ Saved [L Before the station]                       ◌ Deleting…
2s ago
```

Loaded and reverted rows work the same way:

```text
11:18:44  ↶ Loaded [L 10:47:10]                          [↶ REVERT] [del]
1h ago

11:18:44  ↶ Loaded [L 10:47:10]                     Deleting in 3 [Cancel]
1h ago
```

Why a countdown instead of a dialog: deleting a checkpoint should be quick, and a mistake needs to be undoable. A confirmation dialog repeated on every row gets clicked through without reading.

The host owns the countdown. The UI only sends delete and cancel requests and displays the host's state, and its local timer never triggers anything. This is a small in-memory collection in the host, not a job framework or a persistent queue. The core delete operation knows nothing about countdowns.

- **Countdowns are independent.** Each has its own 3-second deadline and Cancel. The same entry never gets a second deletion request.
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

The current shortcuts appear on tabs behind their corresponding main buttons, lowered to overlap the faces by 8 px at rest and 4 px when pressed. The text stays above the faces in the first sidebar heading's band. There is no bottom/status bar. Accepted changes in Settings update the hints. Use platform key names: `Ctrl+F5` / `Ctrl+F9` on Windows and `⌥F5` / `⌥F9` on macOS.

**Which game the hotkeys act on:**

- **When the app is focused,** they act on the *selected* game, even a stopped one.
- **When the app is unfocused,** they act on the active game, even one that just closed.
- **With no target,** they do nothing. With no selected game there are no main buttons or hints; Settings remains accessible.
- While the target game is busy, conflicting operations and their hints appear unavailable. A pending deletion countdown alone changes neither whether the hotkeys work nor which game they target.
- While a shortcut field is capturing a replacement in Settings, those keypresses are input to the field and must not trigger Save or Load.

## DIALOGS

There are four dialogs: Settings, Add custom game, Configure paths and Flush checkpoints. They open inside the main window, one at a time, over a `rgba(12,10,9,.62)` scrim. The dialog surface is `#2a2420` with a 1 px `#3d342d` border and 12 px corners; a hairline separates its 15 px/600 title row from the body. The scrim fades in and the dialog rises slightly with CSS, with an instant reduced-motion state.

- Settings is 480 px wide, Flush 560 px, and Add custom game and Configure paths 680 px. Each fits within the window with 24 px minimum clearance on every side; its body scrolls if its content is taller than the available space. The dialogs cannot be resized separately.
- Labels occupy a 128 px column. Fields are 32 px tall on `#1c1918`, with a red focus border. Inline errors appear at 12 px in `#ff7a80` beneath their fields. The save-location hint stays visible above its error.
- Buttons contain text only. Save and Add are ivory (`#f3eee7`) with dark text; Cancel, Browse, Open and Reset are brown (`#463830`); Flush is red (`#f51e2b`). Cancel is the default action in the Flush dialog.
- Use accessible web dialog and form behavior: focus moves into the dialog and returns to its opener; keyboard navigation stays within the open dialog; Escape, × and Cancel close it. Enter submits the active form's default action, while an explicitly focused button keeps its own action. Do not add global Enter handling that overrides form or button semantics.

The title of a per-game dialog includes the game's name (`Configure paths — Void War`), so the target stays explicit even when the sidebar is covered. Include the install tag when needed to distinguish installs.

Deleting a single checkpoint is not a dialog; it uses the row countdown above.

### Settings

Opened by the cog-and-text `⚙ Settings` button directly below Scan for games, including in the zero-games layout.

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
- Show an inline error for a non-function shortcut without a modifier, a duplicate of the other shortcut, or a system-reserved combination. The host remains the final authority on whether registration succeeds.
- `Play sounds` enables or disables sound effects; `Launch on startup` enables or disables starting the host at sign-in. These preferences appear only here, not in the main window.
- Apply changes on **Save**, the default button, after the host validates and accepts them. Cancel or closing the dialog discards unapplied edits. Accepted settings persist across restarts; shortcut hints update immediately.
- A duplicate, unsupported or unavailable shortcut shows an inline error beside its field. Keep the dialog and entered values open for correction; rejected changes leave the saved configuration and previous bindings intact.
- The host owns shortcut registration, persistence, sound playback and startup integration. The UI saves shortcut changes through the host's settings contract; it does not register a second independent set of global hotkeys.

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
│ Configure paths — Slay the Spire                                                 × │
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
│ Configure paths — Starsector                                                     × │
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
- **Wait for the game to close before saving or loading**, a checkbox, on by default, below the paths. Below it, one line of yellow description text: most games write progress to disk only on Save & Quit, and this makes sure it's there before a checkpoint is made or loaded.
- Changes are applied only after the host validates them. If validation fails, the error appears in the dialog like in Add custom game, the saved configuration doesn't change, and the entered values stay for the user to fix.
- **Save** is the default button.

### Flush checkpoints

This is the only bulk delete, so it shows the user what will go before they confirm:

```text
┌────────────────────────────────────────────────────────────────────────┐
│ Flush checkpoints — Void War                                         × │
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

The Details list uses a scrollable inset `#1c1918` box, grouped by saved backups, recovery points and incomplete copies. Labels appear beside their checkpoint folders. After a successful Flush, the `···` item has no size and is disabled until the game has data to flush again.

- **When it's available:** there are saved checkpoints, recovery checkpoints or history entries, and the game is neither busy nor waiting for recovery. Recovery data left by an interrupted operation is included only after that interruption is resolved.
- **The dialog is only a preview.** Before opening it, the app checks which files would be affected and shows them. Nothing from the preview is passed back to the host. Why: simplicity. The dialog tells the user what the action does; it isn't a contract.
- **Text:** "Permanently delete all backups and clear this game's history?" followed by "Your current game data will be kept."
- **Counts:** separate counts for saved backups, recovery points and incomplete copies, with zero counts left out, then the total size, the same number as the menu item, rounded the same way. One total, not a size per kind: the menu brought the user in with the size, and the dialog confirms it. Left out when the size is unknown. The paths, all inside the checkpoint folder, go under **Details**, loaded in pages. The host names checkpoint folders by time and kind, inside a folder per game. A labeled saved backup shows its label after the path, shortened with `…` when needed, so the user can recognize saves they'd miss.
- **On confirm:** the dialog closes and the UI sends a plain Flush request for the game. The host does the whole job again from scratch: it deletes every saved and recovery checkpoint of the game and clears the history. The game's save locations are not touched. Only the records whose files were actually deleted are cleared. While it runs, the game is in the normal busy state, and any failures go to the error block.
- **Cancel** is the default button.


## MOTION

Motion clarifies structural changes and confirms meaningful actions. It never carries information on its own.

- Animations run only when the system hasn't asked for reduced motion (the equivalent of `prefers-reduced-motion: no-preference`). Otherwise, changes happen instantly.
- Animation is CSS keyframes or view transitions. Script only starts a view transition and scrolls; the root transition is disabled and transition pseudo-elements ignore pointer events.
- Switching games animates only the history, up or down according to the new game's sidebar position.
- When the zero-games layout changes to the sidebar layout, or back, the sidebar slides in or out clipped to the window and the main area settles into its new width. With reduced motion, the layout changes instantly.
- Short success flashes on buttons are fine, but the success state must be clear without them.
- The slow red pulse before `RUNNING` is the only looping animation; disable it with reduced motion.


## WINDOW

The title bar blends into the content surfaces with no horizontal separator or distinct title-bar fill. Keep a usable drag region clear of controls, preserve native minimize/maximize/close behavior and platform conventions, and ensure the seamless treatment works in both focused and unfocused states. The app title uses Chakra Petch as specified above.

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
- each history time column fits `HH:mm:ss` without truncation or wrapping;
- the small labeled row actions are reachable and aligned with their descriptions, and editable labels retain their leading tag icons;
- equal-width action buttons leave room for Load's separate badge; shortcut tabs overlap behind the faces while their text remains unobscured;
- Add custom game, Scan for games and Settings stay reachable.

There is no separate narrow layout.

Accessibility: icon-only controls (`···`, history Delete buttons and label check buttons) need accessible names and tooltips. History Load and Revert buttons have visible text labels and accessible names identifying their targets. Each editable checkpoint chip is one keyboard-accessible edit target; its decorative tag icon is hidden from assistive technology. Keyboard focus follows visual order, and destructive actions stay reachable by keyboard. Non-actionable history entries have no button focus stops.


## TESTING

For the Tauri UI, test React views against a fake typed bridge that can simulate busy, failing, missing snapshots, pending deletions and a disconnected host. Test the Rust bridge against a fake protocol server, including request/response matching, pushed events, host restart and version mismatch. A packaged-window smoke test checks launch, close, reopen, focus and local asset loading on each supported WebView engine. The UI behavior tests focus on the rules that are easy to break:

- **Presentation:** compact 56 px title bar, no extra top padding before the 40 px hotkey band, and matching 16 px section gaps; Installed/day headings align; selected borders and raised edges stay inside control bounds; 16 px side gutters and 8 px row padding; footer and popup commands have identical 24 px icons, 8 px insets and gaps, and 40 px heights; every history row has consistent action-column geometry; Chakra Petch on the title, main controls, shortcut hints and history Load/Revert buttons; Inter on content and dates; Whiteboard Semibold icons with the Duotone crosshairs exception; seamless title bar with usable native controls and dragging in focused and unfocused states.
- **Sidebar:**
  - card art fallbacks: no logo, no hero art, no art at all, and custom games;
  - the pulsing `RUNNING` dot, including a library where every game is running, and no card running marker;
  - the install tag on the card, tooltip and per-game dialog title, only for games installed twice;
    - headings for every non-empty group, including one game, all running and none running; no heading for an empty group;
    - hidden uninstalled games;
    - stopped games kept in the host's order, running games in stack order;
    - the switch to the zero-games layout based on *visible* games;
    - first-card/button alignment stays unchanged when the library moves between one and two non-empty groups.
- **Selection:**
    - external focus moves a game up and selects it;
    - a manual selection is kept when games start or close;
    - the view and the active game staying put when the active game closes.
- **Scan:** the zero, singular and plural messages, counting only newly found known games; background scans leave the button idle while their games appear; pressing Scan during a background scan shows `Scanning…` until the requested scan finishes; showing or focusing the window sends the focus report.
- **Actions:** game identity and running state appear only in the sidebar; Save and Load availability still comes from the host (no game data, no checkpoints). Verify card-height buttons, equal widths after reserving `···` and gaps, centered icon/title groups, the independent hanging Load badge, its jump/expand behavior, and the disabled no-saves outline; shortcut tabs overlap behind the buttons without exposed gaps or hidden text, including at minimum window size. Busy controls show the progressive label and become clickable again with the check. Open next to the executable is disabled while the field is edited. The Flush item's size: rounding at each boundary, the OS's units, and no size when there's nothing to flush or the size is unknown; the dialog's total matching it.
- **History rows:**
    - no visible History title; 18 px leading event icons, fixed-width time column, flexible description column and small labeled Load/Revert buttons aligned at the right; event rows show exact `HH:mm:ss` with optional relative age beneath, and dates only in section headings;
    - non-actionable entries have no buttons or disabled button placeholders; temporarily busy actionable entries keep their disabled buttons;
    - ages show 5-second steps below 60, floored minutes below one hour and floored hours below 24 hours; test Today and Yesterday, 59 seconds, 60 seconds, 59 minutes, 60 minutes, just under 24 hours, exactly 24 hours and future timestamps; older/future events have no age, and changes never resize rows;
    - uniform rounded boxes, 8 px inner padding, 8 px radius and 8 px gaps stay consistent during virtualization; no zebra striping or timeline decorations;
    - Load and Revert use the identical Whiteboard `rotate-left` icon; busy and success states preserve their footprints;
    - Reverted rows that can themselves be reverted and deleted;
    - a Loaded row's notes: removed newer saves, Steam Cloud replacing a restored save, both together, and the Steam Cloud note arriving later without resizing the row.
    - user-triggered rows slide in and glow while scrolling history to the top; background rows and midnight regrouping preserve the scroll position; edge fades appear only where more history can be scrolled into view;
    - Loaded and Reverted chips jump to and flash their referenced checkpoint, scrolling only when needed; histories and empty states stay independent per game;
- **Labels:**
    - event and white 13 px tag chip share one line without an em dash; no trailing pencil or decorative middle dots; full labels appear in tooltips and truncate before actions;
    - clicking or keyboard-activating a Saved chip opens its inline editor and check button; confirming restores the leading tag without moving event, time or actions; Loaded/Reverted chips stay read-only;
    - other text editable in place uses a trailing pencil; read-only label copies and non-actionable entries have none;
    - the autosave pause restarting on every keypress, and saving on focus loss, on the check button and on Enter;
    - clearing a label, a spaces-only label, the 100-character limit and pasted line breaks;
    - a label shown on Loaded rows, the Load button and Flush Details, updating live, with `…` and a tooltip when too long;
    - a Loaded row falling back to the time when the save has no label, and keeping the label after the save is deleted;
    - editing while the game is busy, and the save disappearing mid-edit;
    - Escape restoring the label from before the edit, even after an autosave.
- **Deletion countdowns:**
    - each has a 3-second `3 → 2 → 1` sequence and runs independently;
    - Cancel works, including while the game is busy;
    - the waiting and deleting states;
    - the main Load button doesn't change;
    - rows are rebuilt after navigating away or reconnecting;
    - a failure restores the row with no retry.
- **Midnight:** rows regroup without moving the scroll position.
- **Dialogs:**
    - the four in-window dialogs use the scrim, surface, widths, 128 px label column, field focus/error styling and text-only button treatments described above; a tall Details list scrolls within the dialog;
    - opening a dialog moves focus inside, Tab stays within it, Escape/×/Cancel close it and return focus to its opener, and Enter follows the active form or focused button; reduced motion removes the entrance animation;
    - Settings opens below Scan for games in all layouts; there is no status bar;
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
