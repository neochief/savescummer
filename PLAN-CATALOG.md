# PLAN-CATALOG.md

Catalog (known-games library) sub-manifest: how it is authored, built, packaged,
delivered and consumed at runtime. This document is the complete behavior spec;
implementation is intentionally split from the rest of the host so nearly all of
it can be built and tested in isolation.

Status: design agreed for implementation. Supersedes `catalog/games/*.yaml` and
`crates/scanner`'s current `Definition` shape.

---

## 1. Goals and principles

- The catalog declares **facts**: who a game is, what proves it is installed, and
  where its saves may live. It never encodes detection mechanics (how to read
  Steam libraries, GOG registry, etc.) — that is code.
- Humans maintain exactly two files: `catalog/games.csv` and `catalog/addendum.yaml`. Everything
  else is generated from the pinned Ludusavi manifest by deterministic rules.
- A game's saves are a **save set**: one or more **targets**, each a root
  folder plus a filter (everything, one exact name, or a glob pattern) minus
  excludes. The runtime takes **every** applicable target, never picks one.
  Why: the old "one save directory" forced globs and file paths to be widened
  to a folder, which went wrong in both directions. Nuclear Throne on macOS
  became all of `~/Library/Application Support`; NecroDancer's
  `data/save_data.xml` became the game's asset folder; Slay the Spire's four
  save folders and Terraria's players and worlds have no safe common folder,
  so one was picked and every checkpoint was half a save.
- Only saves, never configuration or logs: entries the manifest tags only as
  config are never backed up or restored, and neither are logs or crash dumps
  inside a save folder.
- The catalog is data: shipped embedded as a fallback and updatable at runtime
  without reinstalling the app.
- The feature has exactly two independently testable units: the **builder**
  (Section 3) and the **resolver** (Section 4). The host only wires them to
  storage and IPC (Section 7).

## 2. Human inputs

### 2.1 `catalog/games.csv`

The single list of supported games plus per-game instructions. One row per
game; the first row is the header.

| Column | Meaning |
|---|---|
| `Name` | display name in the app; exact Ludusavi lookup key |
| `Product fit` | `Keep`, `Remove` or `Ignored`; only `Keep` rows are built |
| `Info` | markdown instructions, copied verbatim into the bundle |

- Lookup is an exact `Name` match against the pinned manifest. No match →
  build error with closest-name suggestions.
- Duplicate names (case-insensitive) → build error.
- Unknown `Product fit` values → build error.
- `Ignored` marks a game we want but can't build yet (e.g. no usable save
  location). It's left out like `Remove`, but every build lists it as a
  warning, so it stays visible until someone addresses it and sets it back to
  `Keep`. Why not just `Remove`: that means "not wanted", and the game would be
  forgotten.
- `Info` holds markdown and may contain commas, quotes and newlines as normal
  quoted CSV content. It is the only source of game instructions.
- The file may contain extra columns; the builder ignores any column beyond
  `Name`, `Product fit` and `Info`.
- `catalog/games.csv` is committed and is the only editing surface; the builder
  reads it directly. Git diffs, agents and scripts read the same file.

Current list: 88 `Keep`, 10 `Remove`, 9 `Ignored`. `Name` must match the
manifest key exactly. The `Ignored` games have no usable save location in the
manifest (Catacomb Kids, Vagante, UnReal World, Total War Saga: Thrones of
Britannia, Darkest Dungeon, Watch Dogs: Legion, King of Dragon Pass, 80 Days)
or no manifest entry at all (Six Ages 2: Lights Going Out); each needs an
addendum entry.

### 2.2 `catalog/addendum.yaml`

Entries for games the manifest does not have, and fixes for games it gets
wrong, keyed by the `Name` column in `games.csv`. Same shape as generated
entries (Section 3.4), so one validator covers both.

```yaml
Void War:
  detect:
    steam: 2853590
  executables:
    windows: ["Void War.exe"]
  save:
    - when: { os: windows }
      path: "{APPDATA}/Void_War"

Risk of Rain Returns:
  override:
    save:
      - when: { os: windows }
        path: "{APPDATA}/Risk_of_Rain_Returns/{STEAM_ID64}_localsave.json"
      - when: { store: steam }
        path: "{STEAM_USERDATA}/1337520/remote"
```

- **Fixes go under `override`**, and a field there replaces the manifest's
  field for that game. Why: the manifest is right for most games, but some
  need a human fix. Risk of Rain Returns' `*_localsave.json` matches every
  Steam account's save on the machine (the `*` is the SteamID64), so a Load
  would roll back the other accounts too; the override pins it to the current
  account. Every override is listed in the build report, so fixes upstream has
  since made can be removed.

- **Standalone installers are named by their uninstall keys.** A game sold
  outside the stores (its own installer, a direct download) has no store id,
  and its installer may put it in any folder under any name, so loose
  candidates can't find it. Most installers register the game in Windows'
  installed-programs list, so an addendum entry may list the key names its
  installer writes there, under `detect.uninstall` (for example
  `{5A1E-...}_is1`). Only the addendum supplies them; the manifest has none.

  ```yaml
  Indie Quest:
    detect:
      uninstall: ["{5A1E-QUEST}_is1"]
  ```

- An addendum key with no matching `Keep` row is a warning and is ignored.
- The addendum **cannot introduce games**; `games.csv` remains the only game list.
- Manifest precedence: when the name exists in the manifest, manifest data wins
  per field and the addendum fills only fields the manifest did not produce,
  except for fields under `override`, which replace the manifest's
  (Section 3.2). A shadowed addendum entry produces a warning so it can be
  deleted, but nothing breaks if it is left in place.

### 2.3 `catalog/manifest.lock`

Pinned upstream source for reproducible builds:

```yaml
repo: mtkennerly/ludusavi-manifest
revision: <commit sha>
sha256: <hash of data/manifest.yaml at that revision>
```

