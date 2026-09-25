# Real-game checks

Things the catalog and the host assume about specific games that only a real install can confirm. Each entry says what to look at, why it matters, and what the answer changes.

The catalog is built from the Ludusavi manifest, which lists where saves may be but not how a game uses them. The builder keeps those paths exactly as targets (PLAN-CATALOG.md, Section 3.1), and the runtime backs up and restores every applicable target together. So most wrong guesses are now harmless: an exact name matches whatever is on disk, parts of one save are never split, and a local copy and a Steam Cloud copy are both restored. What's left is what the design can't settle on its own:

- **a path the manifest gets wrong**, which leaves a game without saves on that platform;
- **how running games and Steam react** to files being replaced under them.

How to record a result: add the date, platform, store and what was seen under the entry, then change the catalog (an addendum entry) or the plan if the answer requires it.


## 1. Paths the manifest may get wrong

A wrong path no longer backs up or restores the wrong thing: its target is just absent, and the game shows "no game data" on that platform. These checks find out whether a game silently has no saves.

- **Streets of Rogue (macOS).** The manifest says `~/Library/ApplicationSupport/unity.streetsofrogue.streetsofrogue`, with no space in `ApplicationSupport`, which looks like a typo in the manifest. Where are the saves really?
  - If they're under `Application Support`: add the path through the addendum `override`.
- **NetHack (Windows).** Are the saves in `%LOCALAPPDATA%\NetHack\3.6`? Does a newer NetHack (3.7) use `...\3.7` instead?
  - If each version gets its own folder: the catalog needs a target per version. Every target is used anyway, so listing both costs nothing.
- **Jupiter Hell (Windows)** (`<install>/save*`) and **Teleglitch (macOS, Linux)** (`<install>/exitsave*`). What do the matched files look like on disk: a `save` folder, or loose files in the install folder?
  - Why: a wildcard directly in the install folder is rejected (it could match game files), so today these games have no saves on those platforms.
  - If there's a real subfolder, or the files have fixed names: add exact paths through the addendum.
- **Crusader Kings II, Europa Universalis IV, Stellaris.** The addendum narrows these to `.../save games` (the manifest lists the whole user folder, with mods and logs). CK2 on Windows confirmed 2026-09-24: `save games` next to `logs`, `gfx`, `map` and `settings.txt`. Still to confirm: EU4 and Stellaris, and whether Paradox's cloud saves go to Steam's `userdata/<id>/<appid>/remote/save games` instead.
  - If cloud saves live in `remote`: add that folder to each override, or the checkpoint misses a cloud campaign.
- **Total War: Shogun 2 (Linux).** The manifest says `<xdgData>/feral-interactive/SaveData`, which doesn't name the game. Is it Shogun 2's own folder, or one shared by Feral's ports?
  - Why: the addendum leaves Linux out until this is known, because a shared folder would put other games' saves in Shogun 2's checkpoints.
  - If it's Shogun 2's own: add it to the override. If shared: add the Shogun 2 subfolder instead.
- **State of Decay 2, Roboquest, Shogun 2 (Windows), Don't Starve (Linux, macOS).** The addendum narrows these to the standard save folder (`Saved/SaveGames` for the Unreal games, `save_games`, `DoNotStarve/save`). Confirm the saves are there.
- **Terraria (macOS).** The addendum uses `Players/*.plr` and `Worlds/*.wld` instead of the whole Terraria folder. Windows confirmed 2026-09-24: `Players` and `Worlds` next to `tModLoader`, `ResourcePacks`, `Retro` and the config files. Confirm the names on macOS.
- **Enter the Gungeon, Into the Breach, Dome Keeper (macOS, Linux).** The addendum narrows these to `Slot*.save`, `profile_*` and `savegame*`, checked on Windows only. Confirm the same names on the other platforms.


## 2. Files the running game keeps open

A Load renames every file it replaces or deletes to `.ssold` before swapping the checkpoint's files in (PLAN-HOST.md, LOAD). On Windows, renaming a file another program holds open usually fails, and then the Load is undone and refused. That's safe, but if it happens for a file the game always holds while running, Load never works while the game is running, and the usual flow (die, back to the menu, Load hotkey) is broken.

Logs are excluded from every target (PLAN-HOST.md, What a checkpoint holds), so the log a game keeps open no longer matters. What's left to check is whether a game holds anything else open. Unity games whose whole `AppData/LocalLow/<Company>/<Product>` folder is a target are the best candidates:

- Signs of the Sojourner
- Yes, Your Grace
- Darkwood
- Sunless Sea
- Enter the Gungeon
- My Summer Car
- Slasher's Keep
- Heaven's Vault
- One Step from Eden

With the game running, at the main menu:

- Does Load succeed? If it's refused, which file was held open (the save itself, a lock file, a log with an unusual name)?
- What the answer changes: an unusual log name gets added to the built-in log exclude. If the save file itself is held open, that game can only be loaded with the game closed, and its instructions in `games.csv` should say so.


