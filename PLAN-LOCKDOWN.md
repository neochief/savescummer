# Save Scummer Lockdown

Status: reviewed. The exit lock (EXIT LOCK) is implemented; everything else here is decided but not built yet: Retry locked while running, the lockdown panel, Start the game, the expert-mode text, exit checkpoints, shared folders, the manifest with the Save and Load locks it drives, and Steam's own file excluded from every save set.

A game's Save and Load can be impossible for a reason that belongs to the game: it's running and hasn't written its progress yet, it has no saves yet, or where it keeps them is unknown, invalid, unreadable or not allowed. This plan defines those **lockdowns**: when each applies, which one wins, what the user sees, and what they can do about it. It also defines the checkpoint the host keeps when a game exits.

- Host behavior and the protocol: [PLAN-HOST.md](PLAN-HOST.md).
- The main window: [PLAN-UI.md](PLAN-UI.md).
- Failures of operations that did start: [PLAN-ERRORS.md](PLAN-ERRORS.md).


## TERMS

- **Lockdown** — a state of one game in which Save, Load, or both are refused before anything is created, for a reason the host reports as the action's availability reason.
- **Exit lock** — the lockdown of a running game whose progress reaches the disk only when it exits (`game_running`).
- **Expert mode** — a game with the exit lock turned off: Save and Load work while it runs.
- **Deliberate checkpoint** — one the user made with Save (kind `saved`). The only kind Load and the Load hotkey restore.
- **Automatic checkpoint** — one the host made on its own: a recovery point before a Load or Revert (`recovery`), or an exit checkpoint (`closed`). Restored only from its own history row.


## WHEN IS A GAME'S STATE ON DISK

Most games keep progress in memory and write it only when the player saves and exits. FTL writes its run (`continue.sav`) only then, and deletes it when the run ends. A Save while such a game runs copies stale files; a Load is overwritten when the game exits.

Once the game's process has exited, what's on disk is what it wrote, for practical purposes:

- **Delayed writes don't matter.** A write lands in the OS file cache at once and reaches the physical disk later, but every process on the machine, the host included, reads through that same cache. So the host sees every write the game made, flushed or not. The same holds for memory-mapped files on Windows, macOS and Linux. Only a power cut loses unflushed data, and it would lose the game's own copy too.
- **An exited process writes nothing more.** Its files are closed; nothing of it is in flight.

What remains uncertain isn't the file system:

1. **The game didn't save.** Quit without saving, a crash, or a kill mid-write leaves older or broken files. The host can't tell a clean exit from a crash: only a process's parent gets its exit status, and the host doesn't start games.
2. **The wrong process.** The monitor watches the catalog's executables. A game that runs behind a launcher, or leaves a helper writing after its main process exits, could look exited too early. Such games need a catalog fix naming the right executable.
3. **Something else writes later.** A cloud-synced folder (iCloud Drive, OneDrive) may replace a file after the exit. Steam mostly uploads at exit and downloads at the next launch; the existing Steam Cloud check at the next start notes a replaced file on the Loaded row.
4. **Network drives** may show changes late. Rare for saves.

So the exit lock lifts the moment the process is gone, with no settling delay: waiting longer wouldn't fix any of the four, and would only slow down the common case.


## EXIT LOCK

Implemented. Host: PLAN-HOST, OPERATIONS, *Games that write their progress on exit*.

- **The setting.** Every game has *wait for the game to close before saving or loading*, on by default, stored on the game's record only when it's off. Configure shows it as a checkbox with yellow text under it (Expert mode below). The CLI sets it with `configure <game> --wait-for-exit on|off`.
- **The rule.** With the setting on and the game on the ACTIVE STACK, the host refuses Save, Load and Revert with `game_running` before anything is created, from every entry point: UI, CLI and hotkeys. Save and Load availability report the same reason.
- **Hotkeys.** A press while the game runs gets the busy cue and a notification (`Save and exit FTL to save or load checkpoints.`), since the player is in the game and sees no window. Nothing is queued. Why not queue it to run at the exit: the player would be committed to a Save or Load they may have forgotten about, at a moment they don't choose.
- **Retry is locked too** (not built yet). Retry runs the recovery rules on the live save files, which a running game may overwrite at the exit, just like a Load.
- **Unaffected:** Delete, Flush, labels and Configure. They don't touch the game's files.
- **When it lifts.** As soon as the game leaves the ACTIVE STACK (the monitor polls every 250 ms). With exit checkpoints on, the closing lockdown follows until the exit checkpoint is recorded (EXIT CHECKPOINTS); then everything works.
- **A game waiting for macOS access** is never on the ACTIVE STACK, so it isn't exit-locked; its own lockdown (`access_needed`) applies instead.