The generator verifies `sha256` after reading the manifest. CI and local builds
use the same revision until a maintainer bumps the lock (a normal reviewed
commit).

## 3. Builder (build pipeline)

The builder is a standalone, offline, deterministic tool (`crates/catalog-build`).
Inputs: `games.csv`, `addendum.yaml`, pinned manifest. Output:
`catalog/catalog.json` (committed, reviewable diff) plus a build report.

For each `Keep` row, in order:

1. Find the name in the manifest.
2. Found → translate the manifest entry (3.1); if it yields no usable save
   target, the game is a build error.
3. Not found → take the addendum entry (which must exist, or error).
4. Merge the addendum when both exist (3.2).
5. Derive identity (3.3), inject `Info` from `games.csv`, validate the result.

### 3.1 Translation rules (manifest → entry)

Used fields:

| Manifest | Entry |
|---|---|
| key | lookup only; bundle `name` comes from `games.csv` |
| `steam.id` + `id.steamExtra` | `detect.steam` (array if extras) |
| `gog.id` + `id.gogExtra` | `detect.gog` |
| `installDir` keys | loose-install candidates (see below) |
| `launch` | `executables` per OS |
| `files` | `save` targets and `exclude` paths |

`files` filtering and normalization:

1. Entries tagged `save` and untagged entries become `save` targets. Entries
   tagged both `config` and `save` are saves too. Why: some games keep
   progress in what they also call config (NecroDancer's `save_data.xml`, Slay
   the Spire's `preferences` folder, which holds profile progress).
2. Entries tagged only `config` are never saves. When one falls inside a save
   target (after placeholder mapping, its path starts with the target's fixed
   part), it becomes an `exclude` of the game, so a Load never resets
   settings: Dome Keeper's `options.txt` inside its save folder, Slay the
   Spire's `preferences/STSGameplaySettings`. Otherwise it's dropped. A wrong
   tag in the manifest is accepted as a risk; the addendum fixes that game.
3. Drop entries whose only applicable conditions are `store: microsoft`
   (MS Store saves are out of scope, Section 8).
4. Deduplicate equivalent paths: `<base>/X` ≡
   `<root>/steamapps/common/<installDir>/X`; trailing slashes are
   insignificant.
5. **Paths are kept exactly as the manifest gives them.** A literal path is a
   target matched by its exact last name, whatever is on disk there, file or
   folder. A glob stays a glob. The runtime splits each into root and filter
   (4.3). Why: the builder never has to guess whether `save_data.xml` or
   `com.vlambeer.nuclearthrone` is a file or a folder, and never widens a path
   to its parent. Companion files the manifest doesn't list (a `.bak` next to
   a save) are not guessed at either; a real problem gets an addendum fix.
6. `<storeUserId>` names the current Steam account, but games spell it in
   one of two forms (Section 4.4): the 64-bit SteamID (`76561198004523847`) or
   the 32-bit account id (`44258119`). Risk of Rain Returns uses both, one per
   file. The manifest doesn't say which, so each path with `<storeUserId>`
   becomes two targets, one with `{STEAM_ID64}` and one with
   `{STEAM_ACCOUNT_ID}`; at runtime the one that doesn't exist is simply
   absent. A `*` that stands for an account (Risk of Rain Returns'
   `*_localsave.json`) can't be recognized from the text; the addendum pins
   it (2.2). This
   covers per-user save folders such as
   `<home>/Saved Games/Jagged Alliance 3/<storeUserId>/*.sav`. The addendum
   may pin one form for a game. Steam userdata remains special:
   `<root>/userdata/<storeUserId>/<appId>/...` becomes
   `{STEAM_USERDATA}/<appId>/...` (Section 4.4).
7. Drop targets that still carry unresolved placeholders or launcher-managed
   storage, with a warning:
   - unknown `<root>/...` paths (examples from the current list:
     `<root>/savegames/<storeUserId>/3353` in Watch Dogs: Legion,
     `<root>/steamapps/compatdata/<appId>/remote/pfx` in Road 96);
   - `<base>` occurring anywhere except as the leading path root (example:
     NEO Scavenger's Flash shared-object path);
   - GOG Galaxy-managed storage
     (`.../GOG.com/Galaxy/Applications/.../Storage/...`, example: BattleTech);
   - a target that would take a whole bare root (`{INSTALL_DIR}`, `{HOME}`,
     `{APPDATA}`, ...) or a folder every app shares (`~/Library` and its direct
     children such as `Application Support` and `Group Containers`,
     `AppData/*`, `Documents`, `My Games`, `Saved Games`, `~/.config`,
     `~/.local/share`), or a wildcard directly inside one (`<base>/save*`,
     `<home>/Library/Application Support/*`). Why: a Load makes everything a
     target matches identical to the checkpoint, so it would roll back other programs' data. An exact name
     inside such a folder is fine: `Application Support/com.vlambeer.nuclearthrone`
     touches only that entry. This is the same rule the host applies to every
     target (PLAN-HOST.md, Save set safety).
   Every dropped broad target is a build warning, even when other targets
   remain, so a lost OS never goes unnoticed. A game whose targets all drop out
   fails as "no usable save location". If a `Keep` game depends on one of
   these forms, the rule is revisited deliberately rather than guessed.
8. Conditions are preserved per target and exclude: `when.os` (`mac` →
   `macos`) and `when.store`; `bit` is dropped. No `when` means any OS/store.

`launch` → `executables`:

1. Keys must start with `<base>/` (all manifest entries do); strip it.
2. `when.os` selects the OS bucket; absent `os` means every OS.
3. `bit` and `store` are ignored; multiple entries per OS are kept.
4. Drop entries that can't be a program, such as documents (Total War: Shogun 2
   lists `data/encyclopedia/how_to_play.html`). Why: executables decide which
   build is installed (4.3), and a file every build ships would make every
   install look like that build.
