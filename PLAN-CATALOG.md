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
- One game has exactly one save directory (DIR). Multiple candidate DIRs are
  allowed in the catalog; the runtime picks one, sticky, without asking the user.
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
| `Product fit` | `Keep` or `Remove`; only `Keep` rows are built |
| `Info` | markdown instructions, copied verbatim into the bundle |

- Lookup is an exact `Name` match against the pinned manifest. No match →
  build error with closest-name suggestions.
- Duplicate names (case-insensitive) → build error.
- Unknown `Product fit` values → build error.
- `Info` holds markdown and may contain commas, quotes and newlines as normal
  quoted CSV content. It is the only source of game instructions.
- The file may contain extra columns; the builder ignores any column beyond
  `Name`, `Product fit` and `Info`.
- `catalog/games.csv` is committed and is the only editing surface; the builder
  reads it directly. Git diffs, agents and scripts read the same file.

Current list: 97 `Keep`, 10 `Remove`. `Name` must match the manifest key
exactly. `Six Ages 2: Lights Going Out` has no manifest entry and needs an
addendum entry.

### 2.2 `catalog/addendum.yaml`

Entries for games the manifest does not have, keyed by the `Name` column in
`games.csv`. Same shape as generated entries (Section 3.4), so one validator
covers both.

```yaml
Void War:
  detect:
    steam: 2853590
  executables:
    windows: ["Void War.exe"]
  save:
    - when: { os: windows }
      dir: "{APPDATA}/Void_War"
```

- An addendum key with no matching `Keep` row is a warning and is ignored.
- The addendum **cannot introduce games**; `games.csv` remains the only game list.
- Manifest precedence: when the name exists in the manifest, manifest data wins
  per field; the addendum fills only fields the manifest did not produce
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
   directory, the game is a build error.
3. Not found → take the addendum entry (which must exist, or error).
4. Apply the addendum overlay when both exist (3.2).
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
| `files` | `save` candidates |

`files` filtering and normalization:

1. Keep entries tagged `save`; keep untagged entries; drop `config`-only entries.
2. Drop entries whose only applicable conditions are `store: microsoft`
   (MS Store saves are out of scope, Section 8).
3. Deduplicate equivalent paths:
   - `<base>/X` ≡ `<root>/steamapps/common/<installDir>/X`
   - trailing slashes are insignificant
   - globs collapse to their static directory: cut at the first `*`, `?` or `[`,
     then drop the trailing partial path segment
     (`<base>/save/user_*.dat` → `<base>/save`)
4. File-looking paths resolve to their parent directory: if the final segment
   contains a dot and the path points to a file-like leaf, use the parent.
