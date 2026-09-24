# Save Scummer Host

The host is the app's engine: everything that must keep working with no window open. It finds games, watches them run, makes and restores backups, keeps the history, reacts to hotkeys and the Explorer menu, and serves one small protocol that the desktop UI, the CLI and the Explorer extension all use.

The main window lives in [`PLAN-UI.md`](PLAN-UI.md). Known-game data and every decision about where a known game's saves are live in [`PLAN-CATALOG.md`](PLAN-CATALOG.md); the host calls that module and never re-decides. Builds, packaging and releases live in [`PLAN-BUILD.md`](PLAN-BUILD.md).


## TERMS

- **DIR** — the one directory backed up for a game: its save folder, or its whole data folder if the game protects its saves. The whole DIR is the unit of every backup.
- **Checkpoint** — a copy of DIR. A **saved** checkpoint is one the user made (Save, or a manual copy in Explorer). A **recovery** checkpoint (the UI calls it a "recovery point") is the state of DIR captured automatically just before a Load or Revert.
- **History** — the per-game log of what happened: saves, loads, reverts, game starts and closes. Rows point at checkpoints; they never own files.
- **ACTIVE STACK** — the running games, ordered by which one the user switched to last.
- **Known game** — found on the machine through the catalog. **Custom game** — added by the user with their own paths.


## PRINCIPLES

These decide the cases this document doesn't cover:

- **The host is the only authority.** Every rule, check and lock lives here. Clients ask and display; a disabled button is never what stops an unsafe action. Why: there are four entry points (UI, CLI, hotkeys, Explorer), and they must never disagree.
- **Works without a window.** Scans, monitoring, hotkeys, operations, history and sounds all work with no UI attached. Closing or crashing the UI never cancels anything. Why: the user is in a game, not in our window.
- **Files on disk are the truth; the database describes them.** Checkpoints are ordinary folders next to DIR, so the user can see, copy and delete them in Explorer. The database only remembers what they are. Why: a user's saves must never be locked inside our format.
- **Change nothing you can't verify.** When the state is unclear, leave the live data alone, keep every copy, and say what happened. Why: a wrong "fix" can destroy the one save that mattered.
- **Only an explicit user action deletes.** Save, restarts, scans, game exits and failures never delete checkpoints or history. Only Delete on a row and Flush do. Why: a backup tool that loses backups is worse than none.
- **One operation per game, and reject rather than queue.** A second request for a busy game is refused, not stored for later. Other games stay fully usable. Why: a replayed hotkey press minutes later would load a save the user no longer wants.
- **Deterministic recovery.** An interrupted operation is resolved by fixed rules at startup, never by asking the user to choose. Why: the user can't judge half-finished filesystem states, and there may be no UI.
- **Portable core, thin platform adapters.** Game and operation rules know nothing about Windows, Qt, SQLite or the protocol. Each OS feature (process watching, hotkeys, tray, sounds, Explorer) is a small replaceable adapter. Why: macOS and Linux should be new adapters, not a rewrite.


## PROCESSES

The app ships as three programs:

- **`SaveScummer`** — the desktop UI, the normal entry point.
- **`SaveScummer.Host`** — this document: one background process per user.
- **`SaveScummer.CLI`** — a console client for scripts and diagnostics.

Only one host runs per user. Anything that needs it starts it from the same install folder if it isn't running, and otherwise connects to the one that is:

- Opening the UI starts or reuses the host, then shows the window.
- A CLI command that needs the host starts it and waits until it's ready. Help and syntax errors don't start anything.
- At sign-in (when Launch on startup is on) the host starts in the tray without opening the window.

The host starts, in order: open the database, resolve interrupted operations, scan, start monitoring, then accept operations. The UI can connect earlier and will see the scan in progress.

Closing the window leaves the host in the tray. Clicking the tray icon opens or focuses the UI. The tray menu has two items:

- Main window
- Exit

Exit stops the host safely:

1. Stop accepting new operations.
2. Let any running operation (and any rollback it needs) reach a safe point.
3. End remaining delete countdowns early and run their deletions. Failures don't block exit.
4. Tell connected clients the host is shutting down, so the UI closes instead of reconnecting.
5. Save state, release hotkeys and the tray, and exit.

The UI disconnecting is never a reason to shut down.

### Where data lives

One per-user data folder holds the database, settings and the downloaded catalog. Installs, upgrades and uninstalls never touch it.

| | Data | Cache (disposable) |
| --- | --- | --- |
| Windows | `%LOCALAPPDATA%\SaveScummer` | `%LOCALAPPDATA%\SaveScummer\cache` |
| macOS | `~/Library/Application Support/SaveScummer` | `~/Library/Caches/SaveScummer` |
| Linux | `~/.local/share/SaveScummer` | `$XDG_CACHE_HOME/SaveScummer`, else `~/.cache/SaveScummer` |