5. `.app` bundles are kept as-is (runtime resolves the process); `.sh`
   launchers are kept for install validation but never used for monitoring.

Loose installs (for stores without an ID, mainly Epic and standalone):
for each `installDir` key emit candidates such as
`{PROGRAMFILES}/Epic Games/<dir>`, `{PROGRAMFILES}/<dir>` and
`{LOCALAPPDATA}/Programs/<dir>`, validated at runtime by executable existence.
These are probed for every game that has them, even when a store install was
found (Section 4.1).

Ignored manifest data: `cloud`, `registry` save keys, `notes`, `alias`, launch
`arguments`/`workingDir`, `bit`, `id.*` except `steamExtra`/`gogExtra`.

### 3.2 Addendum merge

When a name exists in both:

- `detect`: merged per store key; manifest value wins, addendum fills missing keys.
- `executables`: per OS; manifest wins when it produced a non-empty list for that OS.
- `save` and `exclude`: per OS; manifest entries replace addendum entries for
  every OS the manifest covers; addendum keeps only OSes the manifest produced
  nothing for.
- `override`: any field under it replaces the manifest's field outright.
- `info`: always from `games.csv`.
- `id`: derived (3.3); an addendum-declared id is used only when no store id exists.

### 3.3 Identity

- `steam-<id>` when a Steam id exists; else `gog-<id>`; else the addendum's
  explicit id; else a slug of the name.
- Store-based ids are stable across upstream renames, so history, overrides and
  checkpoints survive catalog updates.
- Install-level split ids are assigned at runtime (Section 4.2), not here.

### 3.4 Output bundle

`catalog/catalog.json`:

```json
{
  "schema": 1,
  "source": { "repo": "mtkennerly/ludusavi-manifest", "revision": "<sha>" },
  "games": [
    {
      "id": "steam-1434950",
      "name": "HighFleet",
      "info": "Exit to main menu before saving...",
      "detect": { "steam": 1434950, "gog": 1589167087 },
      "executables": { "windows": ["Highfleet.exe"] },
      "save": [
        { "when": { "os": "windows" }, "path": "{INSTALL_DIR}/Saves" },
        { "when": { "os": "windows" }, "path": "{INSTALL_DIR}/SavesSkirmish" },
        { "when": { "os": "windows" }, "path": "{INSTALL_DIR}/Ships" }
      ]
    },
    {
      "id": "steam-588650",
      "name": "Dead Cells",
      "save": [
        { "path": "{INSTALL_DIR}/save/user_*.dat" },
        { "path": "{INSTALL_DIR}/save/customGameData_*.json" },
        { "when": { "store": "steam" }, "path": "{STEAM_USERDATA}/588650/remote/user_*.dat" }
      ],
      "exclude": [
        { "path": "{INSTALL_DIR}/save/dc_options.json" }
      ]
    }
  ]
}
```

Rules:

- Only present keys are emitted; empty lists/maps are omitted.
- `detect` holds store ids (`steam`, `gog`) and, from the addendum only,
  uninstall-registry key names (`uninstall`). A key name is one subkey name,
  never a path.
- `save` entries are targets, all of them used, not candidates to pick from.
  No `primary`, `legacy` or `version` flags exist.
- A `path` is a literal path or a glob (`*`, `?`, `[...]`, `**`), with
  placeholders. It's never rewritten into a folder.
- `schema` increments only on incompatible changes; a host refuses bundles with
  an unsupported major schema and keeps its current catalog.

### 3.5 Build report and failures

Hard failures (build stops): unresolved `Keep` name; `Keep` game with no usable
save target, reported with its reason category (name unresolved, no `files`
section, config-only, userdata-only, MS-Store-only, unsupported path form, broad
folder only); duplicate names; invalid `Product fit`; addendum
schema errors; manifest hash mismatch.

Warnings: addendum shadowed by the manifest; addendum with no `Keep` row;
every dropped broad target; every `Ignored` row. Listed for review, not failures: every addendum
override, every game whose save set spans more than one target, and every
save target that is a whole folder tagged both config and save. Why the last:
the manifest is written for backups, where taking too much is harmless, so it
sometimes lists a game's whole user folder (Crusader Kings II, Europa
Universalis IV and Stellaris list all of `Documents/Paradox Interactive/<game>`;
Terraria lists all of `My Games/Terraria`). For us that folder holds mods, logs
and settings too: every Save copies them, a Load rolls them back, and a log the
running game holds open refuses every Load. Each listed game gets an addendum
`override` pointing at its real save folders (`save games/`; Terraria's
`Players/` and `Worlds/`).

### 3.6 Determinism and CI

- Sorted games and candidates; no timestamps in the bundle (source revision only).
- CI runs the generator and fails on a dirty `git diff`.
- `cargo xtask catalog` regenerates the bundle and prints the build report.
  `--check` regenerates in memory and fails if the committed bundle differs
  (what CI runs). `--strict` also fails on warnings, for cleanup passes.
- Regenerating from the same lock + inputs must be byte-identical.

### 3.7 Worked example: HighFleet (manifest → bundle)

Manifest has six `files` entries: `Config.ini` (config, not inside a save
target, dropped), `Saves`, `SavesSkirmish`, `Ships` under `<base>` with
`when: os: windows`, and the same four under
`<root>/steamapps/common/HighFleet/` with `when: store: steam`. Step 4
deduplicates the two spellings; `Saves`/`SavesSkirmish`/`Ships` remain as three
windows targets, and all three are backed up and restored together. `launch`
gives `Highfleet.exe`. Result is the bundle entry in Section 3.4.

### 3.8 Worked example: Void War (addendum lifecycle)

Today: `games.csv` row + addendum entry as in Section 2.2; bundle uses the addendum.

Later: the manifest gains `Void War`. The generator uses manifest data, warns
that the addendum is shadowed, and keeps injecting `games.csv`'s `Info`. Deleting
the addendum entry changes nothing in the output.