### Expert mode

Some games write every change to disk as it happens, and saving mid-game is safe for them. Turning the exit lock off is the expert switch: a casual user saving or loading at the wrong moment can capture a half-written save, or lose a Load when the game overwrites it at the exit.

- Per game, never app-wide. Why: whether it's safe depends on the game.
- A Load while the game holds a save file open is still refused (`held_open` on macOS and Linux, `in_use` on Windows), after about a second of waiting (PLAN-HOST, *Interference from the running game*). This safety net only matters in expert mode now.
- The yellow text under the checkbox, decided, replacing today's short line: *Depending on the game, there may be ways to force it to write its progress to disk, so we can save it, and ways to force it to read the saved progress back, so we can load. It doesn't work for every game, and it takes intimate knowledge of the game and real save-scumming skills. Untick this only if you know when the game saves and loads its progress on disk.*
- Nothing more for now: no tag, no per-game instructions. A real expert mode needs knowledge of each game, which the catalog doesn't have yet.


## LOCKDOWN PANEL

One container replaces today's mix of disabled buttons, tooltips and error blocks for every lockdown.

### The container

- **Where:** over the Save and Load buttons, lined up with the first game card, like the exit-lock panel today. It covers both buttons (full width) or only the one that's locked (Save or Load), leaving the other usable.
- **Look:** an info panel, not a warning: the regular surface color and border color, with the current 2 px border width and padding. No warning colors or icons by default.
- **Content, centered:** a large title, a smaller subtitle, one line of regular text, and at most one button.
- **Buttons behind it** are blurred and unclickable. No wait cursor: nothing is working.
- **The history's Load and Revert** are hidden, keeping their space, for the lockdowns that refuse them too (1–6). The others leave them: a row's Load of an older checkpoint still changes the files.
- **One at a time,** by the priority below. It hides as soon as the host stops reporting the reason.

### States

In priority order: the first that applies is shown.

| # | Reason | Covers | Title / subtitle | Text | Button |
|---|---|---|---|---|---|
| 1 | `access_needed` | both | **Allow access** / to this game's saves | macOS asks before SaveScummer can read where the game keeps its saves. | **Allow access…**; once denied, **Open System Settings** (macOS won't ask again) |
| 2 | `no_save_location` | both | **Choose where the saves are** / to save or load checkpoints | We don't know where the game keeps its saves on this computer. | **Configure…** |
| 3 | `invalid_target` | both | **Fix the save location** / to save or load checkpoints | Why it's invalid (too broad, overlapping another game, a changed link), from the host's detail. | **Configure…** |
| 4 | `target_unavailable` | both | **Save location unavailable** / to save or load checkpoints | The drive or folder holding the saves can't be read right now. | none, or **Configure…** |
| 5 | `game_running` | both | **Save and Exit the game** / to save or load any checkpoints | We can only intercept the game progress after the game saves it to disk. | none |
| 5a | `closing` | both | **Keeping the closing state** / one moment | — | none |
| 6 | `blocked` | both | **Finish the interrupted save / load** / before anything else | What was interrupted and what Retry does. | **Retry** |
| 7 | `no_game_data`, no checkpoints | both | **Play first** / to have something to save | The game hasn't written any saves yet. Make some progress, then save. | **Start the game** |
| 8 | `no_saves` | Load | **No checkpoints yet** / press Save to make one | — | none; Load keeps its muted outline behind the panel, without the `No saves yet` badge |
| 9 | `no_game_data`, with checkpoints | Save | **No game data** / to save | The game has no saves on disk now. You can still load a checkpoint. | **Start the game** |
| 10 | `unchanged` (Save) and `already_loaded` (Load): the files equal the latest deliberate checkpoint | both | **Nothing changed** / since the save at 21:06 | — | none |
| 11 | `unchanged` (Save): the files equal an older deliberate checkpoint | Save | **Nothing changed** / matches the save at 21:06 | — | none |