5. `<storeUserId>` is rewritten to `{STORE_USER_ID}` wherever it appears
   (resolved at runtime to the detected store's user ID, Steam first). This
   covers per-user save folders such as
   `<home>/Saved Games/Jagged Alliance 3/<storeUserId>/*.sav`. Steam userdata
   remains special: `<root>/userdata/<appId>/...` becomes
   `{STEAM_USERDATA}/<appId>/...` (Section 4.4).
6. Drop candidates that still carry unresolved placeholders or launcher-managed
   storage, with a warning:
   - unknown `<root>/...` paths (examples from the current list:
     `<root>/savegames/<storeUserId>/3353` in Watch Dogs: Legion,
     `<root>/steamapps/compatdata/<appId>/remote/pfx` in Road 96);
   - `<base>` occurring anywhere except as the leading path root (example:
     NEO Scavenger's Flash shared-object path);
   - GOG Galaxy-managed storage
     (`.../GOG.com/Galaxy/Applications/.../Storage/...`, example: BattleTech).
   A game whose candidates all drop out fails as "no usable save directory".
   If a `Keep` game depends on one of these forms, the rule is revisited
   deliberately rather than guessed.
7. Conditions are preserved per candidate: `when.os` (`mac` → `macos`) and
   `when.store`; `bit` is dropped. No `when` means any OS/store.

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
These are probed only for games with no detected store install (Section 4.1).

Ignored manifest data: `cloud`, `registry` save keys, `notes`, `alias`, launch
`arguments`/`workingDir`, `bit`, `id.*` except `steamExtra`/`gogExtra`.

### 3.2 Addendum merge (base + manifest overlay)

When a name exists in both:

- `detect`: merged per store key; manifest value wins, addendum fills missing keys.
- `executables`: per OS; manifest wins when it produced a non-empty list for that OS.
- `save`: per OS; manifest candidates replace addendum candidates for every OS the
  manifest covers; addendum keeps only OSes the manifest produced nothing for.
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
        { "when": { "os": "windows" }, "dir": "{INSTALL_DIR}/Saves" },
        { "when": { "os": "windows" }, "dir": "{INSTALL_DIR}/SavesSkirmish" },
        { "when": { "os": "windows" }, "dir": "{INSTALL_DIR}/Ships" }
      ]
    }
  ]
}
```

Rules:

- Only present keys are emitted; empty lists/maps are omitted.
- `save` entries are candidates, not priorities. No `primary`, `legacy` or
  `version` flags exist.
- `schema` increments only on incompatible changes; a host refuses bundles with
  an unsupported major schema and keeps its current catalog.

### 3.5 Build report and failures

Hard failures (build stops): unresolved `Keep` name; `Keep` game with no usable
save candidate, reported with its reason category (name unresolved, no `files`
section, config-only, userdata-only, MS-Store-only, unsupported path form);
duplicate names; invalid `Product fit`; addendum schema errors; manifest hash
mismatch.

Warnings: addendum shadowed by the manifest;
addendum with no `Keep` row; multi-target games with no name/ancestor signal
(listed for future review, not failures).

### 3.6 Determinism and CI

- Sorted games and candidates; no timestamps in the bundle (source revision only).
- CI runs the generator and fails on a dirty `git diff`.
- `cargo xtask catalog` regenerates the bundle and prints the build report.
  `--check` regenerates in memory and fails if the committed bundle differs
  (what CI runs). `--strict` also fails on warnings, for cleanup passes.
- Regenerating from the same lock + inputs must be byte-identical.

### 3.7 Worked example: HighFleet (manifest → bundle)

Manifest has six `files` entries: `Config.ini` (config, dropped), `Saves`,
`SavesSkirmish`, `Ships` under `<base>` with `when: os: windows`, and the same
four under `<root>/steamapps/common/HighFleet/` with `when: store: steam`.
Step 3 deduplicates the two spellings; `Saves`/`SavesSkirmish`/`Ships` remain as
three windows candidates. `launch` gives `Highfleet.exe`. Result is the bundle
entry in Section 3.4.

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
- **GOG**: gog id → Galaxy registry install path.
- **Epic**: no product id in the manifest; probe loose candidates
  (`{PROGRAMFILES}/Epic Games/<dir>`, …) and accept the first with an existing
  executable.
- **Standalone**: loose candidates plus Windows uninstall-registry keys where an
  addendum supplies them; executable must exist.
- **MS Store / Game Pass**: not supported in v1.

Cost control: enumerate Steam app manifests and GOG registry once per scan and
look up catalog entries by id; probe loose directories only for games with no
store install found.

### 4.2 Installs as separate games

Each detected install becomes its own game record:

- Key: store + install directory identity (volume/file id, not the path string),
  so a moved or reinstalled directory stays the same install and history.
- Two installs of the same game → two records, each with its own DIR,
  checkpoints and history. The host gives them an install tag so they can be
  told apart ("Dead Cells — Steam", "Dead Cells — GOG"; see PLAN-HOST.md).
- If two installs resolve to the **same** save DIR, they merge into one record;
  a directory carries one checkpoint history.
- Install-level catalog ids: the first install for a catalog id keeps
  `steam-<id>`; additional installs get `steam-<id>#<install-identity>`.
- The monitor maps each install's executable paths to its record, so Save/Load
  hotkeys always target the running install. Which game hotkeys target
  otherwise is the host's rule (PLAN-HOST.md).
- No discovery error and no prompt is ever produced because a game has several
  installs.

### 4.3 Candidate filtering and resolution

For one install, with platform = the game build being run (not the runtime OS),
store = discovery source. Why: `when.os` in the manifest describes the build a
path belongs to. A Windows build running through Proton on Linux writes exactly
where it would on Windows, so it has platform `windows`; with the runtime OS it
would match the native Linux rows instead.