## 4. Resolver (runtime decisions)

The resolver is a pure module (`crates/catalog`) that the host calls. It owns
every catalog decision and observes the machine only through a `Probe` trait;
it has no OS APIs, database or host types.

### 4.1 Discovery per store

Given a `Game` catalog entry, the scanner finds installs:

- **Steam**: app id → `appmanifest_<id>.acf` in every library → install dir.
  An install counts only once Steam has finished it and one of the game's
  executables exists (any platform's, since a Proton install runs the Windows
  build; a game with no listed executables is taken on the manifest alone).
  Why: Steam writes the manifest, the folder and even the executable while a
  download is still running; counting that would show the game for the whole
  download. "Finished" is the fully-installed flag (bit 4) of the manifest's
  `StateFlags`. Steam keeps that flag set while an installed game updates, so
  an update never makes a game disappear.
- **GOG**: gog id → Galaxy registry install path.
- **Epic**: no product id in the manifest, but the launcher keeps one manifest
  per installed game (`Epic/EpicGamesLauncher/Data/Manifests/*.item`, JSON
  with the install location, on Windows and macOS). Read them once per scan and
  match an install by its folder name against the game's `installDir` keys
  and an existing executable. Loose candidates
  (`{PROGRAMFILES}/Epic Games/<dir>`, …) remain the fallback when the
  launcher's manifests are unavailable.
- **Standalone**: loose candidates plus Windows uninstall-registry keys where an
  addendum supplies them (Section 2.2); executable must exist. For each key a
  game names:
  - look under the installed-programs list machine-wide in both registry views
    (64-bit and 32-bit), then the user's own;
  - take the install folder from `InstallLocation`, or from Inno Setup's
    `Inno Setup: App Path` when that is empty (Inno installers don't always
    fill it);
  - count it only when that folder holds one of the game's executables.

  Only games that name a key cost a lookup; the whole list is never walked. A
  folder a loose candidate also finds is the same install, not a second one.
- **MS Store / Game Pass**: not supported in v1.

Cost control: list each Steam library's `steamapps` folder, the GOG registry
and the Epic manifests once per scan and look up catalog entries by id or
folder name, never one file check per catalog game per library. Loose
directories are probed for **every** game that has loose candidates, even when
a store install was found. Why: skipping them hid a second copy of a game
(Steam plus a standalone or Epic copy), which 4.2 promises to show as its own
record. A loose candidate that is the same directory as a store install is that
install, not a second one. Probing is a few file checks per such game.

### 4.2 Installs as separate games

Each detected install becomes its own game record:

- Key: store + product id. Why: Steam's "Move install folder", a reinstall
  and a library on a new drive all change the directory's volume/file id, and
  each must stay the same install with the same history. The directory's
  volume/file id (not the path string) only tells apart two installs of the
  same product that exist **at the same time** (two Steam libraries). An
  install that disappears while another copy of the same product appears is a
  move, not a new game.
- Two installs of the same game → two records, each with its own save set,
  checkpoints and history. The host gives them an install tag so they can be
  told apart ("Dead Cells — Steam", "Dead Cells — GOG"; see PLAN-HOST.md).
- If two installs' save sets **share a target** (both use `{APPDATA}/Game`),
  they merge into one record whose save set is the union of both; a folder
  carries one checkpoint history. Why the union: two records restoring the same
  folder would overwrite each other's saves.
- Install-level catalog ids: the first install for a catalog id keeps
  `steam-<id>`; additional installs get `steam-<id>#<install-identity>`.
- The monitor maps each install's executable paths to its record, so Save/Load
  hotkeys always target the running install. Which game hotkeys target
  otherwise is the host's rule (PLAN-HOST.md).
- No discovery error and no prompt is ever produced because a game has several
  installs.

### 4.3 Building the save set

For one install, with platform = the game build being run (not the runtime OS),
store = discovery source. Why: `when.os` in the manifest describes the build a
path belongs to. A Windows build running through Proton on Linux writes exactly
where it would on Windows, so it has platform `windows`; with the runtime OS it
would match the native Linux rows instead.

1. Decide the possible builds (below).
2. For each possible build, keep `save` and `exclude` entries where `when.os`
   is absent or equals that build, and `when.store` is absent or equals the
   store.
3. Resolve placeholders to concrete paths for that build (4.4). A path whose
   placeholder can't resolve (Steam account unknown) is dropped with a warning.
4. Split each path into a **root** and a **filter**. The root is everything
   before the first segment with a wildcard; for a literal path, it's the
   parent folder and the filter is the exact last name. Exact names match
   whatever is on disk, file or folder.
5. Deduplicate targets by real root and filter, following the filesystem's
   case rules, junctions, symlinks and redirected known folders (`AppData` and
   `appdata` on Windows are one target; on Linux they are two). A target whose
   files another target of the same game already covers entirely (a folder
   target containing a pattern target) is dropped. Why: the same file counted
   twice would be copied twice and restored in a conflicting order.
6. Attach each exclude to the target it falls inside. The host adds its own
   built-in excludes to every target (Steam's files, logs and crash dumps; see
   PLAN-HOST.md, What a checkpoint holds), so the catalog never lists those.
7. **The save set is every remaining target of every possible build**, whether
   or not it exists yet. Nothing is ranked or picked. Why: a folder that doesn't
   exist now can appear later (the game creates it, Steam Cloud is switched on),
   and a set that changed every time would make checkpoints come and go.

**Why every target, not one pick.** Candidates are sometimes parts of one save
and sometimes alternatives, and the manifest doesn't say which. Taking all of
them is right for both:

- **Parts** (Slay the Spire's `saves`, `preferences`, `runs`; Terraria's
  players and worlds) are all backed up together, so a checkpoint is never half
  a save.
