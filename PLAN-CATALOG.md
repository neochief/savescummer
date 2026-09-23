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
- Humans maintain exactly two files: `catalog/games.xlsx` and `catalog/addendum.yaml`. Everything
  else is generated from the pinned Ludusavi manifest by deterministic rules.
- One game has exactly one save directory (DIR). Multiple candidate DIRs are
  allowed in the catalog; the runtime picks one, sticky, without asking the user.
- The catalog is data: shipped embedded as a fallback and updatable at runtime
  without reinstalling the app.
- The feature has exactly two independently testable units: the **builder**
  (Section 3) and the **resolver** (Section 4). The host only wires them to
  storage and IPC (Section 7).

## 2. Human inputs

### 2.1 `catalog/games.xlsx`

The single list of supported games plus per-game instructions. Sheet
**Candidates**; first row is the header.

| Column | Read by builder | Meaning |
|---|---|---|
| `Name` | yes | display name in the app; default lookup key |
| `Manifest name` (optional) | yes | exact Ludusavi key when it differs from `Name` |
| `Product fit` | yes | `Keep` or `Remove`; only `Keep` rows are built |
| `Info` | yes | markdown instructions, copied verbatim into the bundle |
| `PCGamingWiki`, `Category`, `Actionable feedback` | no | human review metadata |
| Sheet `Product Fit Review` | no | review summary; other sheets are ignored |

- Lookup order: `Manifest name` when present, then exact `Name`, then
  case-insensitive `Name` (warning). No match anywhere → build error with
  closest-name suggestions.
- Duplicate names (case-insensitive) → build error.
- Unknown `Product fit` values → build error.
- `Info` holds markdown and may contain commas, quotes and newlines as normal
  cell content. It is the only source of game instructions.
- The builder consumes the generated, committed `catalog/games.csv`; the export
  from `games.xlsx` is produced by the builder toolchain and CI fails when the
  CSV is stale. Git diffs, agents and scripts read the CSV; the spreadsheet
  stays the only editing surface.

Current list: 96 `Keep`, 10 `Remove`. Known name mismatches that need the
`Manifest name` column (or a rename): `ADOM (Ancient Domains Of Mystery)` →
`ADOM: Ancient Domains of Mystery`; `Total War: ROME II - Emperor Edition` →
`Total War: Rome II`; `A Total War Saga: THRONES OF BRITANNIA` →
`Total War Saga: Thrones of Britannia`. `Six Ages 2: Lights Going Out` has no
manifest entry and needs an addendum entry.

### 2.2 `catalog/addendum.yaml`

Entries for games the manifest does not have, keyed by the sheet `Name`. Same
shape as generated entries (Section 3.4), so one validator covers both.

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
- The addendum **cannot introduce games**; the sheet remains the only game list.
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
Inputs: `games.xlsx`, `addendum.yaml`, pinned manifest. Output:
`catalog/catalog-v1.json` (committed, reviewable diff) plus a build report.

For each `Keep` row, in order:

1. Find the name in the manifest.
2. Found → translate the manifest entry (3.1); if it yields no usable save
   directory, the game is a build error.
3. Not found → take the addendum entry (which must exist, or error).
4. Apply the addendum overlay when both exist (3.2).
5. Derive identity (3.3), inject `Info` from the sheet, validate the result.

### 3.1 Translation rules (manifest → entry)

Used fields:

| Manifest | Entry |
|---|---|
| key | lookup only; bundle `name` comes from the sheet |
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
     (`.../GOG.com/Galaxy/Applications/.../Storage/...`, example: BATTLETECH).
   A game whose candidates all drop out fails as "no usable save directory".
   If a `Keep` game depends on one of these forms, the rule is revisited
   deliberately rather than guessed.
7. Conditions are preserved per candidate: `when.os` (`mac` → `macos`) and
   `when.store`; `bit` is dropped. No `when` means any OS/store.

`launch` → `executables`:

1. Keys must start with `<base>/` (all manifest entries do); strip it.
2. `when.os` selects the OS bucket; absent `os` means every OS.
3. `bit` and `store` are ignored; multiple entries per OS are kept.
4. `.app` bundles are kept as-is (runtime resolves the process); `.sh`
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
- `info`: always from the sheet.
- `id`: derived (3.3); an addendum-declared id is used only when no store id exists.