## 3. Steam Cloud after a Load

The host never touches Steam's own files. It restores every copy of a save, including Steam's `userdata/<id>/<appid>/remote` folder, and at the next start of the game compares the restored files with the checkpoint (PLAN-HOST.md, LOAD). Only real Steam shows how it reacts to files changed behind its back.

Check one game of each kind:

- **The Binding of Isaac: Rebirth**: writes through Steam's cloud API, straight into `remote`.
- **Slay the Spire**: Steam Auto-Cloud copies its install folder.
- **Risk of Rain Returns**: writes both a local `<SteamID64>_localsave.json` and `remote/save.json`.

For each:

- **Load with the game closed, then start it through Steam.** Does Steam keep the restored files, download the cloud version over them, or show a sync conflict? After quitting, does Steam upload the restored files as the new cloud state?
- **A Load that deletes a file** (a checkpoint from before a run started, so the Load removes the run's save). Does Steam delete the file from the cloud too, or download it again?
- **A Load while the cloud has newer progress from another device** (a Steam Deck or a second PC), where possible. Does Steam show its conflict dialog?
- Why: if Steam quietly replaces or brings back files, Load looks successful but the game starts from the wrong state. That affects every cloud-synced game.
- What the answer changes: whether the check at the next launch is enough, and what the Loaded row tells the user.


## 4. Our temporary files during a Load (optional)

While a Load copies files, `name.ssnew` copies sit next to the saves for a moment. Between two of the Load stages, the save names briefly don't exist.

- Watch for anything odd during the checks above: a game listing an extra save, complaining about an unknown file, or starting a new profile.
- The riskiest are games whose save pattern is a prefix: OTXO's `Save*` would match `Save1.ssnew` if the game scanned its folder at that moment.
- What the answer changes: nothing is expected. A game that does react gets a note in its instructions, or Load for it waits until the game is closed.


## 5. macOS

What only a real Mac shows (PLAN-MACOS.md). Found on a Mac with Steam, 2026-09-25: FTL at `~/Library/Application Support/fasterthanlight`, Into the Breach at `~/Library/Application Support/IntoTheBreach/profile_*`, Six Ages in its group container `group.com.a-sharp.Six-Ages`.

- **FTL and Into the Breach.** ⌥F5 in fullscreen makes a checkpoint with both cues, ⌥F9 restores it; starting from Steam and quitting write the start and close markers; ⌘-Tab between the two moves the focused one to the top of the stack; after quitting FTL with Into the Breach still running, ⌥F9 loads FTL.
- **Terraria.** The `Players`/`Worlds` names, as in section 1.
- **Six Ages** (a group container: macOS 14+ asks for *other apps' data*). The host finds it in the background without asking and marks it waiting; *Allow access* (`request-access`) shows the prompt once, naming SaveScummer; after allowing, Save and Load work, and a new build asks again. Record what denying returns and which System Settings pane lists the grant.
- **Slay the Spire** (saves inside its `.app`: App Management guards writes). Does a Load prompt, and does the prompt name SaveScummer?
- **Steam Cloud on macOS**: section 3's checks with a Mac copy.
- **Unknowns to settle once** (PLAN-MACOS.md, PRIVACY PERMISSIONS): whether an FSEvents watch on a guarded folder prompts or silently gets nothing; whether `stat` inside one prompts; whether picking a folder in the UI's open panel grants the host anything.


## Resolved by the save-set design

These were open questions when every game had one save folder. They no longer need a real install:

- **File or folder** (Suzerain, Nuclear Throne and Six Ages on macOS): an exact-name target matches either.
- **Paths the old rules dropped** (This War of Mine and ScourgeBringer on Linux, Risk of Rain and Barony on Windows): paths are kept exactly, and an exact name directly in the install folder is allowed.
- **Game files next to a save** (NecroDancer's `data`): only `save_data.xml` is a target.
- **Saves split across folders** (Slay the Spire, Terraria, HighFleet): every folder is a target, and nothing is picked.
- **Local copy or Steam Cloud copy** (Terraria, Dead Cells, Isaac and ten more): both are restored, so whichever the game reads is right. Steam's own reaction is section 3.
- **One folder, several accounts** (Risk of Rain Returns): the `*` is the SteamID64, seen on a PC with three accounts (`76561198004523847_localsave.json` next to the config folder `user_44258119`). It needs an addendum `override` pinning `{STEAM_ID64}_localsave.json`; that's a catalog change, not a test.
- **Logs in save folders** (Unity's `Player.log`, Unreal's `Saved/Logs`, Godot's `logs`, Isaac's `log.txt`, Paradox's `logs/game.log`): logs are excluded from every target, so a log the running game keeps open never blocks a Load.
- **BattleTech's `C*` folders**: `C*/SGS*` stays a pattern, so what the folders mean doesn't matter.
- **Campaigns in one folder** (Crusader Kings II): a Load always restores the whole checkpoint, and campaigns aren't detected.