- **Alternatives** are harmless together. A local copy and a Steam `remote`
  copy of the same save are both restored, so whichever the game reads is
  right: Risk of Rain Returns writes both, Isaac only `remote`, Slay the Spire
  only its install folder, and the manifest can't tell them apart. An old,
  frozen folder from before a game update restores to exactly what it already
  holds. Native and Proton folders for a build that can't be decided are both
  covered.
- The cost is space: checkpoints are larger than with a single folder.

Every existence check reports one of three answers: **present**, **missing**
(confirmed: the nearest existing parent can be read and the entry isn't in
it), or **unknown** (an unplugged drive, an unreachable share, an access
error). The resolver reports each target's presence with the save set. Only
*missing* counts as absent. Why: a save folder on an unplugged USB library
isn't deleted, and treating it as absent would make a checkpoint that silently
skips it; the host makes operations unavailable instead.

**Which build an install runs.** Only Linux has a choice: a Steam game there
can be the native Linux build or the Windows build through Proton. Windows
installs are always the Windows build, macOS installs the macOS build (no Wine
support in v1). On Linux the resolver decides from the game's own files:

- Check which of the catalog's per-OS executables exist in the install dir.
  Only one build's executables exist → that build. Why: Steam installs one
  build's files at a time and swaps them when the user switches, so the files
  on disk say what will run. Caves of Qud with only `CoQ.exe` is the Windows
  build; with only `CoQ.x86_64` it is the Linux build.
- The files can't tell when both builds' executables exist, the catalog lists
  executables for one build or none (Europa Universalis IV, Stellaris), or both
  builds list the same file (Vampire Survivors). Then **both builds are
  possible**, and the save set holds both builds' targets. Why: guessing picks
  the wrong folder for roughly one in ten affected games.
- The Proton prefix is always `<library>/steamapps/compatdata/<appid>/pfx`,
  computed whether or not it exists. Its existence is never evidence of a
  build. Why: it doesn't exist on a fresh install before the first launch, and
  it stays behind after the user switches back to the native build.
- An install with two possible builds is still **one install and one game
  record**, never two (4.2 splits records per install, not per build). Its
  executables are both builds' executables, so the monitor recognizes whichever
  one runs.

If filtering leaves no target, the game has no save location for that install
and is reported as unsupported (no operations).

### 4.4 Placeholders and Proton

Template placeholders (bundle → resolved), by the build being run. `—` means
the placeholder does not resolve for that build and the target is dropped.

| Bundle | Windows build | Windows build via Proton | macOS build | Linux build |
|---|---|---|---|---|
| `{INSTALL_DIR}` | install dir | install dir | install dir | install dir |
| `{HOME}` | user profile | prefix profile | home | home |
| `{APPDATA}` | `%APPDATA%` | prefix | — | — |
| `{LOCALAPPDATA}` | `%LOCALAPPDATA%` | prefix | — | — |
| `{LOCALLOW}` | `%LOCALAPPDATA%Low` | prefix | — | — |
| `{DOCUMENTS}` | Documents | prefix | — | — |
| `{PUBLIC}` | `%PUBLIC%` | prefix | — | — |
| `{PROGRAMDATA}` | `%PROGRAMDATA%` | prefix | — | — |
| `{PROGRAMFILES}` | Program Files | prefix | — | — |
| `{WINDIR}` | Windows folder | prefix | — | — |
| `{XDG_DATA_HOME}` | — | — | — | XDG data home |
| `{XDG_CONFIG_HOME}` | — | — | — | XDG config home |
| `{STEAM_ACCOUNT_ID}` | current Steam account id (32-bit) | same | same | same |
| `{STEAM_ID64}` | current SteamID64 | same | same | same |
| `{STEAM_USERDATA}` | `<root>/userdata/<account id>` | same | same | same |

Proton (Steam, Linux, Windows build): the game is the Windows build, so
`os: windows` targets apply when the install is a Proton install (compatdata
prefix exists and no native build is in use). The prefix is a private Windows
drive at `<library>/steamapps/compatdata/<appid>/pfx`; every Windows location,
including the user's home, resolves inside it:

```
{HOME}         → <pfx>/drive_c/users/steamuser
{APPDATA}      → <pfx>/drive_c/users/steamuser/AppData/Roaming
{LOCALAPPDATA} → <pfx>/drive_c/users/steamuser/AppData/Local
{LOCALLOW}     → <pfx>/drive_c/users/steamuser/AppData/LocalLow
{DOCUMENTS}    → <pfx>/drive_c/users/steamuser/Documents
{PUBLIC}       → <pfx>/drive_c/users/Public
{PROGRAMDATA}  → <pfx>/drive_c/ProgramData
{PROGRAMFILES} → <pfx>/drive_c/Program Files
{WINDIR}       → <pfx>/drive_c/windows
{INSTALL_DIR}  → unchanged (the Windows install dir)
```

`{HOME}` matters as much as the AppData folders: manifest rows often spell
Windows locations from the profile (Caves of Qud:
`<home>/AppData/LocalLow/Freehold Games/CavesOfQud/Saves`). Resolving it to the
Linux home would point Proton saves at `~/AppData/...`, which never exists.

`steamuser` is the default profile name; if the prefix contains exactly one
other `drive_c/users/*` profile, that one is used instead. The bundle contains
no Proton-specific data; translation is runtime policy.

**The current Steam account.** `{STEAM_ACCOUNT_ID}`, `{STEAM_ID64}` and
`{STEAM_USERDATA}` all name one account, found in this order:

1. **`ActiveUser`**, the account logged into the running Steam client, when it
   is not 0. Windows keeps it in
   `HKCU\Software\Valve\Steam\ActiveProcess`; Linux in `~/.steam/registry.vdf`
   (to verify on macOS). Why first: it is who a game started now runs under,
   not who logged in last.
2. The entry marked `MostRecent` in `loginusers.vdf` (older Steam versions).
3. The entry with the newest `Timestamp` in `loginusers.vdf`. Why: current
   Steam versions no longer write `MostRecent`.