1. Decide the possible builds (below).
2. For each possible build, keep candidates where `when.os` is absent or equals
   that build, and `when.store` is absent or equals the store.
3. Resolve placeholders to concrete paths for that build (4.4).
4. Apply the sticky ladder (4.5) once, over the candidates of all possible
   builds together.

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
  possible**: the ladder sees both candidate sets and existing saves decide.
  Why: guessing picks the wrong folder for roughly one in ten affected games.
- The Proton prefix is always `<library>/steamapps/compatdata/<appid>/pfx`,
  computed whether or not it exists. Its existence is never evidence of a
  build. Why: it doesn't exist on a fresh install before the first launch, and
  it stays behind after the user switches back to the native build.
- An install with two possible builds is still **one install and one game
  record**, never two (4.2 splits records per install, not per build). Its
  executables are both builds' executables, so the monitor recognizes whichever
  one runs.

If filtering leaves no candidate, the game has no save dir for that install and
is reported as unsupported (no operations).

### 4.4 Placeholders and Proton

Template placeholders (bundle → resolved), by the build being run. `—` means
the placeholder does not resolve for that build and the candidate is dropped.

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
| `{STORE_USER_ID}` | current Steam id | same | same | same |
| `{STEAM_USERDATA}` | `<root>/userdata/<current steam user>` | same | same | same |

Proton (Steam, Linux, Windows build): the game is the Windows build, so
`os: windows` candidates apply when the install is a Proton install (compatdata
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

`{STEAM_USERDATA}` resolves through the most recently logged-in entry in
`loginusers.vdf`, falling back to the only `userdata/<id>` directory.
`{STORE_USER_ID}` resolves to the same Steam id when the detected store is
Steam; when the store's user id cannot be resolved, that candidate is dropped
with a warning.

### 4.5 Sticky resolution ladder

Run on first detection, then keep the result while the chosen directory exists:

1. Drop candidates that do not exist. Exactly one left → pick it.
2. Several exist → pick the one with the newest **save activity**: newest
   modification among files inside the candidate tree, restricted to files
   matching the original manifest save globs when those were captured, otherwise
   all files. Traversal is bounded.
3. Tie-break by exact save-directory name (`save`, `saves`, `savegame`,
   `savegames`, `saved games`, `save data`).
4. Fresh install, nothing exists → exact-name rule first, then first candidate.
   This pick is **provisional**: every scan runs the ladder again until some
   candidate exists, and only then does the pick stick. Why: with no saves yet
   the choice is a guess (native or Proton folder, old or new layout); nothing
   is lost by revising it, because there can be no checkpoints before the
   first save.
5. If one candidate is an ancestor of the others and sits at least one named
   segment below a known-folder/install root (never a bare `%APPDATA%`, install
   dir or volume root), it may stand in for the whole set.
6. Ambiguity never blocks the game. The chosen DIR is persisted as the game's
   `data_dir`; checkpoints are bound to it. Later scans do not re-rank while it
   exists and is still one of the install's candidates; otherwise the ladder
   runs again. Configure lets the user override at any time.
7. **A build switch counts as the pick disappearing.** When the files show the
   install changed build (native → Proton or back), candidates of the old build
   are no longer the install's candidates, so a pick among them is dropped even
   though its folder still exists, and the ladder runs again. Why: the old
   folder usually survives the switch, and keeping it would back up and restore
   a folder the game no longer uses. Old checkpoints stay bound to the old
   folder and become usable again if the user switches back (PLAN-HOST.md,
   vanished DIR).

## 5. Cases

1. **Single candidate** (Hades, one Steam install): one candidate exists →
   chosen. No prompt.
2. **Multi-target with an obvious name** (HighFleet): `Saves`, `SavesSkirmish`,
   `Ships` exist → exact-name tie-break picks `Saves`.
3. **Multi-target without a signal** (Battle vs. Chess: `profiles`,
   `live_profiles`): newest activity decides; sticky keeps it stable; Configure
   is the correction path.
4. **Old/new paths, both exist** (RimWorld, Subnautica): the old directory is
   frozen, newest activity picks the new one; sticky prevents churn.
5. **Old/new, only one exists**: existence picks it. Common case.
6. **Store-scoped dirs** (Returnal): Steam install → Steam candidate only;
   Epic install → Epic candidate only.
7. **Two installs, distinct dirs** (Dead Cells Steam + GOG): two game records,
   each with its own `{INSTALL_DIR}/save`; hotkeys follow the running install.
8. **Two installs, shared dir** (`{APPDATA}/Game` both): one record; both exe
   sets map to it.
9. **Same store twice** (two Steam libraries): install identity splits records;
   the first keeps `steam-<id>`, the second `steam-<id>#<identity>`.
10. **Linux native** (Caves of Qud, only `CoQ.x86_64` present): Linux build;
    linux candidates and XDG placeholders
    (`{XDG_CONFIG_HOME}/unity3d/Freehold Games/CavesOfQud/Saves`).
11. **Linux + Proton** (Caves of Qud, only `CoQ.exe` present): Windows build;
    windows candidates translated into the compatdata prefix, `{HOME}` included
    (`<pfx>/drive_c/users/steamuser/AppData/LocalLow/Freehold Games/CavesOfQud/Saves`);
    same bundle entry as Windows.
12. **Proton before the first launch** (only `CoQ.exe`, no prefix yet): still
    the Windows build; the prefix path is computed anyway; the pick is
    provisional until a folder exists.
13. **Stale prefix** (only `CoQ.x86_64` present, a prefix left from an earlier
    Proton run): Linux build; the prefix is ignored.
14. **Files can't tell the build** (Europa Universalis IV has no executables;
    Vampire Survivors lists the same `.exe` for both; both builds' files
    present): both candidate sets go to one ladder; the folder with saves wins;
    one game record whose executables cover both builds.