Why location problems (1–4) come first: they must be fixed either way, and fixing them doesn't need the game closed. Why the exit lock (5) comes before blocked (6): Retry changes the live save files, so it waits for the exit like a Load (EXIT LOCK); a blocked game that's running shows the exit lock, then Retry once it exited.

The checkpoint store being unreachable (`store_unavailable`) isn't about one game. It stays an app-wide notice rather than a panel state.

### What the buttons need

- **Allow access…** and **Retry** use the existing `request_access` and `retry` commands; the UI doesn't offer them yet. A denied request answers with the System Settings page to open.
- **Configure…** opens the Configure dialog for the game.
- **Start the game** needs a new host command, `launch { game }` (CLI `launch <game>`), available for an installed game with an executable, or with a Steam app id:
  - **Steam:** always through `steam://rungameid/<appid>`. Many Steam games refuse a direct start (they relaunch through Steam or fail its DRM check), and a direct start skips Steam Cloud's sync before launch, which is how save conflicts start. It also keeps the user's launch options and, on Linux, Proton.
  - **GOG:** the executable directly. GOG games are DRM-free, and Galaxy has no reliable launch link.
  - **Epic, later:** its launch link (`com.epicgames.launcher://apps/<appName>?action=launch`), from the app name in its install manifest; a direct start may fail its ownership check.
  - **Custom games:** the configured executable; a Mac `.app` through `open`.
  - Started from the executable's folder, which many games expect, and detached, so quitting SaveScummer never takes the game with it. The monitor sees the start as usual.


## EXIT CHECKPOINTS

When a game exits, the host keeps its closing state as an automatic checkpoint. It's the record of how the session ended, and the safety net for "I exited without saving a checkpoint first".

- **The setting:** per game, *keep a checkpoint when the game closes*, on by default, in Configure; stored on the game's record only when it's off, like the exit lock. In the protocol it's `checkpoint_on_exit` in the game summary and in Configure (CLI `configure <game> --checkpoint-on-exit on|off`).
- **Taken when:** the game leaves the ACTIVE STACK, whatever the exit lock setting. Not while it runs, and not for an exit the host didn't see (it wasn't running then).
- **Nothing is copied twice:** when the files equal an existing checkpoint, the `closed` record points at its folder (SHARED FOLDERS). That covers launching and quitting without playing.
- **Kind `closed`, treated like `recovery`:**
  - Load, the Load hotkey and the Load badge never pick it: "latest" only looks at deliberate checkpoints. Why: in a roguelike the closing state is often the dead run (FTL deletes `continue.sav` when a run ends), and a Load that restored it would defeat the whole app.
  - The **Game closed** marker becomes a normal history row: the time card, the closed icon and a Revert-style button (RESTORE) that restores it. Like every Revert, that first keeps the current state as a recovery point and adds a Reverted row pointing at the Game closed row. With the setting off it stays the small marker.
  - Delete on that row removes the exit checkpoint; the marker stays.
  - Nothing prunes exit checkpoints: only Delete and Flush remove checkpoints, as everywhere else. With shared folders for unchanged exits and small roguelike saves, the growth is modest.