4. The only `userdata/<id>` directory.

Otherwise the account is unknown, and targets that need it are dropped with
a warning. Why no "only folder" shortcut earlier: `userdata` keeps a folder for
every account that ever logged in on the machine, so several are normal (a
machine with three accounts in `loginusers.vdf` had six).

Both forms come from one number: `SteamID64 = 76561197960265728 + account id`.
Steam's own folders and files use the account id (`userdata/44258119`,
`steam_autocloud.vdf`); its APIs and the web use the SteamID64, so games use
either, and some use both.

The account is read at every scan and again whenever a game starts. Why at
start: the user can switch Steam accounts between two sessions, and a game
runs under whoever is logged in when it launches. A different account at
start is a changed context (4.5).

### 4.5 The save set over time

Nothing is sticky. Given the same install, build, Steam account and catalog, the
resolver always returns the same save set, and it's asked again at every scan
and whenever a Steam game starts. Why no stickiness: it existed to keep one
pick from flip-flopping between candidates, and with every target in the set
there's no pick to keep stable.

The save set comes with its **context**: the build and the Steam account (the
account behind `{STEAM_USERDATA}`, `{STEAM_ACCOUNT_ID}` and `{STEAM_ID64}`).
The set changes when:

- **the build changes** (native → Proton or back): its targets now name the
  other build's folders. Why this matters: the old build's folder usually
  survives the switch, and backing it up would back up a folder the game no
  longer uses;
- **the Steam account changes**: its targets now name the new account's
  folders. Why: the old account's folder is still there, and a Load would
  restore into someone else's saves;
- **a catalog update edits the game's paths.**

Old checkpoints are never moved or rewritten. The host restores a checkpoint
only into targets it has in common with the current save set, so after a build
or account switch old checkpoints are unavailable, and they become usable again
when the context returns (PLAN-HOST.md, Checkpoints belong to their targets).

A user override (Configure) replaces the whole save set with one location. The
resolver isn't consulted for it, and it never changes on its own.

## 5. Cases

1. **Single target** (Hades, one Steam install): the save set is that one
   folder. No prompt.
2. **Several folders, one save** (Slay the Spire: `saves`, `preferences`,
   `runs`, `betaPreferences` in the install folder, no safe common parent):
   four targets; every checkpoint holds all four. Formerly one was picked by
   activity, and after a finished run that could stick to `runs` forever.
3. **Several folders of different kinds** (HighFleet: `Saves`,
   `SavesSkirmish`, `Ships`): all three are backed up and restored together.
4. **Old and new paths, both exist** (RimWorld, Subnautica): both are targets;
   the frozen old folder restores to exactly what it already holds.
5. **Old and new, only one exists**: the other is absent; a Load leaves an
   absent target alone.
6. **Store-scoped paths** (Returnal): Steam install → Steam targets only;
   Epic install → Epic targets only.
7. **Two installs, distinct folders** (Dead Cells Steam + GOG): two game
   records, each with its own `{INSTALL_DIR}/save`; hotkeys follow the running
   install.
8. **Two installs, shared folder** (`{APPDATA}/Game` both): one record whose
   save set is the union; both executable sets map to it.
9. **Same store twice** (two Steam libraries): install identity splits records;
   the first keeps `steam-<id>`, the second `steam-<id>#<identity>`.
10. **Linux native** (Caves of Qud, only `CoQ.x86_64` present): Linux build;
    linux targets and XDG placeholders
    (`{XDG_CONFIG_HOME}/unity3d/Freehold Games/CavesOfQud/Saves`).
11. **Linux + Proton** (Caves of Qud, only `CoQ.exe` present): Windows build;
    windows targets translated into the compatdata prefix, `{HOME}` included
    (`<pfx>/drive_c/users/steamuser/AppData/LocalLow/Freehold Games/CavesOfQud/Saves`);
    same bundle entry as Windows.
12. **Proton before the first launch** (only `CoQ.exe`, no prefix yet): still
    the Windows build; the prefix paths are computed anyway; the targets are
    absent until the game writes them, and Save is unavailable until then.
13. **Stale prefix** (only `CoQ.x86_64` present, a prefix left from an earlier
    Proton run): Linux build; the prefix is ignored.
14. **Files can't tell the build** (Europa Universalis IV has no executables;
    Vampire Survivors lists the same `.exe` for both; both builds' files
    present): the save set holds both builds' targets; one game record whose
    executables cover both builds.
15. **Build switch, both folders exist** (played natively, then forced Proton):
    the files now show the Windows build; the save set becomes the prefix
    targets although the Linux folder exists. Old checkpoints have no target in
    common and are unavailable.
16. **Switching back** (Proton → native again): the Linux targets return and
    their old checkpoints are usable, with the same IDs and history.
17. **A local copy and a Steam Cloud copy** (Risk of Rain Returns writes both
    `..._localsave.json` and `remote/save.json`; Dead Cells lists
    `{INSTALL_DIR}/save` and `remote`): both are targets and both are restored,
    so whichever the game reads is right.
18. **Fresh install, no saves yet**: every target is absent; Save is
    unavailable until one appears. Nothing is provisional, because nothing is
    picked.
19. **Saves only in Steam userdata** (Risk of Rain 2): the `{STEAM_USERDATA}`
    target resolves like any other.
20. **Upstream rename**: store-id identity keeps the same `id`, so history and
    overrides survive.
21. **Addendum game lands upstream** (Void War): manifest data wins, warning is
    emitted, `games.csv` `Info` still applied.
22. **Catalog update changes a path** (an update rewrites, adds or removes a
    target): the save set follows the new catalog. Old checkpoints restore the
    targets they have in common with it; new targets are left alone by them.
23. **Save folder on an unplugged drive** (`{INSTALL_DIR}/save` on a USB
    library that is disconnected): the target is unknown, not missing; the host
    makes operations unavailable. When the drive returns, everything works as
    before.