### 3.3 Identity

- `steam-<id>` when a Steam id exists; else `gog-<id>`; else the addendum's
  explicit id; else a slug of the name.
- Store-based ids are stable across upstream renames, so history, overrides and
  checkpoints survive catalog updates.
- Install-level split ids are assigned at runtime (Section 4.2), not here.

### 3.4 Output bundle

`catalog/catalog-v1.json`:

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

Warnings: case-insensitive name match; addendum shadowed by the manifest;
addendum with no `Keep` row; multi-target games with no name/ancestor signal
(listed for future review, not failures).

### 3.6 Determinism and CI

- Sorted games and candidates; no timestamps in the bundle (source revision only).
- CI runs the generator and fails on a dirty `git diff`.
- Regenerating from the same lock + inputs must be byte-identical.

### 3.7 Worked example: HighFleet (manifest → bundle)

Manifest has six `files` entries: `Config.ini` (config, dropped), `Saves`,
`SavesSkirmish`, `Ships` under `<base>` with `when: os: windows`, and the same
four under `<root>/steamapps/common/HighFleet/` with `when: store: steam`.
Step 3 deduplicates the two spellings; `Saves`/`SavesSkirmish`/`Ships` remain as
three windows candidates. `launch` gives `Highfleet.exe`. Result is the bundle
entry in Section 3.4.

### 3.8 Worked example: Void War (addendum lifecycle)

Today: sheet row + addendum entry as in Section 2.2; bundle uses the addendum.

Later: the manifest gains `Void War`. The generator uses manifest data, warns
that the addendum is shadowed, and keeps injecting the sheet's `Info`. Deleting
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
  checkpoints and history. Display shows the store/install label when names
  collide ("Dead Cells — Steam", "Dead Cells — GOG").
- If two installs resolve to the **same** save DIR, they merge into one record;
  a directory carries one checkpoint history.
- Install-level catalog ids: the first install for a catalog id keeps
  `steam-<id>`; additional installs get `steam-<id>#<install-identity>`.
- The monitor maps each install's executable paths to its record, so Save/Load
  hotkeys always target the running install. With nothing running, the selected
  library row decides.
- No discovery error and no prompt is ever produced because a game has several
  installs.

### 4.3 Candidate filtering and resolution

For one install, with platform = runtime OS, store = discovery source:

1. Keep candidates where `when.os` is absent or equals the platform, and
   `when.store` is absent or equals the store.
2. Resolve placeholders to concrete paths (4.4).
3. Apply the sticky ladder (4.5).

If filtering leaves no candidate, the game has no save dir for that install and
is reported as unsupported (no operations).

### 4.4 Placeholders and Proton

Template placeholders (bundle → resolved):

| Bundle | Windows | macOS | Linux |
|---|---|---|---|
| `{INSTALL_DIR}` | install dir | install dir | install dir |
| `{APPDATA}` | `%APPDATA%` | — | Proton prefix |
| `{LOCALAPPDATA}` | `%LOCALAPPDATA%` | — | Proton prefix |
| `{LOCALLOW}` | `%LOCALAPPDATA%Low` | — | Proton prefix |
| `{DOCUMENTS}` | Documents | — | Proton prefix |
| `{PUBLIC}` | `%PUBLIC%` | — | Proton prefix |
| `{PROGRAMDATA}` | `%PROGRAMDATA%` | — | Proton prefix |
| `{PROGRAMFILES}` | Program Files | — | — |
| `{HOME}` | profile | home | home |
| `{XDG_DATA_HOME}` | — | — | XDG data home |
| `{XDG_CONFIG_HOME}` | — | — | XDG config home |
| `{STORE_USER_ID}` | current Steam id | same | same |
| `{STEAM_USERDATA}` | `<root>/userdata/<current steam user>` | same | same |

Proton (Steam, Linux, Windows build): `os: windows` candidates apply when the
install is a Proton install (compatdata prefix exists and no native build is in
use). Windows placeholders resolve inside the prefix:

```
{APPDATA}      → <pfx>/drive_c/users/steamuser/AppData/Roaming
{LOCALAPPDATA} → <pfx>/drive_c/users/steamuser/AppData/Local
{LOCALLOW}     → <pfx>/drive_c/users/steamuser/AppData/LocalLow
{DOCUMENTS}    → <pfx>/drive_c/users/steamuser/Documents
{INSTALL_DIR}  → unchanged (the Windows install dir)
```

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
5. If one candidate is an ancestor of the others and sits at least one named
   segment below a known-folder/install root (never a bare `%APPDATA%`, install
   dir or volume root), it may stand in for the whole set.
6. Ambiguity never blocks the game. The chosen DIR is persisted as the game's
   `data_dir`; checkpoints are bound to it. Later scans do not re-rank while it
   exists; if it disappears, the ladder runs again. Configure lets the user
   override at any time.

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
10. **Linux native**: linux candidates and XDG placeholders.
11. **Linux + Proton**: windows candidates translated into the compatdata prefix;
    same bundle entry as Windows.
12. **Native and Windows builds both shipped**: native install → linux
    candidates; Proton install → translated windows candidates.
13. **Fresh install, no save dir yet**: name rule / first candidate; sticky;
    when the game writes its first save, the pick is already stable.
14. **Saves only in Steam userdata** (Risk of Rain 2): `{STEAM_USERDATA}`
    candidate resolves and participates in the ladder normally.
15. **Upstream rename**: store-id identity keeps the same `id`, so history and
    overrides survive.
16. **Addendum game lands upstream** (Void War): manifest data wins, warning is
    emitted, sheet `Info` still applied.

## 6. Bundle delivery and updates

- The host embeds `catalog/catalog-v1.json` at build time as the fallback.
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

Offline, deterministic, no host or OS dependencies. Inputs are files (sheet,
addendum, manifest path); outputs are the bundle and the report. Library plus a
`catalog-gen` binary. It depends on `crates/catalog` for the bundle model and
validator, so builder output and resolver input cannot drift.

Tests (no network, golden files under `tests/fixtures/catalog/`):

- translation rules: tags, untagged inclusion, glob collapsing, file-vs-dir,
  `<base>`/`<root>` dedupe, placeholder mapping, launch → executables;
- addendum overlay precedence, gap filling and shadow warnings;
- identity derivation and name matching/normalization;
- every hard failure and warning in Section 3.5;
- `games.csv` export matches `games.xlsx` (freshness check in CI);
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
    fn install_identity(&self, install_dir: &Path) -> Option<String>;
    /// Newest modification among files matching `globs`, or all files when empty.
    fn newest_activity(&self, dir: &Path, globs: &[String]) -> Option<u64>;
}

pub struct Install {                    // produced by the scanner, passed in
    pub catalog_id: Id,
    pub store: Store,                   // steam | gog | epic | standalone
    pub platform: Platform,             // windows | macos | linux
    pub install_dir: PathBuf,
    pub executables: Vec<PathBuf>,
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
  `gog-...`). `candidates` exists for diagnostics and the Configure correction
  UI; it is never a blocking prompt.
- Stickiness is explicit: the host passes the persisted `current_pick`;
  `resolve` keeps it while it exists and re-runs the ladder otherwise.

Tests: in-memory `Probe` maps cover every Section 5 case — single and multiple
candidates, activity ordering, sticky retention and re-resolution after
deletion, ancestor guard, store filtering, Proton translation, `{STEAM_USERDATA}`
resolution, install merge/split, unsupported games, rename stability.

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
- Authoring instructions anywhere except `games.xlsx`.

## 9. Decision log

| Decision | Choice |
|---|---|
| Manual inputs | `games.xlsx` + `addendum.yaml` only |
| Manifest precedence | manifest wins per field; addendum fills gaps; shadow warning |
| Multi-target storage | candidate list; no flags |
| Multi-target pick | sticky ladder: existence → newest activity → name → ancestor |
| New/legacy marking | none in manifest; runtime recency decides |
| Installs | one game record per install; merge on identical save DIR |
| Ambiguity UX | auto-pick silently; Configure as correction; never block |
| Steam userdata saves | resolved via current Steam user; included as candidates |
| Untagged file entries | included as candidates |
| Bundle format | versioned JSON, embedded + optionally downloaded |