- **A crash or a quit without saving** gives an older or broken closing state. That's harmless: it's an extra, clearly labelled copy that nothing restores on its own.
- **The closing lockdown.** From the exit until the exit checkpoint is recorded, Save, Load and Revert are refused with `closing`, and the panel says **Keeping the closing state** (LOCKDOWN PANEL, 5a). Why: the exit checkpoint holds the game's lock like any operation, so without it a Load pressed right after the exit would get a bare busy refusal, just after the exit lock said everything works. A hotkey press gets the busy cue, with no notification: it's over in a moment (milliseconds for FTL, seconds for large saves). Nothing is queued. With the setting off there's no exit checkpoint, so no `closing` state.
- **Failed** (the store is full or unreachable, a file can't be read): logged; the Game closed row stays the plain marker and the closing lockdown ends. No failure cue, notification or error block: the user didn't ask for it, and a real store problem shows on their next Save or Load anyway.
- **Interrupted** (the host stopped mid-copy): discarded silently at the next start, like an interrupted recovery copy. No "save was interrupted" notice: the user didn't ask for it.
- **Folder name:** `<time> closed`, next to `saved` and `recovery`, when it has its own folder.

## SHARED FOLDERS

**The host never stores the same files twice.** Before copying the save files into a new automatic checkpoint (an exit checkpoint, or a recovery point before a Load or Revert), it looks for an existing checkpoint of the game holding exactly those files (UNCHANGED SAVES). If there is one, the new record's `folder` holds that checkpoint's folder path and nothing is copied. It's an ordinary checkpoint record otherwise, so restoring, integrity checks and history follow `folder` as always. A Save never gets here: files equal to a deliberate checkpoint lock it, and files equal only to an automatic one make a real copy, since that Save is the user's own.

- **The record that made the folder owns it.** Deleting the owner removes the folder and every record pointing at it, so no record outlives its folder; the countdown shows on every row whose checkpoint shares the folder, so the user sees everything that goes. Deleting a record that only points at another's folder removes just that record. Why: an automatic exit checkpoint must never take a deliberate save with it. Flush removes all of a game's records and folders, so it needs nothing extra.
- **A shared folder changed outside the app** is handled once, for its owner, by the existing rules (PLAN-HOST, *Backups changed outside the app*); records pointing at it follow the owner's verdict, so one edit never registers several new generations.
- **Why a path in the record, not hard links or clones:** it needs no file-system support, checkpoints stay plain folders, and identical is the case that matters here. Sharing identical *files* between different checkpoints is the separate idea for very large saves (PLAN-HOST, OPERATIONS).


## UNCHANGED SAVES

The host tells whether a game's save files still equal a checkpoint without reading checkpoint folders again, from a manifest recorded when the checkpoint was made.

### The manifest

- **Recorded at copy time,** for every file the checkpoint holds: its target, relative path, size, modification time (to the second: some file systems keep coarser times) and a BLAKE3 content hash. The hash is computed as the copy streams the bytes, so it costs no extra reads.
- **A target** here is its root and filter, never its position in the save set, so a reordered or reconfigured save set can't make two different targets look the same. A checkpoint made for another save set never matches the live files. An absent target counts as empty.
- **File times are kept** by every copy, into a checkpoint and back out of it by a Load or Revert; the comparison depends on it.
- **Two digests in the checkpoint's database record,** indexed:
  - the *metadata digest*: a hash over every file's target, path, size and time;
  - the *content digest*: a hash over every file's target, path and content hash.
- **The per-file list** is kept in the record and in the checkpoint's `checkpoint.json`, so a checkpoint describes itself (files on disk are the truth).
- **Why BLAKE3:** several times faster than SHA-256 (it uses the processor's vector instructions and several cores), from the team behind BLAKE2, with the official Rust crate `blake3` under CC0 / Apache-2.0, both accepted licenses. It detects changes; it isn't a security boundary, so not being a formal standard doesn't matter. The existing folder signatures stay SHA-256: they hash a short listing.
- **No compatibility:** the app is still in development, and existing checkpoints are cleared by hand. Every checkpoint has a manifest from the start; there's no backfill, migration or fallback for checkpoints without one.

### Comparing the live saves

1. **Walk the live save folders for metadata only,** with the same filters and excludes a copy uses, and compute the live metadata digest. A directory listing: nothing is read, fast even for thousands of files. A Load keeps the checkpoint's file times, so right after a Load the live files match it here too.
2. **Look the digest up** among the game's checkpoints: one indexed query, however long the history. No match means changed, which is the common case, and nothing was read.
3. **On a match, confirm by content:** read the live files once, compute the content digest and compare. Why step 3 at all: a same-size change within the same second passes step 2; very unlikely for a save, but possible. A game rewriting identical content fails step 2 and counts as changed, which is harmless: the user just gets to Save.

Why not compare folders directly: that reads the live files and every candidate checkpoint's files each time. With the manifest, checkpoints are never read again, and the common case reads nothing at all.

Checked at the game's exit, after every Save, Load and Revert, when the window gains focus or the game is selected, and ideally whenever the save folders change (the host already watches Steam library folders the same way). And always again at the request itself, so a hotkey never acts on a stale answer.

The same comparison decides, before any automatic checkpoint is copied, whether an existing one already holds the files (SHARED FOLDERS).

### What it locks

- **Save, when the files equal any deliberate checkpoint**, the latest or an older one (just loaded). A Save-only panel: **Nothing changed** / matches the save at 21:06. A hotkey press gets the busy cue. Automatic checkpoints (`recovery`, `closed`) don't count: the user may not know they exist, and saving a state that only an automatic checkpoint holds is a real save.
  - Load keeps restoring the latest deliberate checkpoint. So after loading an older save, playing and dying, Load brings back the latest save, not the one loaded. That's today's rule; the locked Save only makes it more visible.
- **Load, when the files equal the latest deliberate checkpoint** (`already_loaded`). It would change nothing and only add a recovery point. A hotkey press gets the busy cue. A history row's Load of an *older* checkpoint stays available: it changes the files.
- These files equal a deliberate checkpoint, so Save is locked too: the Load lock never shows alone. One full-width panel covers both: **Nothing changed** / since the save at 21:06 (row 10).
- A history row whose checkpoint already equals the live files keeps its Load or RESTORE: it would only add a recovery point, which is rare and harmless.


## STEAM'S OWN FILE IN SAVE FOLDERS

Steam writes `steam_autocloud.vdf` into every folder it syncs through Steam Cloud's automatic mode, recording which account owns it. It isn't game progress (FTL's save folder has one). Kept in a save set, a Steam rewrite makes the files look changed, defeating UNCHANGED SAVES and sharing, and a Load puts back whatever account it held. So the catalog builder adds it to every target's excludes, as one rule rather than an addendum entry per game.


## TESTING

The exit lock (implemented):

- Save, Load and both hotkeys on a running exit-locked game are refused with `game_running`, create nothing, and work at once after the exit (e2e: `a_game_that_saves_on_exit_is_locked_while_it_runs`).
- The setting is on for a new game and for records written before it existed; `configure --wait-for-exit off` turns it off.
- The UI shows the panel, disables and blurs the buttons, hides the rows' Load and Revert, shows no wait cursor, and drops all of it when the host reports the game exited.

To add with the rest:

- Every panel state, its priority when several apply, and its button's command.
- Retry on a blocked game is refused with `game_running` while it runs and works after the exit.
- The closing lockdown: from the exit until the exit checkpoint is recorded, Save, Load and hotkeys are refused with `closing`, then work; none of it with exit checkpoints off.
- Start the game (`launch`) for a Steam game (through Steam's link), a GOG game and a custom one (the executable, from its folder), and the panel hiding once the game runs.
- `steam_autocloud.vdf` in a save folder is in no checkpoint and never touched by a Load.
- Exit checkpoints: only with the game's setting on (`--checkpoint-on-exit`); a failed one leaves the plain marker and ends the closing lockdown; the Game closed row with RESTORE, and the plain marker with it off; taken at the exit, never the Load target, restored and deleted from the Game closed row, discarded after an interrupted copy.
- Shared folders: an exit checkpoint or a recovery point whose files equal any existing checkpoint (not only the latest: loading an older save, then launching and quitting) points at its folder and copies nothing; a Save whose files equal only an automatic checkpoint makes its own copy; deleting the owner removes the folder and every record pointing at it, with the countdown shown on all their rows; deleting a pointing record removes only it and keeps the folder and the owner usable; cancelling keeps everything; Flush removes all.
- Manifests: every checkpoint records its files' sizes, times and BLAKE3 hashes, matching what was copied, in its record and its `checkpoint.json`; the digests are the same for identical files whatever order they're listed in, differ for any change, and differ between two targets holding the same files; excluded files never count; a Load and a Revert leave the files with the checkpoint's times.
- Unchanged saves: files equal to the latest or an older deliberate checkpoint (by metadata and by content) lock Save; equal only to a recovery or closed checkpoint don't; equal metadata with different content doesn't; a Load leaves the files equal to its checkpoint, so Save and the main Load are locked right after while a row's Load of an older save isn't; a change in the save folder unlocks both.


## OPEN QUESTIONS

None right now. Decided but not built: the panel states and their order, Retry locked while running, Start the game, the expert-mode text, exit checkpoints with their per-game setting and history row, the manifest and the Save and Load locks, and Steam's file excluded everywhere.