24. **User deleted their saves to start over** (RimWorld: the new `Saves`
    folder deleted): the target is missing. Save works again once the game
    recreates it; until then a Load of a checkpoint with data there is refused,
    because roots are never recreated.
25. **Game update moved its saves** (the game now writes to another path the
    catalog lists): both paths are targets; the new one simply starts having
    files.
26. **Steam account switch** (the Steam account placeholders now name another
    account; detected at a scan or when the game starts): the save set changes
    to the new account's targets; switching back makes the old checkpoints
    usable again. Targets that don't depend on the account stay in common.
27. **Steam copy plus a standalone or Epic copy**: both are found, the second
    through loose candidates or Epic's launcher manifests; two records, as in
    case 7.
28. **Install moved to another library** (Steam "Move install folder", new
    drive, new volume/file id): same store and product id with only one copy
    present, so it's the same install, record and history.
29. **User override** (a save location set in Configure): replaces the save
    set; the resolver isn't consulted for it, even when the location is
    missing.
30. **No applicable target** (a native Linux install whose catalog entry only
    has Windows rows): Unsupported; the game stays visible so the user can set
    a save location in Configure.
31. **Steam account unknown** (Steam never logged in, so the Steam account
    placeholders can't resolve): those targets are dropped with a warning; the
    rest of the save set stays.
32. **Custom Proton profile** (the prefix has exactly one other
    `drive_c/users/*` folder besides `steamuser`): that profile is used for
    every Windows placeholder.
33. **One folder spelled two ways** (`AppData` and `appdata`, or Documents
    reached through a redirected or junctioned path on Windows): one target,
    not two; on Linux, case-different folders stay distinct.
34. **A folder with a dot in a shared place** (Nuclear Throne on macOS:
    `Application Support/com.vlambeer.nuclearthrone`): root `Application
    Support`, filter the exact name; only that entry is ever touched.
35. **A save file among game files** (NecroDancer:
    `{INSTALL_DIR}/data/save_data.xml`): only that file is a target; the game's
    assets next to it are never copied or restored.
36. **Patterns** (Dead Cells `save/user_*.dat`, BattleTech `C*/SGS*`): only
    matching entries are backed up; logs and settings next to them aren't.
37. **Settings inside a save folder** (Dome Keeper `options.txt`): excluded;
    a Load never resets them.
38. **An account-standing wildcard** (Risk of Rain Returns
    `*_localsave.json`): the addendum override pins it to `{STEAM_ID64}`, so a
    Load never touches other accounts' saves.
39. **Two spellings of the Steam id** (`<storeUserId>` in a path): two
    targets, one per form; the one that doesn't exist is simply absent.
40. **A target covered by another** (a folder target and a pattern inside it):
    the pattern is dropped; the folder covers it.
41. **A standalone installer's own folder** (installed to
    `D:\Games\IQ v1.2`, a name no loose candidate has): found through the
    uninstall key the addendum names; gone again when the key and folder
    are.
42. **A Steam download in progress** (manifest, folder and executable already
    there, `StateFlags` without the fully-installed bit): not installed yet;
    the game appears when Steam finishes. An installed game that is updating
    keeps the bit and stays.

## 6. Bundle delivery and updates

- The host embeds `catalog/catalog.json` at build time as the fallback.
- A bundle may be fetched at runtime (release asset or raw URL) with ETag /
  SHA-256 verification, size cap, schema validation (same parser as build-time),
  atomic replacement in `%LOCALAPPDATA%\SaveScummer\catalog`, and last-good
  retention. Any failure falls back silently to the previous/embedded bundle.
- A changed bundle revision triggers a rescan. `--no-catalog-update` disables
  fetching for tests and isolated runs; a CLI command reports the active bundle
  revision and refreshes it on demand.
- Catalog data only: a downloaded bundle can add or adjust games, but it never
  touches user overrides or checkpoints. A game whose paths changed gets a new
  save set, and old checkpoints restore the targets they have in common with
  it (4.5).

## 7. Architecture and testability

The feature is two independently testable units plus thin host wiring.

### 7.1 Builder — `crates/catalog-build`

Offline, deterministic, no host or OS dependencies. Inputs are files (`games.csv`,
addendum, manifest path); outputs are the bundle and the report. Library plus a
`catalog-gen` binary. It depends on `crates/catalog` for the bundle model and
validator, so builder output and resolver input cannot drift.

Tests (no network, golden files under `tests/fixtures/catalog/`):

- translation rules: tags (save, untagged, config-and-save, config-only),
  config-only entries inside a save target becoming excludes, paths kept
  exactly (literal names with dots, globs), `<base>`/`<root>` dedupe,
  placeholder mapping, both `<storeUserId>` forms, broad targets dropped with a
  warning while exact names inside broad folders are kept, launch →
  executables;
- addendum merge precedence, gap filling, shadow warnings, and `override`
  replacing a manifest field;
- identity derivation and name matching/normalization;
- every hard failure and warning in Section 3.5;
- byte-identical regeneration from the same lock and inputs.

### 7.2 Resolver — `crates/catalog`

The decision module the host calls. Owns the bundle model, parsing and
validation, placeholder and Proton translation, the Steam account, building the
save set, and install-level assignment (separate records, merge when save sets
share a target).

Pure by construction: every environment observation goes through one trait, so
tests never touch a real filesystem.

