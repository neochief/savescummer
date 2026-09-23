# Addendum research prompt

Task: for each game in `catalog/addendum-research.csv`, find the Windows save
directory that SaveScummer should back up, then produce one `catalog/addendum.yaml`
entry per game.

## Input

Read `catalog/addendum-research.csv`. Columns already filled:

- `Name` — entry key in `addendum.yaml` (must match the sheet `Name` exactly).
- `ManifestKey` — the Ludusavi key, empty when the game is absent upstream.
- `SteamAppId`, `GogId` — store ids for `detect` (may be empty).
- `InstallDir` — install folder name seed (may be empty).
- `WindowsExe` — main executable seed (may be empty; research it).
- `Reason` — why the manifest is unusable for this game.
- `Notes` — sheet category, for context only.

Columns to fill: `SaveDir`, `Evidence`.

## Output

`catalog/addendum.yaml` entry per game, keyed by `Name`:

```yaml
Darkest Dungeon:
  detect:
    steam: 262060
    gog: 1450711444
  executables:
    windows: ["_windows/darkest.exe"]
  save:
    - when: { os: windows }
      dir: "{INSTALL_DIR}/..."
```

Plus the CSV columns: `SaveDir` (same template as the YAML), `Evidence` (one
URL per game, e.g. the PCGamingWiki "Save game data location" anchor or a
manufacturer/community source).

## Rules

- Exactly one save directory per entry. Pick the folder holding the actual
  progress, not settings/config.
- Templates: `{INSTALL_DIR}` for paths inside the install folder; `{APPDATA}`,
  `{LOCALAPPDATA}`, `{DOCUMENTS}`, `{HOME}`, `{PROGRAMDATA}` for absolute
  locations.
- If the location differs by store, add one `save` entry per store:
  `when: { os: windows, store: steam }` / `store: gog` / `store: epic`.
- If several plausible folders exist (old/new version, per-profile), list each
  as its own `save` entry; do not guess which is primary and do not merge them.
- Prefer facts over convention. No invented paths.
- Mark uncertainty in `Notes` and still provide the best-supported candidate.

## Games

| Name | Reason |
|---|---|
| Catacomb Kids | no files section upstream |
| Vagante | config-only upstream |
| UnReal World | no files section upstream |
| A Total War Saga: THRONES OF BRITANNIA | no files section upstream |
| Darkest Dungeon | no files section upstream; no Windows launch exe upstream either |
| Watch Dogs: Legion | only `<root>/savegames/<storeUserId>/...` (unsupported form) |
| King of Dragon Pass | no files section upstream; GOG-only entry |
| Six Ages 2: Lights Going Out | absent from upstream entirely |
| 80 Days | config-only upstream |