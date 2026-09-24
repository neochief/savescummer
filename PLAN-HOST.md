# Save Scummer Host

The host is the app's engine: everything that must keep working with no window open. It finds games, watches them run, makes and restores backups, keeps the history, reacts to hotkeys, and serves one small protocol that the desktop UI and the CLI both use.

The main window lives in [`PLAN-UI.md`](PLAN-UI.md). Known-game data and every decision about where a known game's saves are live in [`PLAN-CATALOG.md`](PLAN-CATALOG.md); the host calls that module and never re-decides. Builds, packaging and releases live in [`PLAN-BUILD.md`](PLAN-BUILD.md).


## TERMS

- **Target** — one place a game keeps saves: a root folder plus a filter saying what inside it counts (everything, one exact name, or a glob pattern), minus excludes. A plain folder, a single file and a pattern are all targets.
- **Save set** — all of a game's targets. The whole save set is the unit of every backup: Save copies all of it, Load restores all of it.
- **Checkpoint** — a copy of everything the save set matched at one moment, kept in the **checkpoint store**. A **saved** checkpoint is one the user made with Save. A **recovery** checkpoint (the UI calls it a "recovery point") is the state captured automatically just before a Load or Revert.
- **History** — the per-game log of what happened: saves, loads, reverts, game starts and closes. Rows point at checkpoints; they never own files.
- **ACTIVE STACK** — the running games, ordered by which one the user switched to last.
- **Known game** — found on the machine through the catalog. **Custom game** — added by the user with their own paths.


## PRINCIPLES

These decide the cases this document doesn't cover:

- **The host is the only authority.** Every rule, check and lock lives here. Clients ask and display; a disabled button is never what stops an unsafe action. Why: there are three entry points (UI, CLI, hotkeys), and they must never disagree.
- **Works without a window.** Scans, monitoring, hotkeys, operations, history and sounds all work with no UI attached. Closing or crashing the UI never cancels anything. Why: the user is in a game, not in our window.
- **Files on disk are the truth; the database describes them.** Checkpoints are ordinary folders in the checkpoint store, holding plain copies of the game's files, so the user can open and copy them in the file manager. The database only remembers what they are. Why: a user's saves must never be locked inside our format.
- **Change nothing you can't verify.** When the state is unclear, leave the live data alone, keep every copy, and say what happened. Why: a wrong "fix" can destroy the one save that mattered.
- **Only an explicit user action deletes.** Save, restarts, scans, game exits and failures never delete checkpoints or history. Only Delete on a row and Flush do. Why: a backup tool that loses backups is worse than none.
- **One operation per game, and reject rather than queue.** A second request for a busy game is refused, not stored for later. Other games stay fully usable. Why: a replayed hotkey press minutes later would load a save the user no longer wants.
- **Deterministic recovery.** An interrupted operation is resolved by fixed rules at startup, never by asking the user to choose. Why: the user can't judge half-finished filesystem states, and there may be no UI.
- **Portable core, thin platform adapters.** Game and operation rules know nothing about Windows, Qt, SQLite or the protocol. Each OS feature (process watching, hotkeys, tray, sounds, sign-in) is a small replaceable adapter. Why: macOS and Linux should be new adapters, not a rewrite.


## PROCESSES

The app ships as three programs:

- **`SaveScummer`** — the desktop UI, the normal entry point.
- **`SaveScummer.Host`** — this document: one background process per user.
- **`SaveScummer.CLI`** — a console client for scripts, testing and diagnostics.

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

### The host log

The host keeps a plain-text log, `host.log` in the data folder, and writes the same lines to its error output. Why: the host usually starts at sign-in or from the UI, where nobody reads its output, and "the game never showed up" or "it didn't notice I quit" can only be answered by what the host saw and when.

Each line has a UTC timestamp. The log records:

- startup and shutdown steps, and a clean stop;
- games installed, installed again and uninstalled, with each game's name, ID, store and folder, and the reason the scan ran (startup, a store folder or registry key changed, the window gained focus, the periodic scan, a catalog update, a user request);
- games started and closed, and a game found already running when the host started (its start time is unknown, so none is claimed).

Rules:

- **Only what the host noticed on its own.** Operations are already in the history, so the log doesn't repeat them.
- **Bounded.** Past 1 MB the file becomes `host.log.1`, replacing the previous one, and a new file starts. Why: a host that runs for months must not grow a file forever.
- **Never in the way.** A log that can't be written is skipped; it never fails or delays anything.

### The checkpoint store

Checkpoints live in a `checkpoints` folder inside the data folder by default. Why one central place instead of next to the saves: a save set can span several folders, even several drives, so there is no single "next to" location.

- **The user can move the store**, for example to a bigger drive. Moving is an operation for every game at once: copy everything to the new location, verify it, switch, then delete the old copies. Until the switch, the old store stays in use, so an interrupted move changes nothing. No game operation runs while a move is in progress.
- **A store that can't be reached** (its drive unplugged) makes Save, Load and Revert unavailable with a clear reason. Checkpoints are marked unavailable, never retired.
- **Installs, upgrades and uninstalls never touch it**, wherever it is.

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

Finding known games belongs to the catalog module: which stores to read, how installs are matched, which build an install runs, which Steam account is current, and which targets make up each game's save set. The host only runs it and keeps the results:

- It runs discovery, passes each install to the catalog resolver, and stores the game records with their save sets and the context they were resolved in (build, Steam account).
- It asks again at every scan and whenever a Steam game starts, and takes the answer as it is.
- It never adds its own rules on top. A game that needs a different rule is a catalog change.

A game the user just installed should be in the library the next time they look, without pressing anything. Why: the host usually runs in the tray from login, so a scan only at host start misses everything installed afterwards.

There are two kinds of scan, because they cost very different amounts:

- **Install scan:** read the store records (Steam libraries and app manifests, the GOG and uninstall registry, Epic manifests), check executables, and run the catalog resolver. It costs tens of milliseconds and grows with the number of libraries, not the catalog size: each Steam `steamapps` folder is listed once and catalog games are looked up by ID, never one file check per catalog game. It is safe to run often.
- **Full scan:** an install scan plus re-checking every checkpoint on disk (see CHECKPOINTS AND HISTORY). The checkpoint part opens every file of every checkpoint, so it grows with history and runs only when needed.

Triggers:

| Trigger | Scan |
|---|---|
| Host start | full |
| The user presses Scan games | full |
| A new catalog bundle arrives | full |
| Every 15 minutes, including with no UI | full |
| The desktop window is shown or gains focus (from the UI's focus report), at most once per 20 seconds | install |
| A watched store location changes, 2 seconds after the last change | install |

Why focus: it is the moment the user looks, and the cooldown means alt-tabbing never causes repeated scans. Why watching as well: with the window already open while Steam finishes a download, focus never fires; and in SteamOS Game Mode our window never gets focus at all, so watching and the periodic scan are the only triggers there.

**Watched locations** are few and fixed: each Steam library's `steamapps` folder (not recursive: app manifests are created when an install starts and rewritten when it completes), the main Steam `libraryfolders.vdf` (a library added or removed changes the watch list), the Epic manifests folder, Heroic's install lists on Linux, and on Windows the uninstall registry keys (both views, machine and user) and the GOG games key. Unrelated changes there (a Windows update, another app's installer) only cause a cheap scan that finds nothing. Loose install folders (Epic without manifests, standalone candidates) can't be watched; focus and periodic scans cover them.

Per platform:

- **Windows:** folder change notifications and registry change notifications. A folder watch keeps the folder open, which would stop the user from safely removing a USB drive holding a Steam library, so the host releases a drive's watches when Windows asks to remove it and watches again when the drive returns.
- **macOS:** FSEvents; it holds nothing open, so ejecting is never blocked. The first access to a removable or network volume, or to Documents, makes macOS ask the user for permission. The first scan to touch such a location must follow a user action (first run, Scan games), never a silent background scan. Why: a permission dialog out of nowhere looks like the app is snooping.
- **Linux / SteamOS:** inotify; a watch is dropped automatically on unmount. New mounts (a Steam Deck SD card under `/run/media`) are watched for so their libraries are picked up.
- **Everywhere:** network drives don't reliably report changes, and the OS may drop events under load and only say "something changed". Both are answered with a scan; focus and periodic scans remain the safety net.

How scans run:

- On their own background worker. A scan never holds up requests and never pauses game monitoring. Why: a library on a sleeping hard disk takes seconds to spin up, and an offline network share can hang for 20 seconds or more; neither may delay a Save hotkey or a game-start marker.
- Only one scan runs at a time; a request during a scan joins it and gets that scan's result, instead of starting another or returning early. A full scan requested during an install scan runs right after it.
- Scan state is published with its origin: a user-requested scan, or a background one. Only a user-requested scan's state and result (how many known games are *newly* found) are shown in the UI; background scans are silent and their games simply appear.

Rules the host enforces around scanning:

- **Only finished installs count.** A Steam game is installed once Steam marks the install finished, not when its app manifest first appears (the catalog's rule, PLAN-CATALOG.md 4.1). Why: Steam writes the manifest, the folder and even the executable at the start of a download; counting that would show the game for the whole download. The watched `steamapps` folder sees the manifest rewritten when the download completes, so the game appears then.
- **Installed is not the same as having saves.** An installed game whose save set matches nothing yet stays visible; Save is unavailable until there is data.
- **Unsure is not uninstalled.** A disconnected drive or unreadable store folder keeps the previous state. A game is uninstalled only when its absence is confirmed.
- **Uninstalled games are hidden, never forgotten.** Their configuration, checkpoints and history stay, and come back if the game does.
- **Scans never touch user choices.** Path overrides and custom games survive every scan.
- **Scans never delete files or history.** They may notice that checkpoints changed on disk (see CHECKPOINTS AND HISTORY).
- **A save set can change, by the catalog's rules.** It changes when the build or the Steam account changes (the targets then name other folders) or when a catalog update edits the game's locations. The host accepts the catalog's answer. Old checkpoints are never moved or rewritten; which of them can still be restored follows the rule in Checkpoints belong to their targets. User overrides and custom games are never re-resolved.
- **A target's presence is present, missing or unknown.** Only a confirmed absence counts as missing. A target on an unplugged drive or an unreadable folder is unknown, and Save, Load and Revert are unavailable for that game until it can be read again. Why: recording an unreadable target as absent would make a checkpoint that silently skips it.
- **No save location at all.** If the catalog finds no target for an install (the game is unsupported on this platform or store), the game stays visible with a configuration error and no operations, so the user can set a save location in Configure.

The catalog can turn two installs of one game into two game records, each with its own save set and history (for example a Steam copy and a GOG copy). When that happens, the host gives each of them an **install tag**: the store's name (`Steam`, `GOG`, `Epic`), or the install folder's name when both come from the same store. A game with one install has no tag. Why: two identical names in the library would be impossible to tell apart, and the tag says which copy the user is about to touch.

### Custom games

The user can add a game the catalog doesn't know. A custom game has:

- a host-generated ID that never collides with catalog IDs and doesn't depend on the name;
- a name (non-blank; doesn't need to be unique);
- one absolute executable path, used for install status and monitoring;
- one **save location**, which is its whole save set: an absolute folder, file or glob pattern in the catalog's syntax (`D:\Game\saves`, `D:\Game\saves\slot1.sav`, `D:\Game\saves\*.sav`, `...\Profiles\C*\SGS*`). The root is everything before the first segment with a wildcard;
- no instructions.

One location is enough for games the catalog doesn't know. A custom game whose saves are split across folders isn't supported yet; allowing several locations later wouldn't change the model.

Neither path has to exist yet, so the user can register a game before installing it or before its first save. Validation never creates anything. An invalid addition is rejected whole; nothing partial is left behind.

The executable is the install evidence: the game is installed when the file exists, uninstalled only when its absence is confirmed, and unchanged otherwise. Every scan rechecks this. A scan never turns a custom game into a known one, even if its paths match a catalog game.

Custom games are kept forever. There is no way to remove one in this version.

### Configure

- **Known games:** the user can override the executable, and override the save set with one save location (folder, file or pattern) that replaces all of the catalog's targets. Reset returns each to the catalog. The catalog's resolved targets stay visible, so the user sees what they're replacing. Name and instructions always come from the catalog. Why replace the whole set instead of editing single targets: merging a user's edits with a later catalog update is guesswork, and "yours" or "the catalog's" is simple to explain.
- **Custom games:** name, executable and save location are all editable.
- A change is applied only after every new value validates. Otherwise the old configuration stays exactly as it was.
- Changing the save set never moves or rewrites checkpoints. Which checkpoints can still be restored follows Checkpoints belong to their targets; switching back makes old checkpoints usable again, with the same IDs and history.

### Save set safety

Every target (from the catalog, an override or a custom game) passes the same checks. They're about what a Load may overwrite or delete: Load only touches what a target's filter matches, so a target is judged by its root and filter together.

- **Broad folders** are the shared places other programs use, plus the game's own install folder:
  - the game's install folder (for a custom game, the folder holding its executable). Why: it holds the game's executables and data archives, which a wildcard could match;
  - drive, volume and network-share roots;
  - user home and profile roots;
  - OS folders;
  - AppData roots (Roaming, Local, LocalLow) and ProgramData;
  - Documents, Saved Games and My Games roots;
  - Program Files, Steam library roots, `steamapps`, `common`;
  - `~/Library` and its direct children (`Application Support`, `Group Containers`, `Preferences`), `~/.config`, `~/.local/share`;
  - the equivalents inside a Proton prefix.
- **In a broad folder, only an exact name is allowed.** `Documents\mygame.sav` or `Application Support/com.vlambeer.nuclearthrone` touches only that one entry, so it's safe. The whole broad folder, or a wildcard directly in it (`Documents\*.sav`, `D:\Game\save*`), is rejected: it could catch other programs' files or the game's own. So `D:\Game\Save.ini` is allowed and `D:\Game\save*` isn't. A game's own child folder, such as `{APPDATA}/Void_War` or `D:\Game\saves`, is not broad, however shallow.
- **Known bad patterns are rejected:**
  - a filter that would match the game's executable;
  - anything that could match the reserved `.ssnew` and `.ssold` suffixes on its own.
- **No overlaps between games,** including games that are currently unavailable. Two games may share a root only when both use exact names and the names differ. Otherwise one game's root can't equal, contain or sit inside another's. Why so strict: two patterns can't be proven never to match the same file, so we don't try. The error names the other game.
- **Compare real directories, not strings.** Resolve aliases and follow the filesystem's case rules, so `Game` and `Game2` don't conflict and two case-different folders on Linux stay distinct. Base the checks on resolved OS folders, not folder names.
- **Links resolve to their target.** If a root is a symlink or junction, all operations run on the real folder; the link itself is never renamed, replaced or copied. If the link later points elsewhere or breaks, the game fails validation until it's configured again. Links inside a target are never followed or copied.
- **Paths that don't exist yet are fine,** so the user can register a game before its first save. An exact name matches whatever eventually appears there, file or folder. Validation never creates anything.
- **Recheck before every operation,** so a folder changed since configuration can't slip past.

These are hard errors, not warnings. If a catalog target is invalid, it's left out of the save set with a warning; a game left with no valid target and no valid override stays visible with a configuration error and no operations.


## MONITOR AND ACTIVE STACK

After the first scan, the host watches every game's executables start, get focus and exit, and keeps the ACTIVE STACK:

- One entry per running game. Several processes of one game count as one.
- A game switched to (its window gets focus) moves to the top.
- A game that starts appears in the stack but doesn't jump ahead of games focused more recently.
- When the last process of a game exits, it leaves the stack.

For example: FTL is running, so the stack is just FTL. Void War starts and gets focus, so it goes on top. The user alt-tabs to FTL, so FTL goes on top. FTL exits, so Void War is on top again, even if another app is in the foreground now.

Processes are matched to games by the full executable path, so an unrelated program with the same file name elsewhere doesn't count. When two installs of one game exist, each install's executables map to its own game record, so hotkeys target the copy that's actually running.

Game starts and exits add **Game started** and **Game closed** history markers. They create no checkpoints and have no actions. Relaunching continues the same history.

The host keeps a record of when it was running, so time it didn't observe never joins two unrelated sessions and no exact start or exit time is invented:

- **Each run is recorded:** when it started, when it was last known to be alive, and when it ended cleanly. A heartbeat every 5 minutes moves "last seen". A run without an end crashed or lost power somewhere after it was last seen.
- **A session belongs to one run.** A restart always starts new sessions, even for a game that kept running.
- **A game already running when the host starts** gets no Game started marker: its start wasn't seen.
- **A game that exits while the host is down** gets no Game closed marker: its exit wasn't seen. Its session simply ends there.

Why record runs at all, when sessions already stop at a restart: "last seen" bounds how long the host was blind after a crash, and the runs can be listed (CLI `host-runs`) to answer "was SaveScummer even running when I played?".

When a Steam game starts, the host asks the catalog resolver for the current Steam account again and re-resolves that game if the account changed since the last scan. Why: the user can switch Steam accounts between two sessions, and the game runs under whoever is logged in at launch. Without this, the first Save after a switch would back up the other account's folder. The account order and the context rule are the catalog's (PLAN-CATALOG.md, 4.4 and 4.5).

The monitor starts correctly whether games were started before or after it, and rebuilds the current stack after a restart.


## CHECKPOINTS AND HISTORY

### Where checkpoints live

Each game has a folder in the checkpoint store, and each checkpoint is a folder inside it, with one subfolder per target and a small record of where each target came from:

```text
checkpoints/
`-- Slay the Spire (steam-646570)/
    |-- 2026-09-24 19.25.03 saved/
    |   |-- checkpoint.json       each target's root, filter, excludes, and whether it was absent
    |   |-- saves/                what the "saves" target matched
    |   |-- preferences/
    |   `-- runs/
    `-- 2026-09-24 19.31.10 recovery/
        `-- ...
```

- **Names are for people.** The game's name with its ID, and the time with the kind, so the store makes sense in a file manager. Identity lives in the database and the record, never in the name; a name that's already taken gets a suffix.
- **The record makes a checkpoint self-describing.** Its targets are stored as they were at Save time, so a checkpoint never depends on the game's current configuration to be understood.
- **Temporary folders** (a Save being copied, a folder being deleted) use reserved names. They are never treated as checkpoints and are cleaned up on the next start or scan, unless an unresolved interruption still needs them.
- **Never overwrite.** A new checkpoint always takes a free name, even if a folder appears there at the last moment.
- **Completed checkpoints are read-only.** Restoring copies from them; the checkpoint stays as it was.
- **Only the app makes checkpoints.** A folder the user copies into the store isn't registered. Why: copying the save folder in the file manager was a way to save when a game had one folder; with several targets there's nothing single to copy, and dropping it keeps the app out of the user's folders.

### What a checkpoint holds

- **Only saves.** What the catalog calls configuration is never backed up or restored, so a Load never resets settings. Config files that sit inside a save folder are the target's excludes.
- **Never Steam's own files** (`steam_autocloud.vdf`, `remotecache.vdf`), even inside a save folder. They belong to Steam's cloud sync, not to the game.
- **Never logs or crash dumps**, even inside a save folder: files ending in `.log`, folders named `logs` (any case), the known log names `log.txt`, `client_log.txt` and `output_log.txt` (older Unity games), and folders named `Crashes`. Why: many games' save folders also hold the log the running game keeps open (Unity's `Player.log`, Unreal's `Saved/Logs`, Godot's `logs/godot.log`, Isaac's `log.txt`). A Load at the main menu would then fail to rename it and be refused every time, and restoring an old log is pointless anyway. The list is short on purpose: a name that could plausibly be a save never goes on it.
- **Never our reserved suffixes** (`.ssnew`, `.ssold`), even when a pattern like `save*` would match them. Why: leftovers from an interrupted Load must never be backed up or restored as saves.
- **A target that didn't exist at Save time is recorded as absent**, so a Load knows to leave it alone.

### Checkpoints belong to their targets

A save set can change: another build, another Steam account, a catalog update, an override or a Reset. A checkpoint is restored only into targets it has in common with the game's current save set (same real root, same filter):

- **All of them in common:** the normal case. The checkpoint is usable.
- **Some in common** (a catalog update added or dropped a location): the checkpoint is usable, and restores only the common targets. Current targets it doesn't have are left alone, like targets that were absent at Save time.
- **None in common** (another account's folder, a Proton prefix instead of the native folder, an override): the checkpoint is unavailable, with the reason. It stays recorded and becomes usable again when its targets return, with the same IDs and history.

Why this one rule: it replaces separate rules for account switches, build switches, overrides and catalog updates, and it never restores into a folder the checkpoint didn't come from.

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
- **Only in the database.** The checkpoint folder's name and contents are never touched. Why: a completed checkpoint is read-only, and its folder name isn't its identity.
- **Only saved checkpoints.** Recovery checkpoints have no label.
- **Limits:** one line, at most 100 characters. The host trims it and turns line breaks into spaces. Empty means no label. The host enforces this for every client; the UI's limit is only a convenience.
- **Not an operation.** Setting a label changes no files and adds no history event, so it doesn't wait for the game's lock. It works while the game is busy and while the checkpoint is counting down to deletion.
- **Last write wins.** A label request for a checkpoint that no longer exists is rejected quietly, as "gone".
- **It outlives deletion where rows do.** When a checkpoint is deleted, its record keeps the label, so a Loaded row still says what it loaded. Flush clears it with everything else.
- **A new generation starts unlabeled.** A folder replaced or edited outside the app becomes a new checkpoint, and the old label isn't carried over. Why: the host can't know the new contents still match the name.
- **Save can carry a label** (used by the CLI). Hotkey and UI saves start unlabeled.

Actions in the same second stay distinct: rows have stable IDs and a stable order that doesn't depend on timestamps.


## OPERATIONS

Every operation, from any entry point, goes through the same handling and the same per-game lock. The lock covers Save, Load, Revert, Delete, Flush and configuration changes. A request for a busy game is rejected immediately; nothing is queued for later.

The app works on files. The user is responsible for the game picking up a restored state, for example by reloading or restarting it. Copying is ordinary and best-effort: the host doesn't pause the game or detect writes during a copy. A copy made while the game is writing may be inconsistent; real copy errors fail the operation.

**Known issue, ignored for now: very large saves.** Every Save copies the whole save set, and every Load copies it twice (the recovery checkpoint, then the `.ssnew` copies). For games with huge save folders, such as a Project Zomboid world with hundreds of MB in thousands of files, a hotkey press can take minutes and every checkpoint takes the full size on disk. The catalog is mostly roguelikes and strategy games with small saves, so this is accepted. If it becomes a problem, the options are skipping files unchanged since the previous checkpoint, hard links between checkpoints, or copy-on-write clones where the file system supports them.

### SAVE

1. If no target matches anything, there is nothing to save: Save is unavailable. A target whose presence is unknown makes Save unavailable too.
2. Copy what every target matches (minus its excludes) into a temporary folder in the store, and write the record, including which targets were absent.
3. When the copy is complete, rename the folder to its checkpoint name. Why the detour: a half-finished copy must never look like a checkpoint.
4. Only then register the checkpoint and add a Saved row. A failed or interrupted copy never becomes a checkpoint; a finished copy whose Saved row wasn't recorded is recorded at the next start (recovery rule 3).

### LOAD

A Load restores the **whole checkpoint**: every target in it, never a part. Why not one profile or campaign at a time: the host can't reliably tell which files belong to which campaign, and a campaign can span several targets. The rare cost, accepted: someone playing two campaigns in turn who loads an old checkpoint of one also rolls back the other, whose newer state stays in the recovery checkpoint.

**A Load makes the saves exactly as they were.** Inside each target, everything the filter matches is made identical to the checkpoint: the checkpoint's files are put back, and matched files the checkpoint doesn't have are deleted. Why delete: many games continue from their newest file (rotating autosaves, date-named saves), and a run started after the checkpoint must disappear. Without deletion a Load reports success and the game carries on from where the user was. Why this is right for every game in the catalog: they're there because the player can't freely save (permadeath, ironman, one-life, committed choices), so there is no in-game save list whose newer entries the player would want kept. Deleted files are never lost; they're in the recovery checkpoint.

**A target that was absent at Save time is left alone.** Why: a folder that appears later is usually a change of setup (Steam Cloud switched on, Proton, a new save path), not game progress, and deleting inside Steam's `remote` folder can delete the files from the cloud too. A target that existed but matched nothing (no run in progress) is a normal state, and the Load makes it match nothing again.

Steps:

1. Pick the checkpoint:
   - **Load** (button or hotkey): the newest usable saved checkpoint. Order by save time, breaking ties by the order the host registered them. Recovery checkpoints are never picked. With none, Load is unavailable.
   - **Load this save** (a history row): exactly that row's checkpoint.
2. Check it: it belongs to this game, it's a saved checkpoint, it shares targets with the current save set, it's unchanged on disk, and every target it will touch has a known presence. A target root that held data at Save time and is now confirmed missing refuses the Load too. Why not recreate it: a missing folder can mean the game moved its saves or the path is wrong, and loading into it would hide that; restoring only the other targets would be half a save. Otherwise stop before touching anything.
3. Copy the current state of the save set to a new recovery checkpoint, with the same filters. If the copy fails, stop; nothing live was touched. Because the filters are the same, every file the Load deletes is in this recovery checkpoint.
4. Apply the checkpoint in four stages. Each stage finishes for **all** files before the next one starts, so a failure always leaves a state that can be undone:
   1. **Copy in.** Copy each checkpoint file next to its live counterpart as `name.ssnew`. If this fails, delete the `.ssnew` files; the live saves were never touched.
   2. **Set aside.** Rename each live file the Load will replace or delete to `name.ssold`. If this fails, rename the `.ssold` files back; nothing was lost. A file the running game holds open fails here, while everything can still be reversed, so no separate lock check is needed.
   3. **Swap in.** Rename each `.ssnew` to its real name. If this fails, rename those back to `.ssnew`, then undo stage 2.
   4. **Clean up.** Delete the `.ssold` files. A failure here doesn't undo the Load; leftovers are removed later.
5. After stage 3, add a Loaded row that references the checkpoint loaded and the new recovery checkpoint. It also says how many files were removed, if any ("removed 2 newer saves, kept in the recovery point").

Why stage next to the live files: the copies sit in the same folder as the files they replace, so every rename is on the same drive and atomic, and undoing a failed Load is just reversing renames. It needs no staging area of its own and no restore from the recovery checkpoint. The costs, accepted: our temporary files are briefly visible in the game's folder while stage 1 copies; between stages 2 and 3 the save names briefly don't exist; and the drive needs room for a second copy of the files being restored.

Load works while the game is running. Why: the usual flow is to die, go back to the main menu, press the Load hotkey and continue, with the game's process running throughout. The user is responsible for the game picking up the restored state.

The checkpoint itself is never modified, and every later row stays.

**Steam Cloud after a Load.** Steam compares every file it syncs with its own record of the file. A file restored behind its back looks changed on this machine: with nothing newer in the cloud, Steam uploads it, which is what we want; with newer progress from another device, Steam asks the user which copy to keep. The host never touches Steam's own files. Instead, at the next start of the game after a Load, it compares the restored files with the checkpoint. If Steam replaced a file or brought a deleted one back, the Loaded row says so. Why check afterwards: it turns a silent failure into a visible one without touching anything of Steam's.

### REVERT

Revert puts back the state from just before a Loaded or Reverted row's operation. It is a Load whose source is a recovery checkpoint, so it follows every Load step, including preserving the current state first. Why: the state being replaced may hold progress the user wants back, and one rule for every restore means one set of safety and interruption rules.

1. Take the row's recovery checkpoint and check it like a Load target (it must be a recovery checkpoint instead of a saved one).
2. Copy the current state to a new recovery checkpoint, then apply exactly as in Load.
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

Flush deletes every checkpoint of a game (saved, recovery and leftover temporary folders) and clears its history. The game's own saves are not touched.

- Before confirming, a client can ask for a preview: counts per kind, and the paths (with labels) in pages.
- The preview is not a contract. On confirm, the host finds everything again from scratch and deletes what it finds. Why: simpler than binding the confirmation to a snapshot of the preview, and it can't delete anything the preview didn't describe in kind.
- Only records whose files were really deleted are cleared. Failures are reported and the remaining checkpoints stay.
- Flush isn't available while a game is waiting on an unresolved interruption.

**Checkpoint size.** The state carries, per game, the total size in bytes of what a Flush would delete: saved checkpoints, recovery checkpoints and leftover temporary folders. Why: the size is often the reason to flush, and the user should see it before opening the dialog.

- It comes from the file sizes the host already records for change detection, plus leftover folders measured when they're found. Nothing is read from disk to answer a client.
- It updates after every Save, Load, Revert, Delete, Flush and scan that changes it.
- It's unknown while the checkpoint store is unavailable, and clients then show none.


## INTERRUPTIONS AND FAILURES

Files and the database can't change in one transaction. So before each step that changes files, the host durably records what it's about to do: the operation, the stage, the exact files involved (live paths, their `.ssnew` and `.ssold` names, the source checkpoint, the recovery checkpoint, the temporary Save folder) and the identities needed to verify them. After the step, it records the result. Failed or unfinished operations never appear as completed history.

The files on disk describe themselves too: a `.ssnew` next to a live file means stage 1 or 3 ran, a `.ssold` means stage 2 ran. The journal says which Load they belong to, so leftovers are never guessed at.

### Recovery at startup

Before a game can be operated on again, the host resolves any unfinished operation for it. The rules run in order, with or without a UI:

1. **Nothing live changed** (interrupted while copying a Save, a recovery checkpoint, or a Load's stage 1): delete the temporary folder or the `.ssnew` files, mark the operation failed and release the game. An interrupted Save leaves a one-line notice; an interrupted Load is silent.
2. **A Load was interrupted in stage 2 or 3, and every file involved is verifiably where the journal says:** undo it by reversing the renames (swapped-in files back to `.ssnew`, `.ssold` files back to their names), then delete the `.ssnew` files. The operation never happened.
3. **The result is verifiably in place but wasn't recorded** (a Save's folder already has its checkpoint name, or a Load's stage 3 finished for every file): finish the operation, including stage 4, and add its history row, exactly as if it had completed.
4. **Anything else:** leave every live file exactly as it is, mark the operation failed without a history row, and keep every `.ssnew`, `.ssold`, recovery and temporary file. Release the game if every target is in a coherent state; otherwise keep it blocked with a sticky error and try again at every start and whenever the user presses Retry.

Never roll back over data that may be newer, never retry the requested Load or Revert on its own, and never delete kept material while an interruption is unresolved. Leftover `.ssold` files from a finished Load (stage 4 failed) are removed quietly at the next start or scan.

### Failures the host reports

Every failure reaches clients as a structured result: a stable kind, the game, and the exact paths involved. The wording, buttons and layout belong to the UI. The host must tell these situations apart:

| Situation | Host behavior |
| --- | --- |
| A save file is in use, so setting it aside fails (Load stage 2) | Renames undone; nothing changed. Retry can help. |
| A file couldn't be read during a copy | Nothing changed. Retry can help. |
| The destination is full, or writing is denied (the store, or a target's drive for `.ssnew` copies) | Nothing changed. Retry can help. |
| A target's drive or share is unavailable | Nothing changed; the game's operations are unavailable until it's back. |
| The checkpoint store is unavailable | Nothing changed; every game's operations are unavailable, checkpoints aren't retired. |
| A target contains a link or special file | Links and special files aren't followed or copied. |
| The checkpoint changed or vanished since it was shown | Refused before anything is touched. |
| The checkpoint shares no target with the current save set | Refused; points at Configure. |
| A target root that held data at Save time is missing | Refused; nothing is recreated. |
| A target is invalid, overlapping, or a link that changed | Refused until reconfigured. |
| A Load couldn't be undone (recovery rule 4) | Game blocked, all material kept, retried at every start. |
| A Load was applied but couldn't be recorded | Reported; startup rule 3 finishes recording it. |
| Steam Cloud replaced a restored file (found at the next game start) | Noted on the Loaded row; nothing is changed. |
| A deletion couldn't finish | Record kept; leftovers cleaned later. |
| The database can't be opened | The host can't serve; says so. |
| The host is shutting down | New requests are rejected. |
| The client and host versions don't match | Refused with an "upgrade required" error. |

A failed or blocked game never affects other games.

### Opening folders

A client names *what* to open and the host opens the real path. Clients never have to type or pass paths. What can be opened:

- **A target's root:** the folder a target lives in (for a pattern, the folder before the first segment with a wildcard). Used by error blocks for the affected target.
- **The game's checkpoints folder:** the game's folder in the checkpoint store. This is also what errors about the whole store open; the user starts from their game's backups anyway.
- **A checkpoint** or **a kept recovery checkpoint.**
- **The executable's folder:** the folder holding the game's configured executable.

A folder that doesn't exist (a save folder the game hasn't made yet, a drive that's gone) opens the nearest parent that exists, so an open never fails just because a folder is missing.

A client can also ask for the resolved folder without opening anything. Why: opening a file-manager window can't be checked by a test, and the resolution must be. Returning a path is safe; the rule is only that the host never acts on a path a client sends.


## STORAGE

One SQLite database holds game configuration, settings, checkpoint records (including labels), history, the operation journal and the host's runs (the newest 500). Game files never go into it.

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

### Per OS

| | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Process and focus watching | Yes | To investigate | To investigate |
| Global hotkeys | Yes | To investigate | To investigate |


## ARTWORK

The UI shows Steam art for known games: hero art and logo for sidebar cards, the header image as a fallback, and a small icon next to the game's name. The host gets and caches it; the UI never downloads anything.

- Read Steam's local library cache first (`Steam/appcache/librarycache/<appid>/`, including its hashed subfolders), then Steam's public CDN (`shared.fastly.steamstatic.com/store_item_assets/steam/apps/<appid>/`: `library_hero.jpg`, `logo.png`, `header.jpg`). No Steam login or API key. Why both: the local cache works offline, but Steam has changed its layout before, and the CDN has every game's art, even games Steam hasn't cached.
- Scale images down to display size and keep them in the host's own cache. Deleting the cache is harmless; it's rebuilt.
- Check the cache at host start, after every scan and when a UI attaches. Download what's missing or unreadable. A download is written to a temporary file and published only once it decodes as an image.
- Artwork is best-effort: it runs in the background, never blocks scans, operations, monitoring or the UI, and retries later on failure without nagging.
- Custom games and games without art get none; the UI shows its placeholder.


## CLI

`SaveScummer.CLI` is for scripts, testing and diagnostics. It's a client like the UI:

- **It can drive the host in nearly every way the UI can.** Why: the host can then be tested end to end, and a problem reproduced, without the desktop UI or a custom test harness. Concretely:
  - every protocol command and query has a CLI command, including the ones only the UI normally sends;
  - it can watch: print the current state, then each new state as it arrives, until stopped;
  - it can stand in for the UI's focus and selection report, so hotkey targeting can be tested without a window;
  - it can wait for an accepted operation's outcome, or return right away with the operation ID and ask later. For a Delete, the outcome is the files gone (or the failure), not the countdown starting; while it waits, it prints each state: counting down, waiting for the game, deleting;
  - it can pass a request ID of its own, so repeated requests can be tested;
  - it can resolve any folder the UI would open and print its path without opening a window.
- **No generic retry.** After an ordinary failure, running the same command again is the retry. Only a blocked game has its own retry command.
- **What it doesn't replace:** real hotkey presses, the tray, sounds and notifications. Those are OS adapters, not protocol, and stay under "By hand".
- It never opens the database or touches game files itself. A Save from the CLI can set a label, and a separate command sets or clears one later.
- It starts the host when a command needs it, and never becomes a second host. `--no-start` makes it fail instead of starting one, for tools like the installer that only want to talk to a running host.
- It prints readable output by default and machine-readable output on request, with exit codes that tell success, rejection and failure apart.
- Long history can be read page by page or streamed without loading it all. If the history changes mid-stream, it says so instead of printing duplicates or gaps.


## PROTOCOL

This is the contract between the host and its clients (UI, CLI). A client can be built against this section alone.

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
- Configure a game (executable, save location, name for custom games, reset overrides)
- Scan
- Change settings (Play sounds, Launch on startup)
- Move the checkpoint store
- Report the UI's focus and selected game
- Open a folder by what it is (see Opening folders), or only resolve it
- Retry a blocked game (an ordinary failure has no retry command: the client sends the same command again with a new request ID)
- Refresh the catalog now
- Shut down (the same safe Exit as the tray menu)

Queries:

- The current state (see Watching)
- A game's history, in pages
- A Flush preview, with paths in pages
- An operation's outcome by ID
- A game's save set: the catalog's resolved targets and any override, with each target's presence
- The active catalog revision
- When the host was running: its runs, newest first, each with its start, last-seen time, clean end (if any) and whether it's the current one

### Commands are safe to repeat

- Operations are **accepted or rejected** at once: accepted with an operation ID, or rejected with a reason (busy, unavailable, blocked, invalid).
- "Accepted" is sent only after the operation is durably recorded, so a crash right after can't lose it.
- Every request carries a client-made ID. Repeating it, even after a restart, returns the same operation instead of running it twice.
- A client that disconnects can reconnect and ask for the outcome by operation ID. It never re-sends a destructive command to find out.
- A disconnect never means success and never cancels anything.
- Clients refer to games and checkpoints by ID. The host resolves every path itself; clients never supply a path it trusts.

### Watching

A client that watches gets the full current state, then a new state whenever something changes:

- The state is a **summary**, and it's self-contained: games (configuration, install tag, install and availability status, instructions, artwork, and whether Save and Load are available with the reason when not: no game data, no save location, a target unreadable, the checkpoint store unavailable, no saves, busy or blocked), settings, the ACTIVE STACK, scan state, each game's latest checkpoint (with its label) and whether it has history, the size of what a Flush would delete, busy and blocked games with their errors, pending delete countdowns, and each game's last result.
- It never contains full history or old records, so its size doesn't grow with history. History is always queried separately, in pages.
- Every state carries a revision and a host instance ID. A new instance ID means the host restarted: throw away everything cached and start again.
- Progress can be coalesced; each delivered state stands on its own.
- The last message before a shutdown says so, and the UI closes instead of reconnecting.

### History pages

- Newest first, in the stable history order, so equal timestamps never cause gaps or duplicates.
- Each row carries its times, its checkpoint reference, the label and time of the save it's about (its own for Saved, the loaded save's for Loaded), the time of the row it reverted (for Reverted), for Loaded rows how many files the Load removed and whether Steam Cloud replaced a restored file, and whether each action is available right now, so the client needs nothing else to show it.
- A page position belongs to one game, one host instance and one version of that game's history. A relevant change to that game invalidates it with an explicit "reload" answer; changes to other games, progress, artwork and labels don't. The watch stream says a game's labels changed; the client re-reads the pages it shows with the same page positions, so editing a label never resets scrolling or history.
- Anything large is paged. A reply that would be too big is an explicit error, never silently cut off.


## TESTING

Rules must be provable without a desktop, and the OS parts must be proven for real. Use the normal test runner, temporary folders and databases, and one small fake-game program. No custom test framework, dashboard or scenario language.

### How

- **Core logic** runs in a plain test process with a fake filesystem, database and clock. This is where the history rules, checkpoint selection, locking, delete countdowns and the four recovery rules are pinned down, including failures injected at every step.
- **File operations** run against real temporary folders: copies, never-overwrite naming, the four Load stages and their undo, leftover `.ssnew`/`.ssold` files, the delete-by-rename, locked files, links and junctions.
- **Storage** runs against real SQLite files: transactions, restart, migrations, and proof that a Save writes nothing it didn't change.
- **Integration** runs the real host with the fake game and drives it through the CLI's machine-readable output, the same way a user or the UI would. The fake game is copied to different paths to act as different games, run several times to act as one game with several processes, and can delay its window, launch a child and exit, crash, write saves slowly or hold a file open. The monitor must find it through the same OS mechanisms as real games.
- **Crashes** are simulated by killing a real host between recorded steps, then starting it again on the same folders and database.
- Tests wait for events with bounded timeouts and use simple synchronization for races, not long sleeps. Tests that fight over focus or hotkeys run one at a time. Every test cleans up its own processes and files, even when it fails.

### What must be proven

- **Monitor:** starts before and after games; normal exit, kill and crash; quick relaunches; a process that exits before showing a window; one entry for several processes; launchers that start the game and exit; same file name at a different path; focus switching between games and unrelated apps; the stack after a host restart; exactly one start and one close marker per session; a Steam account switched between two sessions re-resolves the game at its start, before any Save; a host killed mid-session while the game then exits unseen: no Game closed for that session, the next launch is a separate session, and the killed run has no end while a clean exit records one; starts, closes and "already running" appear in the host log.
- **Library:** scans don't duplicate games; installs and uninstalls are noticed; unavailable drives aren't uninstalls; overrides and custom games survive scans; every save set safety rule, including exact names allowed in broad folders and wildcards rejected there, patterns in custom locations, overlaps between games, aliases, case rules, redirected folders, Proton equivalents and targets that don't exist yet; a target of unknown presence makes operations unavailable; no test ever copies or replaces a real system folder.
- **Scanning:**
  - the periodic scan's handler (without waiting 15 minutes);
  - a focus report runs an install scan, and a second one within 20 seconds doesn't;
  - a fake Steam library gaining an app manifest and executable is found within a few seconds with no request; a burst of changes causes one scan; a library added through `libraryfolders.vdf` is watched from then on;
  - a Steam download in progress (manifest, folder and executable there, not yet marked finished) isn't installed until Steam marks it finished; an installed game that is updating stays;
  - a standalone install is found through the uninstall key its catalog entry names, and its install and uninstall are noticed through registry change notifications with no request (tests use a scratch key under the user's registry, never the real installed-programs list); the real machine-wide keys can be watched without admin rights;
  - installs, reinstalls and uninstalls appear in the host log, in order, with the scan's reason;
  - install scans never re-check checkpoints, full scans do;
  - a scan blocked on a slow probe delays neither requests nor monitor markers;
  - a request during a scan joins it and gets its result; a full-scan request during an install scan runs after it;
  - background scans publish their origin, and a user-requested scan's result counts only newly found games;
  - Windows: a drive's watches are released on a removal request and restored when it returns.
- **Checkpoints:** only configured targets are copied, never configs, Steam's own files, logs and crash dumps, or the reserved suffixes; a Load at the main menu of a game whose save folder holds its open log; absent targets recorded as absent; the "in common" rule after an account switch, a build switch, an override and a catalog update (usable, partly usable, unavailable, usable again on return); moving the store, including an interrupted move; an unreachable store; folders copied into the store by hand are ignored; external deletion, replacement (between scans and while the host is off) and in-place edits; unreadable folders not retired; changed recovery checkpoints never imported.
- **Operations:** Save, Load, Load this save, Revert, reverting a revert and Delete compared by actual file contents; every rejection happens before anything is created; a save set spanning several folders and two drives; a Load deleting newer matched files; a Revert removing files the Load brought back; a target absent at Save time left alone, and one that matched nothing emptied again; a missing root refusing the Load; a Load while a fake game holds a save file open, refused at stage 2 with nothing changed; the Steam Cloud check at the next game start with a file replaced and a deleted file brought back; visible history after deletions, including empty sessions disappearing; Flush with partial failures; labels (limits and trimming, shown on Saved and Loaded rows, kept after deletion, cleared by Flush, not carried to a replaced folder, set while busy, rejected for a gone checkpoint, never written into folders); delete countdowns (cancel versus run, waiting while busy, dropped by Flush, finished on shutdown, dropped after a crash); the checkpoint size after each operation, with leftover folders, and unknown while the store is unavailable.
- **Opening folders,** through the CLI's resolve-only answer: a folder, file and pattern target's root, a folder that doesn't exist yet, the checkpoint folders and the executable's folder.
- **Failures:** a failure injected at every step and every Load stage, and a kill at every recorded step (including mid-stage, with some files renamed and some not), each checked against the four recovery rules by both the files and the records; other games stay usable.
- **Entry points:** UI, CLI and hotkeys all hit the same lock; a busy game rejects and never replays; held keys; the right game is targeted with the window focused and unfocused; the right sound for each outcome, including never a success cue on failure.
- **Protocol:** repeated request IDs; reconnecting mid-operation; a host restart invalidating state and history pages; paging with equal timestamps and sessions crossing pages; history bigger than any single reply; a version mismatch for each client.
- **Scale:** histories of 100, 10,000 and 100,000 rows, in one game and spread across many. Measure startup, memory, writes per Save, the cost of an idle watch and the first history page. Timings are reported separately so a busy machine doesn't fail functional tests.

### By hand, for now

Some things aren't worth automating yet: real hotkey delivery (including fullscreen games and elevated processes), tray behavior, notifications, the sign-in entry, antivirus interference, sleep and resume, and audible sound quality. Keep them as a short checklist per platform.

Steam Cloud must be checked by hand on a real machine before relying on the after-Load check, with one game of each kind: Isaac (writes through Steam's cloud API into `remote`), Slay the Spire (Steam Auto-Cloud of its install folder) and Risk of Rain Returns (both). For each: a Load with the game closed, then launch; a Load that deletes a file; and, where possible, a Load while the cloud has newer progress from another device. Why by hand: only real Steam shows whether it uploads, re-downloads or asks.

Windows comes first. Each new platform proves its own monitoring, hotkeys and file operations; passing on fixtures alone doesn't count.