```rust
pub trait Probe {
    fn presence(&self, path: &Path) -> Presence; // present | missing | unknown
    fn is_file(&self, path: &Path) -> bool;      // which build's executables exist
    fn same_dir(&self, a: &Path, b: &Path) -> bool; // real-directory equality
    fn install_identity(&self, install_dir: &Path) -> Option<String>;
    fn steam_account(&self) -> Option<SteamAccount>; // 4.4 order, both id forms
}

pub struct Install {                    // produced by the scanner, passed in
    pub catalog_id: Id,
    pub store: Store,                   // steam | gog | epic | standalone
    pub os: Platform,                   // the machine: windows | macos | linux
    pub install_dir: PathBuf,
    /// Where a Proton prefix for this install lives (Steam on Linux), whether
    /// or not it exists yet. The resolver decides the build, not the scanner.
    pub proton_prefix: Option<PathBuf>,
}

pub struct Target {
    pub root: PathBuf,                  // real folder, never a glob
    pub filter: Filter,                 // everything | exact name | pattern
    pub excludes: Vec<Filter>,
    pub presence: Presence,
}

pub enum Decision {
    Resolved { game_id: Id, save_set: Vec<Target>, context: Context },
    Unsupported { game_id: Id, reason: String },
}

pub fn resolve(game: &Game, install: &Install, probe: &dyn Probe) -> Decision;

/// Merges decisions whose save sets share a target and assigns install-level ids.
pub fn assign_games(decisions: &[Decision]) -> Vec<GameRecord>;
```

- `game_id` is install-level (`steam-588650`, `steam-588650#<identity>`,
  `gog-...`).
- Nothing is persisted between calls: the same inputs always give the same
  save set. `context` (build, Steam account) is for the host's checkpoint
  records and diagnostics.
- The build decision (4.3) lives here, not in the scanner, because it reads
  catalog data (per-OS executables). The scanner only reports where the install
  and its would-be prefix are. `Resolved` carries the executables of every
  possible build, for the monitor.
- The resolver doesn't apply the host's safety rules for overrides and custom
  games; it only drops broad catalog targets (3.1 rule 7) as a second line of
  defence.

Tests: in-memory `Probe` maps, **one named test per Section 5 case**, so a
missing case is visible in the test list. Beyond the cases:

- one and several targets, root and filter splitting for literal paths and
  globs, deduplication, a target covered by another, excludes attached to the
  right target, store filtering, unsupported games, rename stability;
- placeholders per build: every row of the 4.4 table, including `{HOME}` inside
  a Proton prefix and Windows placeholders dropped for a Linux build;
- build decision: only Windows files, only Linux files, both, none in the
  catalog, the same file listed for both, a missing or stale prefix; Windows
  and macOS installs never consider another build;
- build switches: native → Proton → native changes the save set and back;
- one record per install even with two possible builds, with both builds'
  targets and executables;
- presence: present, missing and unknown reported per target, a folder or a
  file behind an exact name;
- Steam account: each step of the 4.4 order wins over the ones below it
  (`ActiveUser` over a newer `loginusers.vdf` entry, `MostRecent` over a newer
  `Timestamp`); `ActiveUser` of 0 falls through; several `userdata` folders
  with no other source leave the account unknown; both id forms come from one
  account; a `<storeUserId>` path resolves through whichever form exists;
- determinism: the same inputs give the same save set in the same order;
- discovery inputs: an Epic launcher manifest matched by folder name, loose
  probing alongside a store install, a moved install keeping its record, two
  simultaneous copies splitting by directory identity, two installs sharing a
  target merging.

### 7.3 Host wiring (not a third testable unit)

- `crates/scanner` — store discovery only: Steam libraries and app manifests, GOG
  registry, uninstall-registry keys, loose install probing, install identity. Produces `Install` records;
  contains no save policy.
- `apps/host` — embeds the bundle, runs discovery, calls `resolve` per install,
  stores game records with their save sets, publishes state. No decision logic.

Integration tests (`tests/integration/catalog.rs`) use fixture bundles and
temporary directories, never real game libraries or the network.

## 8. Non-goals (v1)

- Restoring part of a checkpoint, or detecting a game's profiles or campaigns.
  A Load always restores the whole save set. Why: which files belong to which
  campaign can't be detected reliably, and a campaign can span targets.
- Guessing companion files the manifest doesn't list (`.bak`, checksums).
- MS Store / Game Pass saves (`wgs` containers are opaque and cloud-bound).
- Backing up registry-stored saves.
- Non-Steam/non-GOG launcher detection (Heroic, Lutris, Flatpak) beyond
  placeholder support.
- Authoring instructions anywhere except `games.csv`.

## 9. Decision log

| Decision | Choice |
|---|---|
| Manual inputs | `games.csv` + `addendum.yaml` only |
| Manifest precedence | manifest wins per field; addendum fills gaps; `override` replaces; shadow warning |
| Unit of backup | save set of targets (root + filter + excludes); never one picked folder |
| Candidates | every applicable target, existing or not; nothing ranked, nothing sticky |
| Manifest paths | kept exactly; literal = exact name, glob = pattern; no widening to a parent |
| Config entries | never backed up; config-only inside a save target → exclude |
| Broad folders | a whole broad folder or a wildcard in one is dropped; an exact name inside one is fine |
| Steam id forms | `{STEAM_ACCOUNT_ID}` and `{STEAM_ID64}`; `<storeUserId>` → both, the existing one counts |
| Steam account | `ActiveUser` → `MostRecent` → newest `Timestamp` → only `userdata` folder; read at scans and at game start |
| Account wildcards | pinned per game in the addendum `override` |
| Installs | one game record per install; merge when save sets share a target |
| Build on Linux | the install's executables decide; undecided → both builds' targets; prefix existence is never evidence |
| Build or account switch | the save set changes; old checkpoints unavailable until the context returns |
| Catalog update | the save set follows it; checkpoints restore the targets in common |
| Unreadable location | unknown, not missing; the host makes operations unavailable |
| Install identity | store + product id; directory identity only separates simultaneous copies |
| Loose installs | probed for every game with loose candidates; Epic from launcher manifests |
| Ambiguity UX | never a prompt; Configure as correction; never block |
| Steam userdata saves | resolved via the current Steam account; ordinary targets |
| Untagged file entries | save targets |
| Bundle format | versioned JSON, embedded + optionally downloaded |