15. **Build switch, both folders exist** (played natively, then forced Proton):
    the files now show the Windows build; the Linux pick is dropped although
    its folder exists; the ladder picks the prefix folder. Old checkpoints stay
    bound to the Linux folder.
16. **Switching back** (Proton → native again): the Linux folder is picked
    again and its old checkpoints are usable, with the same IDs and history.
17. **Both folders hold the same saves** (Steam Cloud synced them across
    builds, files can't tell the build): newest activity picks; sticky
    prevents flip-flopping on later scans.
18. **Fresh install, no save dir yet**: name rule / first candidate,
    provisional; each scan re-runs the ladder until a candidate exists, then the
    pick sticks.
19. **Saves only in Steam userdata** (Risk of Rain 2): `{STEAM_USERDATA}`
    candidate resolves and participates in the ladder normally.
20. **Upstream rename**: store-id identity keeps the same `id`, so history and
    overrides survive.
21. **Addendum game lands upstream** (Void War): manifest data wins, warning is
    emitted, `games.csv` `Info` still applied.

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
  touches user overrides, configured dirs of games with history, or checkpoints.

## 7. Architecture and testability

The feature is two independently testable units plus thin host wiring.

### 7.1 Builder — `crates/catalog-build`

Offline, deterministic, no host or OS dependencies. Inputs are files (`games.csv`,
addendum, manifest path); outputs are the bundle and the report. Library plus a
`catalog-gen` binary. It depends on `crates/catalog` for the bundle model and
validator, so builder output and resolver input cannot drift.

Tests (no network, golden files under `tests/fixtures/catalog/`):

- translation rules: tags, untagged inclusion, glob collapsing, file-vs-dir,
  `<base>`/`<root>` dedupe, placeholder mapping, launch → executables;
- addendum overlay precedence, gap filling and shadow warnings;
- identity derivation and name matching/normalization;
- every hard failure and warning in Section 3.5;
- byte-identical regeneration from the same lock and inputs.

### 7.2 Resolver — `crates/catalog`

The decision module the host calls. Owns the bundle model, parsing and
validation, placeholder and Proton translation, candidate filtering, the sticky
ladder, and install-level assignment (separate records, merge on identical DIR).

Pure by construction: every environment observation goes through one trait, so
tests never touch a real filesystem.

```rust
pub trait Probe {
    fn is_dir(&self, path: &Path) -> bool;
    fn is_file(&self, path: &Path) -> bool;   // which build's executables exist
    fn install_identity(&self, install_dir: &Path) -> Option<String>;
    /// Newest modification among files matching `globs`, or all files when empty.
    fn newest_activity(&self, dir: &Path, globs: &[String]) -> Option<u64>;
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

pub struct Environment {
    pub steam_userdata: Option<PathBuf>, // resolved current Steam user
    pub current_pick: Option<PathBuf>,   // sticky value persisted by the host
}

pub enum Decision {
    Chosen { game_id: Id, data_dir: PathBuf, candidates: Vec<PathBuf>, reason: Reason },
    Unsupported { game_id: Id, reason: String },
}

pub fn resolve(
    game: &Game,
    install: &Install,
    environment: &Environment,
    probe: &dyn Probe,
) -> Decision;

/// Merges decisions whose chosen DIR is identical and assigns install-level ids.
pub fn assign_games(decisions: &[Decision]) -> Vec<GameRecord>;
```

- `game_id` is install-level (`steam-588650`, `steam-588650#<identity>`,
  `gog-...`). `candidates` exists for diagnostics (logs and the CLI); it is
  never a blocking prompt.
- Stickiness is explicit: the host passes the persisted `current_pick`;
  `resolve` keeps it while it exists and is still a candidate of the install's
  possible builds, and re-runs the ladder otherwise.
- The build decision (4.3) lives here, not in the scanner, because it reads
  catalog data (per-OS executables). The scanner only reports where the install
  and its would-be prefix are. `Chosen` carries the executables of every
  possible build, for the monitor.

Tests: in-memory `Probe` maps, **one named test per Section 5 case**, so a
missing case is visible in the test list. Beyond the cases:

- single and multiple candidates, activity ordering, sticky retention and
  re-resolution after deletion, ancestor guard, store filtering, unsupported
  games, rename stability;
- placeholders per build: every row of the 4.4 table, including `{HOME}` inside
  a Proton prefix and Windows placeholders dropped for a Linux build;
- build decision: only Windows files, only Linux files, both, none in the
  catalog, the same file listed for both, a missing or stale prefix; Windows
  and macOS installs never consider another build;
- provisional picks: a non-existent pick is revised on the next scan and sticks
  once its folder exists;
- build switches: native → Proton → native with both folders present returns
  to the original folder;
- one record per install even with two possible builds, with both builds'
  executables.

### 7.3 Host wiring (not a third testable unit)

- `crates/scanner` — store discovery only: Steam libraries and app manifests, GOG
  registry, loose install probing, install identity. Produces `Install` records;
  contains no save-dir policy.
- `apps/host` — embeds the bundle, runs discovery, calls `resolve` per install,
  persists chosen dirs and game records, publishes state. No decision logic.

Integration tests (`tests/integration/catalog.rs`) use fixture bundles and
temporary directories, never real game libraries or the network.

## 8. Non-goals (v1)

- Multiple save directories per game / multi-DIR checkpoints. Revisit only with
  a concrete game in hand.
- MS Store / Game Pass saves (`wgs` containers are opaque and cloud-bound).
- Backing up registry-stored saves.
- Non-Steam/non-GOG launcher detection (Heroic, Lutris, Flatpak) beyond
  placeholder support.
- Authoring instructions anywhere except `games.csv`.

## 9. Decision log

| Decision | Choice |
|---|---|
| Manual inputs | `games.csv` + `addendum.yaml` only |
| Manifest precedence | manifest wins per field; addendum fills gaps; shadow warning |
| Multi-target storage | candidate list; no flags |
| Multi-target pick | sticky ladder: existence → newest activity → name → ancestor |
| New/legacy marking | none in manifest; runtime recency decides |
| Installs | one game record per install; merge on identical save DIR |
| Build on Linux | the install's executables decide; undecided → both builds' candidates, one ladder; prefix existence is never evidence |
| Fresh-install pick | provisional until a candidate exists |
| Build switch | drops the old build's pick even if its folder exists |
| Ambiguity UX | auto-pick silently; Configure as correction; never block |
| Steam userdata saves | resolved via current Steam user; included as candidates |
| Untagged file entries | included as candidates |
| Bundle format | versioned JSON, embedded + optionally downloaded |