Resolve these through the OS (folder redirection, the real home directory), never from a hardcoded username or drive. A `--data-dir` option points the host elsewhere for development and tests.

### Command line

The host has a few options for the build tooling, tests and the sign-in entry. They don't change any rule:

- `--minimized` — start in the tray without opening the window (used by the sign-in entry).
- `--data-dir <dir>` — use another data folder (development and tests).
- `--autostart on|off` — see Launch on startup.
- `--demo` — simulated games and operations, for UI development without touching real saves.
- `--no-catalog-update` — never fetch a newer catalog (tests and isolated runs).
- `--no-integrations` — no hotkeys, tray, sounds or sign-in changes (automated tests).
- `--version` — print the version and exit.

When a host is ready to serve, it prints one machine-readable ready line, so tooling can wait for it instead of guessing.

### Launch on startup

The host is the only thing that writes the sign-in entry, so the checkbox and the real entry can't drift apart:

- The UI checkbox and `SaveScummer.Host --autostart on|off` use the same code: set the preference, write or remove the OS entry (Windows `Run` value, macOS LaunchAgent, Linux XDG autostart), and report the result.
- `off` removes only an entry that points at this host.
- An AppImage re-points its entry to its own path on every start, because each version is a new file. Other platforms install to a fixed path and never rewrite the entry on their own.
- Development builds never create a sign-in entry: `--autostart on` refuses and the checkbox is disabled. Why: signing in must never start a debug host, and a dev host must not touch the installed app's entry.


## GAME LIBRARY

The library holds every game the host knows about, known and custom, in one model, so monitoring, backups and history never care where a game came from.

### Known games and scanning

Finding known games belongs to the catalog module: which stores to read, how installs are matched, which candidate folder becomes DIR, and when that choice sticks. The host only runs it and keeps the results:

- It runs discovery, passes each install to the catalog resolver, and stores the chosen DIR and game records.
- It passes back the previously chosen DIR so the catalog's choice stays sticky.
- It never adds its own rules on top. A game that needs a different rule is a catalog change.

Scans run when the host starts, every 15 minutes (including with no UI), when the user presses Scan games, and when a new catalog bundle arrives. Only one scan runs at a time; a request during a scan joins it instead of starting another. Scan state and its result (how many known games are *newly* found) are published so every client shows the same thing.

Rules the host enforces around scanning:

- **Installed is not the same as having saves.** An installed game whose DIR doesn't exist yet stays visible; Save is unavailable until there is data.
- **Unsure is not uninstalled.** A disconnected drive or unreadable store folder keeps the previous state. A game is uninstalled only when its absence is confirmed.
- **Uninstalled games are hidden, never forgotten.** Their configuration, checkpoints and history stay, and come back if the game does.
- **Scans never touch user choices.** Path overrides and custom games survive every scan.
- **Scans never delete files or history.** They may notice that checkpoints changed on disk (see CHECKPOINTS AND HISTORY).
- **A vanished DIR is re-picked.** When a known game's chosen DIR disappears, the catalog chooses again and may pick a different folder (for example after a game update moves its saves). The host accepts the new DIR. Checkpoints from the old DIR stay recorded but can't be restored into the new one; they become usable again if the game returns to the old folder. User overrides and custom games are never re-picked: a missing DIR there just makes Save, Load and Revert unavailable until it comes back or is reconfigured.
- **No save folder at all.** If the catalog finds no candidate for an install (the game is unsupported on this platform or store), the game stays visible with a configuration error and no operations, so the user can set its DIR in Configure.

The catalog can turn two installs of one game into two game records, each with its own DIR and history (for example a Steam copy and a GOG copy). When that happens, the host gives each of them an **install tag**: the store's name (`Steam`, `GOG`, `Epic`), or the install folder's name when both come from the same store. A game with one install has no tag. Why: two identical names in the library would be impossible to tell apart, and the tag says which copy the user is about to touch.

### Custom games

The user can add a game the catalog doesn't know. A custom game has:

- a host-generated ID that never collides with catalog IDs and doesn't depend on the name;
- a name (non-blank; doesn't need to be unique);
- one absolute executable path, used for install status and monitoring;
- one absolute DIR;
- no instructions.

Neither path has to exist yet, so the user can register a game before installing it or before its first save. Validation never creates anything. An invalid addition is rejected whole; nothing partial is left behind.

The executable is the install evidence: the game is installed when the file exists, uninstalled only when its absence is confirmed, and unchanged otherwise. Every scan rechecks this. A scan never turns a custom game into a known one, even if its paths match a catalog game.

Custom games are kept forever. There is no way to remove one in this version.

### Configure

- **Known games:** the user can override the executable and DIR, and Reset each back to the catalog's choice. Name and instructions always come from the catalog.
- **Custom games:** name, executable and DIR are all editable.
- A change is applied only after every new value validates. Otherwise the old configuration stays exactly as it was.
- Changing DIR never moves checkpoints to the new folder. Each checkpoint remembers the DIR it was made from and can only be restored there. Switching back to the old DIR makes its checkpoints usable again, with the same IDs and history.

### DIR safety

Every DIR (catalog choice, override or custom) passes the same checks, because a Load replaces the whole directory:

- **No overlaps.** A DIR can't equal, contain or sit inside another game's DIR, including games that are currently unavailable. The error names the other game.
- **No broad locations.** Reject roots and shared folders, and anything above them:
  - drive, volume and network-share roots;
  - user home and profile roots;
  - OS folders;
  - AppData roots (Roaming, Local, LocalLow, ProgramData);
  - Documents and Saved Games roots;
  - Program Files, Steam library roots, `steamapps`, `common`;
  - the equivalents on macOS, Linux and inside a Proton prefix.

  A game's own child folder, such as `{APPDATA}/Void_War`, is fine, however shallow.
- **Compare real directories, not strings.** Resolve aliases and follow the filesystem's case rules, so `Game` and `Game2` don't conflict and two case-different folders on Linux stay distinct. Base the checks on resolved OS folders, not folder names.
- **Links resolve to their target.** If DIR is a symlink or junction, all operations run on the real folder; the link itself is never renamed, replaced or copied. If the link later points elsewhere or breaks, the game fails validation until it's configured again.
- **Recheck before every operation,** so a folder changed since configuration can't slip past.

These are hard errors, not warnings. If the catalog's pick is invalid and there's no valid override, the game stays visible with a configuration error and no operations.


## MONITOR AND ACTIVE STACK

After the first scan, the host watches every game's executables start, get focus and exit, and keeps the ACTIVE STACK:

- One entry per running game. Several processes of one game count as one.
- A game switched to (its window gets focus) moves to the top.
- A game that starts appears in the stack but doesn't jump ahead of games focused more recently.
- When the last process of a game exits, it leaves the stack.

For example: FTL is running, so the stack is just FTL. Void War starts and gets focus, so it goes on top. The user alt-tabs to FTL, so FTL goes on top. FTL exits, so Void War is on top again, even if another app is in the foreground now.

Processes are matched to games by the full executable path, so an unrelated program with the same file name elsewhere doesn't count. When two installs of one game exist, each install's executables map to its own game record, so hotkeys target the copy that's actually running.

Game starts and exits add **Game started** and **Game closed** history markers. They create no checkpoints and have no actions. Relaunching continues the same history. Keep track of when the host was running, so time the host didn't observe never joins two unrelated sessions and no exact start or exit time is invented.

The monitor starts correctly whether games were started before or after it, and rebuilds the current stack after a restart.


## CHECKPOINTS AND HISTORY

### Where checkpoints live

Checkpoints are folders next to DIR, in the same parent:

```text
Game/
|-- Void_War/                     current game data (DIR)
|-- Void_War - Copy/              saved checkpoint
|-- Void_War - Copy (2)/          saved checkpoint
|-- Void_War.recovery-000001/     recovery checkpoint: state before a load
`-- Void_War.recovery-000002/     recovery checkpoint: state before another load
```

- **Saved checkpoints use the file manager's own duplicate-folder names** (Windows English: `Void_War - Copy`, `Void_War - Copy (2)`; localized variants too). Why: then a copy the user makes in Explorer and a copy the app makes are the same thing.
- **Recovery checkpoints use a reserved name** (`<DIR name>.recovery-<id>`) on every platform, so they can never be mistaken for saved ones.
- **Temporary folders** (staging copies, folders being deleted) also use reserved names. They are never treated as checkpoints and are cleaned up on the next start or scan, unless an unresolved interruption still needs them.
- **Never overwrite.** A new checkpoint always takes a free name, even if a folder appears there at the last moment.
- **Completed checkpoints are read-only.** Restoring copies from them; the checkpoint stays as it was.

### Manual copies

Copying DIR in Explorer is a supported way to save. The user never has to import, rename or tag the copy.

- Look for copies at startup, on every scan, when a game's history is opened, and before a default Load.
- Match only complete native duplicate names derived from DIR's actual name. A folder that merely starts with the same name isn't a checkpoint.
- Register each new copy once, as an ordinary saved checkpoint, without touching its name or contents.
- Its real save time is unknown. Use the folder's modification time as an *estimate* for ordering and display, and mark it as estimated. Never present discovery time as save time.

### Backups changed outside the app

A checkpoint is one observed *generation* of a folder, not a permanent claim on its name. Record the folder's identity and a change signature (paths, kinds, sizes and modification times of everything inside) and check them again:

- **Gone** (the parent can be read and the folder isn't there): retire that checkpoint.
- **Replaced or edited** (different identity or signature): retire the old checkpoint and register the folder as a new one with new IDs. A folder can be deleted and recreated between scans, or while the host was off; the result is the same.
- **Can't tell** (drive disconnected, folder unreadable, inspection incomplete): mark the checkpoint unavailable, but don't retire it and don't register anything new. Try again later.
- **A changed recovery checkpoint** loses its Revert target. It is never registered as a saved checkpoint.
- **Before any restore, check again.** An older action never restores a newer folder that happens to have the same name.

The signature is change detection, not a content checksum: an edit that preserves sizes and times isn't detectable.

### Which rows are visible

The host computes the visible history once, the same for every client:

| Row | Visible while | Actions |
| --- | --- | --- |
| Saved | its saved checkpoint exists | Load this save, Delete |
| Loaded | its recovery checkpoint exists | Revert, Delete |
| Reverted | its recovery checkpoint exists | Revert, Delete |
| Game started / Game closed | the session contains a visible Saved, Loaded or Reverted row | none |

- A temporarily unavailable checkpoint keeps its row, with its actions disabled.
- Sessions with nothing left in them disappear, including ones between two sessions that still have saves.
- With no checkpoints left, history is empty, even if internal records remain.
- Hiding a row changes no files and rewrites no references. The host keeps what it needs internally for recovery.

### Labels

A saved checkpoint can have a **label**, a short name the user gives it ("Before boss fight"). Why: after a dozen saves, times stop meaning anything; a name says which save is which.

- **It belongs to the checkpoint, not to a row.** The Saved row, the Loaded rows that loaded it, and the latest-checkpoint summary behind the Load button all read the same label, so renaming it updates them all.
- **Only in the database.** The folder's name and contents are never touched. Why: the folder is the user's copy, and a native duplicate name must stay native.
- **Only saved checkpoints.** Recovery checkpoints have no label.
- **Limits:** one line, at most 100 characters. The host trims it and turns line breaks into spaces. Empty means no label. The host enforces this for every client; the UI's limit is only a convenience.
- **Not an operation.** Setting a label changes no files and adds no history event, so it doesn't wait for the game's lock. It works while the game is busy and while the checkpoint is counting down to deletion.
- **Last write wins.** A label request for a checkpoint that no longer exists is rejected quietly, as "gone".
- **It outlives deletion where rows do.** When a checkpoint is deleted, its record keeps the label, so a Loaded row still says what it loaded. Flush clears it with everything else.
- **A new generation starts unlabeled.** A folder replaced or edited outside the app becomes a new checkpoint, and the old label isn't carried over. Why: the host can't know the new contents still match the name.
- **Manual copies can be labeled** like any saved checkpoint.
- **Save can carry a label** (used by the CLI). Hotkey and UI saves start unlabeled.

Actions in the same second stay distinct: rows have stable IDs and a stable order that doesn't depend on timestamps.


## OPERATIONS

Every operation, from any entry point, goes through the same handling and the same per-game lock. The lock covers Save, Load, Revert, Delete, Flush and configuration changes. A request for a busy game is rejected immediately; nothing is queued for later.

The app works on files. The user is responsible for the game picking up a restored state, for example by reloading or restarting it. Copying is ordinary and best-effort: the host doesn't pause the game or detect writes during a copy. A copy made while the game is writing may be inconsistent; real copy errors fail the operation.

### SAVE

1. If DIR doesn't exist or is empty, there is nothing to save: Save is unavailable.
2. Copy DIR to a temporary folder with a reserved name.
3. When the copy is complete, rename it to the next free native duplicate name. Why the detour: a half-finished copy under a native name would be picked up as a manual copy.
4. Only then register the checkpoint and add a Saved row. A failed or interrupted copy never becomes a checkpoint; a finished copy whose Saved row wasn't recorded is recorded at the next start (recovery rule 3).

### LOAD

1. Pick the checkpoint:
   - **Load** (button or hotkey): the newest usable saved checkpoint, manual copies included. Order by save time (or the estimate for manual copies), breaking ties by the order the host registered them. Recovery checkpoints are never picked. With none, or when DIR doesn't exist, Load is unavailable. Why not recreate a missing DIR: a missing folder can mean the game moved its saves or the path is wrong, and loading into it would hide that.
   - **Load this save** (a history row): exactly that row's checkpoint.
2. Check it: it belongs to this game, it's a saved checkpoint, it was made from the current DIR, and it's unchanged on disk. Otherwise stop before touching anything.
3. Copy the current DIR to a new recovery checkpoint. If DIR is missing or the copy fails, stop; DIR stays untouched.
4. Copy the chosen checkpoint to a staging folder.
5. Swap: move DIR aside, move the staged copy into place. If this fails, put the original back; if that fails too, keep everything and report it.
6. Only after the swap, add a Loaded row that references both the checkpoint loaded and the new recovery checkpoint. Then remove the moved-aside original.

The checkpoint itself is never modified, and every later row stays.

### REVERT

Revert puts back the state from just before a Loaded or Reverted row's operation. It is a Load whose source is a recovery checkpoint, so it follows every Load step, including preserving the current state first. Why: the state being replaced may hold progress the user wants back, and one rule for every restore means one set of safety and interruption rules.

1. Take the row's recovery checkpoint and check it like a Load target (it must be a recovery checkpoint instead of a saved one).
2. Copy the current DIR to a new recovery checkpoint, then stage and swap exactly as in Load.
3. Add a Reverted row that references the checkpoint restored and the new recovery checkpoint. Its own Revert undoes this revert.

Nothing is used up. The recovery checkpoint that was restored stays, so its row keeps its Revert, and reverting a revert follows exactly the same rule as reverting a load. For example:

| Time | Event | Checkpoints | Revert restores |
| --- | --- | --- | --- |
| 19:25 | Saved | Saved A | — (Load this save restores A) |
| 19:31 | Loaded · 19:25 | Restored A; kept the previous state as B | B |
| 19:38 | Saved | Saved C | — |
| 19:42 | Loaded · 19:25 | Restored A; kept the previous state as D | D |
| 19:43 | Reverted · 19:42 | Restored D; kept the previous state as E | E |

### DELETE

Delete on a row removes one checkpoint: the saved checkpoint of a Saved row, or the recovery checkpoint of a Loaded or Reverted row (and with it the row). It deletes only that exact generation; a folder that changed since it was shown is refused.

Deletion is not immediate. The host owns a short countdown so a misclick can be undone:

- Each Delete starts its own 5-second countdown with a Cancel. The same row can't be requested twice.
- The host decides cancel versus run. An accepted cancel guarantees nothing is deleted; a cancel that arrives too late gets the real state back.
- When a countdown ends, the deletion waits for the game's turn (reported as *waiting*), then runs through the same per-game lock. A countdown doesn't reserve the game: Save and Load still work until the deletion actually starts, and Load still targets the latest checkpoint even if it's counting down.
- Countdowns live only in memory. After a crash, deletions that hadn't started are simply dropped.
- A failed deletion isn't retried; the row comes back and the user can try again.
- If Flush removes an entry that's counting down, the countdown is dropped quietly.

Files are deleted by first renaming the folder to a reserved name, then removing it. Why: a locked folder is never left half-deleted under its real name. Leftover renamed folders are cleaned up at the next start or scan. The record stays until the files are actually gone.

### FLUSH

Flush deletes every checkpoint of a game (saved, recovery and leftover temporary folders) and clears its history. DIR is not touched.

- Before confirming, a client can ask for a preview: counts per kind, and the paths (with labels) in pages.
- The preview is not a contract. On confirm, the host finds everything again from scratch and deletes what it finds. Why: simpler than binding the confirmation to a snapshot of the preview, and it can't delete anything the preview didn't describe in kind.
- Only records whose files were really deleted are cleared. Failures are reported and the remaining checkpoints stay.
- Flush isn't available while a game is waiting on an unresolved interruption.


## INTERRUPTIONS AND FAILURES

Files and the database can't change in one transaction. So before each step that changes files, the host durably records what it's about to do: the operation, the exact paths involved (live DIR, source, recovery, staging, moved-aside original) and the identities needed to verify them. After the step, it records the result. Failed or unfinished operations never appear as completed history.

### Recovery at startup

Before a game can be operated on again, the host resolves any unfinished operation for it. The rules run in order, with or without a UI:

1. **Nothing on disk changed** (interrupted while copying or staging): mark the operation failed and release the game. An interrupted Save leaves a one-line notice; an interrupted Load is silent.
2. **DIR is missing and the moved-aside original is verifiably intact:** move the original back. The operation never happened.
3. **The result is verifiably in place but wasn't recorded** (a Save's copy already has its native name, or a Load's or Revert's replacement is installed): finish the operation and add its history row, exactly as if it had completed.
4. **Anything else:** keep the current DIR exactly as it is, mark the operation failed without a history row, and keep every recovery and temporary folder. Release the game if a coherent DIR exists; otherwise keep it blocked with a sticky error and try again at every start and whenever the user presses Retry.

Never roll back over data that may be newer, never retry the requested Load or Revert on its own, and never delete kept material while an interruption is unresolved.

### Failures the host reports

Every failure reaches clients as a structured result: a stable kind, the game, and the exact paths involved. The wording, buttons and layout belong to the UI. The host must tell these situations apart:

| Situation | Host behavior |
| --- | --- |
| DIR is locked, so moving it aside fails | Nothing changed. Retry can help. |
| A file couldn't be read during a copy | Nothing changed. Retry can help. |
| The destination is full, or writing is denied | Nothing changed. Retry can help. |
| The drive or share is unavailable | Nothing changed; affected checkpoints are unavailable, not retired. |
| DIR contains a link or special file | Refused; links inside DIR aren't copied. |
| The checkpoint changed or vanished since it was shown | Refused before anything is touched. |
| The checkpoint was made from a different DIR | Refused; points at Configure. |
| DIR is invalid, overlapping, or a link that changed | Refused until reconfigured. |
| The original couldn't be put back (recovery rule 4) | Game blocked, all material kept, retried at every start. |
| A Load was applied but couldn't be recorded | Reported; startup rule 3 finishes recording it. |
| A deletion couldn't finish | Record kept; leftovers cleaned later. |
| The database can't be opened | The host can't serve; says so. |
| The host is shutting down | New requests are rejected. |
| The client and host versions don't match | Refused with an "upgrade required" error. |

A failed or blocked game never affects other games.

To open a folder (the save folder, the backups' parent, a checkpoint, a kept recovery folder), a client names *what* to open and the host opens the real path. Clients never have to type or pass paths.


## STORAGE

One SQLite database holds game configuration, settings, checkpoint records (including labels), history and the operation journal. Game files never go into it.

- **Write only what changed.** Saving for one game doesn't rewrite that game's old records or any other game's. Why: histories grow to tens of thousands of rows, and a Save must stay instant.
- **Each change is one transaction,** and in-memory state updates only after it commits. A failed commit leaves the old state everywhere.
- **Keep the history out of memory.** Hold configuration, current operations and small caches in memory; read history and old records from the database when needed. Startup must still find every unfinished operation directly. Something missing from a cache is not proof it doesn't exist.
- **Keep durability.** Saving fewer writes must never weaken crash safety.
- **Migrations preserve** IDs, ordering, checkpoint ownership and unfinished operations.

The visible-history rules live in the core, not in SQL. Storage may keep a derived index to make history pages fast, but it never implements a second version of those rules.


## PLATFORM FEEDBACK

Each of these is a small adapter the host owns, so it works with no window open.

### Hotkeys

- **Ctrl+F5** runs Save and **Ctrl+F9** runs Load.
- When the desktop window is focused, they act on the game selected in it, even a stopped one. Otherwise they act on the top of the ACTIVE STACK. With neither, they do nothing. The UI reports its focus and selection to the host so the host can decide.
- Holding a key triggers one operation, not many.
- A busy game gets a short, rate-limited busy cue.

### Sounds

Hotkey-triggered Save and Load play a start cue when the request is accepted, then a completion cue once the files and history are committed:

| Event | Sound |
| --- | --- |
| Save started | Two short, soft ascending notes |
| Save completed | A brighter ascending resolution |
| Load started | Two short, soft descending notes |
| Load completed | A rounded descending resolution |
| Failed or couldn't start | A distinct, low double knock |
| Rejected because busy | One quiet, dry tick |

- Use the approved sounds in `assets/sounds`, built into the host. Start cues are about 100 ms, completion cues about 200 ms.
- A failed operation plays the failure cue instead of completion, never a success cue.
- For very fast operations, keep both cues distinguishable. Sound never delays the file work or holds the lock.
- One **Play sounds** setting, on by default.
- When the window is hidden, a failure also shows an OS notification.

### Explorer menu

- **Save** appears when right-clicking a game's DIR.
- **Load** appears when right-clicking a saved checkpoint of a DIR, including a manual copy. It loads exactly that folder, even if a newer one exists.
- **A labeled checkpoint's Load names it:** `Load 'Before boss fight'`. Why: native copy names like `Void_War - Copy (3)` don't say which save it is, and the label does. Unlabeled checkpoints just show `Load`. A long label is shortened with `…` to keep the menu narrow, and the text is shown as-is (an `&` in a label is not a menu shortcut). The label comes with the host's answer about what to show, so the menu never shows a label from a folder that has since changed.
- Recovery checkpoints get no menu item; they're reached through Revert in the history.
- The extension is only a client. It asks the host what to show and forwards the click. If the host isn't running, the menu doesn't appear; right-clicking a folder never starts the host.

### Per OS

| | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Process and focus watching | Yes | To investigate | To investigate |
| Global hotkeys | Yes | To investigate | To investigate |
| File-manager menu | Explorer extension | Not planned | Not planned |
| Native duplicate names | Explorer | Finder, localized | Per supported file manager, each verified |


## ARTWORK

The UI shows Steam art for known games: hero art and logo for sidebar cards, the header image as a fallback, and a small icon next to the game's name. The host gets and caches it; the UI never downloads anything.

- Read Steam's local library cache first (`Steam/appcache/librarycache/<appid>/`, including its hashed subfolders), then Steam's public CDN (`shared.fastly.steamstatic.com/store_item_assets/steam/apps/<appid>/`: `library_hero.jpg`, `logo.png`, `header.jpg`). No Steam login or API key. Why both: the local cache works offline, but Steam has changed its layout before, and the CDN has every game's art, even games Steam hasn't cached.
- Scale images down to display size and keep them in the host's own cache. Deleting the cache is harmless; it's rebuilt.
- Check the cache at host start, after every scan and when a UI attaches. Download what's missing or unreadable. A download is written to a temporary file and published only once it decodes as an image.
- Artwork is best-effort: it runs in the background, never blocks scans, operations, monitoring or the UI, and retries later on failure without nagging.
- Custom games and games without art get none; the UI shows its placeholder.


## CLI

`SaveScummer.CLI` is for scripts and diagnostics. It's a client like the UI:

- It uses the same protocol and can do everything the protocol offers. It never opens the database or touches game files itself. A Save from the CLI can set a label, and a separate command sets or clears one later.
- It starts the host when a command needs it, and never becomes a second host. `--no-start` makes it fail instead of starting one, for tools like the installer that only want to talk to a running host.
- It prints readable output by default and machine-readable output on request, with exit codes that tell success, rejection and failure apart.
- Long history can be read page by page or streamed without loading it all. If the history changes mid-stream, it says so instead of printing duplicates or gaps.


## PROTOCOL

This is the contract between the host and its clients (UI, CLI, Explorer extension). A client can be built against this section alone.

### Shape

- **Local and per user.** Named pipes on Windows, Unix sockets elsewhere, reachable only by the signed-in user.
- **Versioned.** Every message carries the protocol version. A mismatch is refused clearly, never half-understood. Host and clients ship together and change together, with shared example messages that both sides test against.
- **Plain data.** Stable opaque IDs, UTC timestamps, no UI concepts, no internal records. Timestamps are for display, never IDs.
- **Structured errors.** A kind, the game and the paths involved. The host sends no user-facing wording.

### What clients can do

Commands:

- Save (optionally with a label)
- Load (the latest checkpoint, or an exact saved checkpoint)
- Revert (an exact recovery checkpoint)
- Delete a checkpoint, and cancel that delete
- Flush
- Set or clear a label
- Add a custom game
- Configure a game (paths, name for custom games, reset overrides)
- Scan
- Change settings (Play sounds, Launch on startup)
- Report the UI's focus and selected game
- Open a folder by what it is
- Retry a blocked game
- Refresh the catalog now
- Shut down (the same safe Exit as the tray menu)

Queries:

- The current state (see Watching)
- A game's history, in pages
- A Flush preview, with paths in pages
- An operation's outcome by ID
- What the Explorer menu should show for a folder: whether it's a DIR or a saved checkpoint, of which game, and the label
- The active catalog revision

### Commands are safe to repeat

- Operations are **accepted or rejected** at once: accepted with an operation ID, or rejected with a reason (busy, unavailable, blocked, invalid).
- "Accepted" is sent only after the operation is durably recorded, so a crash right after can't lose it.
- Every request carries a client-made ID. Repeating it, even after a restart, returns the same operation instead of running it twice.
- A client that disconnects can reconnect and ask for the outcome by operation ID. It never re-sends a destructive command to find out.
- A disconnect never means success and never cancels anything.
- Clients refer to games and checkpoints by ID. The host resolves every path itself; clients never supply a path it trusts. The one exception is the Explorer query, where the folder the user clicked is the question; the host still decides what that folder is.

### Watching

A client that watches gets the full current state, then a new state whenever something changes:

- The state is a **summary**, and it's self-contained: games (configuration, install tag, install and availability status, instructions, artwork, and whether Save and Load are available with the reason when not: no game data, no save folder, no saves, busy or blocked), settings, the ACTIVE STACK, scan state, each game's latest checkpoint (with its label) and whether it has history, busy and blocked games with their errors, pending delete countdowns, and each game's last result.
- It never contains full history or old records, so its size doesn't grow with history. History is always queried separately, in pages.
- Every state carries a revision and a host instance ID. A new instance ID means the host restarted: throw away everything cached and start again.
- Progress can be coalesced; each delivered state stands on its own.
- The last message before a shutdown says so, and the UI closes instead of reconnecting.

### History pages

- Newest first, in the stable history order, so equal timestamps never cause gaps or duplicates.
- Each row carries its times (with a flag for estimated ones), its checkpoint reference, the label and time of the save it's about (its own for Saved, the loaded save's for Loaded), the time of the row it reverted (for Reverted) and whether each action is available right now, so the client needs nothing else to show it.
- A page position belongs to one game, one host instance and one version of that game's history. A relevant change to that game invalidates it with an explicit "reload" answer; changes to other games, progress, artwork and labels don't. The watch stream says a game's labels changed; the client re-reads the pages it shows with the same page positions, so editing a label never resets scrolling or history.
- Anything large is paged. A reply that would be too big is an explicit error, never silently cut off.


## TESTING

Rules must be provable without a desktop, and the OS parts must be proven for real. Use the normal test runner, temporary folders and databases, and one small fake-game program. No custom test framework, dashboard or scenario language.

### How

- **Core logic** runs in a plain test process with a fake filesystem, database and clock. This is where the history rules, checkpoint selection, locking, delete countdowns and the four recovery rules are pinned down, including failures injected at every step.
- **File operations** run against real temporary folders: copies, never-overwrite naming, swaps, rollbacks, the delete-by-rename, locked files, links and junctions.
- **Storage** runs against real SQLite files: transactions, restart, migrations, and proof that a Save writes nothing it didn't change.
- **Integration** runs the real host with the fake game. The fake game is copied to different paths to act as different games, run several times to act as one game with several processes, and can delay its window, launch a child and exit, crash, write saves slowly or hold a file open. The monitor must find it through the same OS mechanisms as real games.
- **Crashes** are simulated by killing a real host between recorded steps, then starting it again on the same folders and database.
- Tests wait for events with bounded timeouts and use simple synchronization for races, not long sleeps. Tests that fight over focus or hotkeys run one at a time. Every test cleans up its own processes and files, even when it fails.

### What must be proven

- **Monitor:** starts before and after games; normal exit, kill and crash; quick relaunches; a process that exits before showing a window; one entry for several processes; launchers that start the game and exit; same file name at a different path; focus switching between games and unrelated apps; the stack after a host restart; exactly one start and one close marker per session.
- **Library:** scans don't duplicate games; installs and uninstalls are noticed; unavailable drives aren't uninstalls; overrides and custom games survive scans; the periodic scan's handler (without waiting 15 minutes); every DIR safety rule, including aliases, case rules, redirected folders, Proton equivalents and a DIR that doesn't exist yet; no test ever copies or replaces a real system folder.
- **Checkpoints:** manual copies registered once, localized names, lookalike folders ignored; estimated versus known times; external deletion, replacement (between scans and while the host is off) and in-place edits; unreadable folders not retired; changed recovery checkpoints never imported.
- **Operations:** Save, Load, Load this save, Revert, reverting a revert and Delete compared by actual file contents; every rejection happens before anything is created; a changed DIR (A to B and back to A); visible history after deletions, including empty sessions disappearing; Flush with partial failures; labels (limits and trimming, shown on Saved and Loaded rows, kept after deletion, cleared by Flush, not carried to a replaced folder, set while busy, rejected for a gone checkpoint, never written into folders); delete countdowns (cancel versus run, waiting while busy, dropped by Flush, finished on shutdown, dropped after a crash).
- **Failures:** a failure injected at every step, and a kill at every recorded step, each checked against the four recovery rules by both the files and the records; other games stay usable.
- **Entry points:** UI, CLI, hotkeys and Explorer all hit the same lock; the Explorer Load item shows a checkpoint's label (shortened, `&` shown literally, plain `Load` without one); a busy game rejects and never replays; held keys; the right game is targeted with the window focused and unfocused; the right sound for each outcome, including never a success cue on failure.
- **Protocol:** repeated request IDs; reconnecting mid-operation; a host restart invalidating state and history pages; paging with equal timestamps and sessions crossing pages; history bigger than any single reply; a version mismatch for each client.
- **Scale:** histories of 100, 10,000 and 100,000 rows, in one game and spread across many. Measure startup, memory, writes per Save, the cost of an idle watch and the first history page. Timings are reported separately so a busy machine doesn't fail functional tests.

### By hand, for now

Some things aren't worth automating yet: real hotkey delivery (including fullscreen games and elevated processes), tray behavior, notifications, the sign-in entry, installing and removing the Explorer extension in a clean profile, antivirus interference, sleep and resume, and audible sound quality. Keep them as a short checklist per platform.

Windows comes first. Each new platform proves its own monitoring, hotkeys, file operations and duplicate-folder names; passing on fixtures alone doesn't count.
