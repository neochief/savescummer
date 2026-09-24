# Catalog review: non-stable games

Generated from the bundle revision `95d9b7af64cbe3fc9585395c511aa8c17f821463`. A game is **stable** when it has exactly one Windows save candidate; everything else appears below for a per-game decision.

Status meanings:

- **multi + anchor** — several candidates, exactly one is named `save(s)`/`saved games`, so the pick is reliable.
- **multi, duplicate name** — several candidates share an accept-name; stable order picks the first.
- **multi, no anchor** — several candidates and no naming clue; the runtime guesses by newest file, then first. Highest risk.
- **dead on Windows** — no Windows candidate at all (only macOS/Linux paths, or install-root files we refuse).
- **unresolved** — no usable save directory; omitted from the bundle in bootstrap mode.

34 games need a decision (dead-windows=2, multi=22, unresolved=10).

| # | Game | Status | Candidates | Note |
|---|---|---|---|---|
| 1 | Caves of Qud | multi, duplicate name | 2 | 2 candidates share an accept-name |
| 2 | Jupiter Hell | dead on Windows | 0 | no Windows candidate |
| 3 | Dead Cells | multi + anchor | 2 | name rule applies |
| 4 | Spelunky 2 | unresolved | – | no usable save directory (config-only entries, path resolves to a bare root, config-only entries, MS Store-only saves) |
| 5 | Dungeon Crawl Stone Soup | multi + anchor | 2 | name rule applies |
| 6 | The Binding of Isaac: Rebirth | multi, no anchor | 7 | activity/first pick |
| 7 | Catacomb Kids | unresolved | – | no usable save directory (no files section) |
| 8 | Vagante | unresolved | – | no usable save directory (config-only entries) |
| 9 | Crypt of the NecroDancer | multi, no anchor | 3 | activity/first pick |
| 10 | Slay the Spire | multi + anchor | 4 | name rule applies |
| 11 | One Step from Eden | multi, no anchor | 2 | activity/first pick |
| 12 | UnReal World | unresolved | – | no usable save directory (no files section) |
| 13 | Terraria | multi, no anchor | 4 | activity/first pick |
| 14 | Grim Dawn | multi, duplicate name | 2 | 2 candidates share an accept-name |
| 15 | Don't Starve | multi + anchor | 2 | name rule applies |
| 16 | Baldur's Gate 3 | multi, no anchor | 2 | activity/first pick |
| 17 | Imperator: Rome | multi, no anchor | 2 | activity/first pick |
| 18 | XCOM: Enemy Unknown | multi, no anchor | 2 | activity/first pick |
| 19 | BattleTech | multi, no anchor | 2 | activity/first pick |
| 20 | Total War Saga: Thrones of Britannia | unresolved | – | no usable save directory (no files section) |
| 21 | Total War: Warhammer II | multi, no anchor | 4 | activity/first pick |
| 22 | Total War: Three Kingdoms | multi, no anchor | 2 | activity/first pick |
| 23 | Darkest Dungeon | unresolved | – | no usable save directory (no files section) |
| 24 | Watch Dogs: Legion | unresolved | – | no usable save directory (unsupported store condition, unsupported <root> path form, unsupported store condition, config-only entries) |
| 25 | This War of Mine | multi, no anchor | 2 | activity/first pick |
| 26 | Outward | multi, duplicate name | 2 | 2 candidates share an accept-name |
| 27 | King of Dragon Pass | unresolved | – | no usable save directory (no files section) |
| 28 | Six Ages 2: Lights Going Out | unresolved | – | name unresolved in the manifest and no addendum entry |
| 29 | HighFleet | multi + anchor | 3 | name rule applies |
| 30 | Pentiment | multi, no anchor | 2 | activity/first pick |
| 31 | 80 Days | unresolved | – | no usable save directory (config-only entries) |
| 32 | Enter the Gungeon | multi, no anchor | 2 | activity/first pick |
| 33 | Risk of Rain | dead on Windows | 0 | no Windows candidate |
| 34 | Risk of Rain Returns | multi, no anchor | 2 | activity/first pick |

## 1. Caves of Qud

- **Status**: multi, duplicate name
- **Id**: `steam-333640`
- **Derived Windows candidates**:
  - `{HOME}/AppData/LocalLow/Freehold Games/CavesOfQud/Saves`  {"os":"windows"}
  - `{HOME}/AppData/LocalLow/Freehold Games/CavesOfQud/Synced/Saves`  {"os":"windows","store":"steam"}
- **Watch-outs**:
  - 2 candidates share an accept-name; the first in stable order wins
  - has macOS/Linux-only candidates as well

Raw manifest entry:

```yaml
Caves of Qud:
  cloud:
    steam: true
  files:
    "<home>/AppData/LocalLow/Freehold Games/CavesOfQud/Local/PlayerOptions.json":
      tags:
        - config
      when:
        - os: windows
          store: steam
    "<home>/AppData/LocalLow/Freehold Games/CavesOfQud/PlayerOptions.json":
      tags:
        - config
      when:
        - os: windows
    "<home>/AppData/LocalLow/Freehold Games/CavesOfQud/Saves":
      tags:
        - save
      when:
        - os: windows
    "<home>/AppData/LocalLow/Freehold Games/CavesOfQud/Synced/Saves":
      tags:
        - save
      when:
        - os: windows
          store: steam
    "<home>/Library/Application Support/com.FreeholdGames.CavesOfQud/PlayerOptions.json":
      tags:
        - config
      when:
        - os: mac
    "<home>/Library/Application Support/com.FreeholdGames.CavesOfQud/Saves":
      tags:
        - save
      when:
        - os: mac
    "<xdgConfig>/unity3d/Freehold Games/CavesOfQud/Local/PlayerOptions.json":
      tags:
        - config
      when:
        - os: linux
          store: steam
    "<xdgConfig>/unity3d/Freehold Games/CavesOfQud/PlayerOptions.json":
      tags:
        - config
      when:
        - os: linux
    "<xdgConfig>/unity3d/Freehold Games/CavesOfQud/Saves":
      tags:
        - save
      when:
        - os: linux
    "<xdgConfig>/unity3d/Freehold Games/CavesOfQud/Synced/Saves":
      tags:
        - save
      when:
        - os: linux
          store: steam
  gog:
    id: 1625207125
  id:
    lutris: caves-of-qud
  installDir:
    Caves of Qud: {}
  launch:
    "<base>/CoQ.app":
      - when:
          - os: mac
            store: steam
    "<base>/CoQ.exe":
      - when:
          - os: windows
            store: steam
    "<base>/CoQ.x86":
      - when:
          - bit: 32
            os: linux
            store: steam
    "<base>/CoQ.x86_64":
      - when:
          - bit: 64
            os: linux
            store: steam
  steam:
    id: 333640
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 2. Jupiter Hell

- **Status**: dead on Windows
- **Id**: `steam-811320`
- **Derived Windows candidates**:
- **Watch-outs**:
  - manifest uses globs; they are collapsed and discarded, so activity ranking sees all files
  - has macOS/Linux-only candidates as well

Raw manifest entry:

```yaml
Jupiter Hell:
  cloud:
    steam: true
  files:
    "<base>/configuration.lua":
      tags:
        - config
      when:
        - os: windows
        - os: linux
    "<base>/save":
      tags:
        - save
      when:
        - os: linux
    "<base>/save*":
      tags:
        - save
      when:
        - os: windows
  gog:
    id: 1603513052
  installDir:
    Jupiter Hell: {}
  launch:
    "<base>/JupiterHell.app/Contents/MacOS/jhapp":
      - when:
          - os: mac
            store: steam
    "<base>/jh.exe":
      - arguments: "--gl"
        when:
          - bit: 64
            os: windows
            store: steam
  steam:
    id: 811320
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 3. Dead Cells

- **Status**: multi + anchor
- **Id**: `steam-588650`
- **Derived Windows candidates**:
  - `{INSTALL_DIR}/save`  {"os":"windows"}
  - `{STEAM_USERDATA}/588650/remote`  {"store":"steam"}
- **Watch-outs**:
  - a candidate is under the install dir (snapshots game files)
  - manifest uses globs; they are collapsed and discarded, so activity ranking sees all files
  - has macOS/Linux-only candidates as well

Raw manifest entry:

```yaml
Dead Cells:
  cloud:
    epic: true
    gog: true
    origin: true
    steam: true
  files:
    "<base>/save/customGameData_*.json":
      tags:
        - save
      when:
        - os: windows
        - os: mac
        - os: linux
    "<base>/save/dc_options.json":
      tags:
        - config
      when:
        - os: windows
        - os: mac
        - os: linux
    "<base>/save/user_*.dat":
      tags:
        - save
      when:
        - os: windows
        - os: mac
        - os: linux
    "<root>/userdata/<storeUserId>/588650/remote/customGameData_*.json":
      tags:
        - save
      when:
        - store: steam
    "<root>/userdata/<storeUserId>/588650/remote/dc_options.json":
      tags:
        - config
      when:
        - store: steam
    "<root>/userdata/<storeUserId>/588650/remote/user_*.dat":
      tags:
        - save
      when:
        - store: steam
    "<winLocalAppData>/MotionTwin/DeadCells/customGameData_*.json":
      tags:
        - save
      when:
        - os: windows
          store: microsoft
    "<winLocalAppData>/MotionTwin/DeadCells/dc_options.json":
      tags:
        - config
      when:
        - os: windows
          store: microsoft
    "<winLocalAppData>/MotionTwin/DeadCells/user_*.dat":
      tags:
        - save
      when:
        - os: windows
          store: microsoft
    "<winLocalAppData>/Packages/MotionTwin.DeadCellsWin10_rtjy889c6zgtg/LocalCache/Local/MotionTwin/DeadCells":
      tags:
        - save
      when:
        - os: windows
          store: microsoft
    "<winLocalAppData>/Packages/MotionTwin.DeadCellsWin10_rtjy889c6zgtg/Settings":
      tags:
        - config
      when:
        - os: windows
          store: microsoft
  gog:
    id: 1237807960
  id:
    gogExtra:
      - 1114691340
      - 1199198569
      - 1385821524
      - 1444893699
      - 1537569459
      - 1753058625
      - 1758551289
      - 1804200569
      - 1896771475
      - 1942714997
      - 2044679306
    steamExtra:
      - 665380
      - 1046440
      - 1204130
      - 1393790
      - 1451460
      - 1580050
      - 2101430
  installDir:
    Dead Cells: {}
  launch:
    "<base>/deadcells":
      - when:
          - os: mac
            store: steam
    "<base>/deadcells.exe":
      - when:
          - os: windows
            store: steam
    "<base>/deadcells.sh":
      - when:
          - os: linux
            store: steam
  steam:
    id: 588650
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 4. Spelunky 2

- **Status**: unresolved — no usable save directory (config-only entries, path resolves to a bare root, config-only entries, MS Store-only saves)
- **Manifest key**: `Spelunky 2`

Raw manifest entry:

```yaml
Spelunky 2:
  cloud:
    steam: true
  files:
    "<base>/*.cfg":
      tags:
        - config
      when:
        - os: windows
    "<base>/savegame.sav":
      tags:
        - save
      when:
        - os: windows
    "<winDocuments>/Spelunky2/*.cfg":
      tags:
        - config
      when:
        - os: windows
          store: microsoft
    "<winLocalAppData>/Packages/MossmouthLLC.Spelunky2_as5w07d1cr6c0/SystemAppData/wgs":
      tags:
        - save
      when:
        - os: windows
          store: microsoft
  id:
    lutris: spelunky-2
  installDir:
    Spelunky 2: {}
  launch:
    "<base>/Spel2.exe":
      - arguments: random
        when:
          - bit: 64
            os: windows
            store: steam
  steam:
    id: 418530
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 5. Dungeon Crawl Stone Soup

- **Status**: multi + anchor
- **Id**: `dungeon-crawl-stone-soup`
- **Derived Windows candidates**:
  - `{APPDATA}/Roaming/crawl/morgue`  {"os":"windows"}
  - `{APPDATA}/Roaming/crawl/saves`  {"os":"windows"}

Raw manifest entry:

```yaml
Dungeon Crawl Stone Soup:
  files:
    "<winAppData>/Roaming/crawl/morgue":
      tags:
        - save
      when:
        - os: windows
    "<winAppData>/Roaming/crawl/saves":
      tags:
        - save
      when:
        - os: windows
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 6. The Binding of Isaac: Rebirth

- **Status**: multi, no anchor
- **Id**: `steam-250900`
- **Derived Windows candidates**:
  - `{DOCUMENTS}/My Games/Binding of Isaac Afterbirth`  {"os":"windows"}
  - `{DOCUMENTS}/My Games/Binding of Isaac Afterbirth+`  {"os":"windows"}
  - `{DOCUMENTS}/My Games/Binding of Isaac Rebirth`  {"os":"windows"}
  - `{DOCUMENTS}/My Games/Binding of Isaac Repentance`  {"os":"windows"}
  - `{DOCUMENTS}/My Games/Binding of Isaac Repentance (Galaxy)`  {"os":"windows","store":"gog"}
  - `{DOCUMENTS}/My Games/Binding of Isaac Repentance+`  {"os":"windows"}
  - `{STEAM_USERDATA}/250900/remote`  {"store":"steam"}
- **Watch-outs**:
  - has macOS/Linux-only candidates as well
  - many variants (expansions/versions) — confirm the ladder or curate the list

Raw manifest entry:

```yaml
"The Binding of Isaac: Rebirth":
  cloud:
    gog: true
    steam: true
  files:
    "<home>/Library/Application Support/Binding of Isaac Rebirth":
      tags:
        - config
        - save
      when:
        - os: mac
    "<root>/userdata/<storeUserId>/250900/remote":
      tags:
        - config
        - save
      when:
        - store: steam
    "<winDocuments>/My Games/Binding of Isaac Afterbirth":
      tags:
        - config
        - save
      when:
        - os: windows
    "<winDocuments>/My Games/Binding of Isaac Afterbirth+":
      tags:
        - config
        - save
      when:
        - os: windows
    "<winDocuments>/My Games/Binding of Isaac Rebirth":
      tags:
        - config
        - save
      when:
        - os: windows
    "<winDocuments>/My Games/Binding of Isaac Repentance":
      tags:
        - config
        - save
      when:
        - os: windows
    "<winDocuments>/My Games/Binding of Isaac Repentance (Galaxy)":
      tags:
        - save
      when:
        - os: windows
          store: gog
    "<winDocuments>/My Games/Binding of Isaac Repentance+":
      tags:
        - config
        - save
      when:
        - os: windows
    "<xdgData>/binding of isaac rebirth":
      tags:
        - config
        - save
      when:
        - os: linux
  gog:
    id: 1205572215
  id:
    gogExtra:
      - 1450497670
      - 1485549041
      - 1990507827
    steamExtra:
      - 322660
      - 401920
      - 570660
      - 1426300
      - 3353470
  installDir:
    The Binding of Isaac Rebirth: {}
  launch:
    "<base>/The Binding of Isaac Rebirth.app":
      - when:
          - os: mac
            store: steam
    "<base>/isaac-ng.exe":
      - when:
          - os: windows
            store: steam
    "<base>/run-i386.sh":
      - when:
          - bit: 32
            os: linux
            store: steam
    "<base>/run-x64.sh":
      - when:
          - bit: 64
            os: linux
            store: steam
  steam:
    id: 250900
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 7. Catacomb Kids

- **Status**: unresolved — no usable save directory (no files section)
- **Manifest key**: `Catacomb Kids`

Raw manifest entry:

```yaml
Catacomb Kids:
  installDir:
    Catacomb Kids: {}
  launch:
    "<base>/CatacombKids.exe":
      - when:
          - os: windows
            store: steam
    "<base>/Catacomb_Kids":
      - when:
          - os: linux
            store: steam
    "<base>/Catacomb_Kids.app/Contents/MacOS/Mac_Runner":
      - when:
          - os: mac
            store: steam
  steam:
    id: 315840
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 8. Vagante

- **Status**: unresolved — no usable save directory (config-only entries)
- **Manifest key**: `Vagante`

Raw manifest entry:

```yaml
Vagante:
  cloud:
    steam: true
  files:
    "<base>/config":
      tags:
        - config
      when:
        - os: windows
  installDir:
    vagante: {}
  launch:
    "<base>/vagante.exe":
      - when:
          - store: steam
  steam:
    id: 323220
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 9. Crypt of the NecroDancer

- **Status**: multi, no anchor
- **Id**: `steam-247080`
- **Derived Windows candidates**:
  - `{APPDATA}/NecroDancer`  {"os":"windows"}
  - `{INSTALL_DIR}/data`  {"os":"windows"}
  - `{STEAM_USERDATA}/247080/remote`  {"store":"steam"}
- **Watch-outs**:
  - a candidate is under the install dir (snapshots game files)
  - has macOS/Linux-only candidates as well

Raw manifest entry:

```yaml
Crypt of the NecroDancer:
  cloud:
    gog: true
    steam: true
  files:
    "<base>/data/save_data.xml":
      tags:
        - config
        - save
      when:
        - os: windows
        - os: linux
    "<root>/userdata/<storeUserId>/247080/remote/save_data.xml":
      tags:
        - config
        - save
      when:
        - store: steam
    "<winAppData>/NecroDancer/SynchronyOptions.lua":
      tags:
        - config
      when:
        - os: windows
    "<winAppData>/NecroDancer/SynchronyUser_<storeUserId>.lua":
      tags:
        - save
      when:
        - os: windows
  gog:
    id: 1432297044
  id:
    gogExtra:
      - 1432925124
      - 1432925592
      - 1439377578
      - 1604766360
      - 1754714337
      - 1981284042
      - 2105882438
    lutris: crypt-of-the-necrodancer
    steamExtra:
      - 314680
      - 366080
      - 379400
      - 2094810
      - 2828720
  installDir:
    Crypt of the NecroDancer: {}
  launch:
    "<base>/NecroDancer64/NecroDancer":
      - when:
          - bit: 64
            os: linux
            store: steam
        workingDir: "<base>/NecroDancer64"
    "<base>/NecroDancerSP.app":
      - when:
          - os: mac
            store: steam
    "<base>/Necrodancer/Necrodancer.exe":
      - when:
          - bit: 32
            os: windows
            store: steam
        workingDir: "<base>/Necrodancer"
    "<base>/Necrodancer64/Necrodancer.exe":
      - when:
          - bit: 64
            os: windows
            store: steam
        workingDir: "<base>/Necrodancer64"
  steam:
    id: 247080
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 10. Slay the Spire

- **Status**: multi + anchor
- **Id**: `steam-646570`
- **Derived Windows candidates**:
  - `{INSTALL_DIR}/betaPreferences`  {"os":"windows"}
  - `{INSTALL_DIR}/preferences`  {"os":"windows"}
  - `{INSTALL_DIR}/runs`  {"os":"windows"}
  - `{INSTALL_DIR}/saves`  {"os":"windows"}
- **Watch-outs**:
  - a candidate is under the install dir (snapshots game files)
  - has macOS/Linux-only candidates as well
  - many variants (expansions/versions) — confirm the ladder or curate the list

Raw manifest entry:

```yaml
Slay the Spire:
  cloud:
    gog: true
    origin: true
    steam: true
  files:
    "<base>/SlayTheSpire.app/Contents/Resources/betaPreferences":
      tags:
        - save
      when:
        - os: mac
    "<base>/SlayTheSpire.app/Contents/Resources/preferences":
      tags:
        - save
      when:
        - os: mac
    "<base>/SlayTheSpire.app/Contents/Resources/preferences/STSGameplaySettings":
      tags:
        - config
      when:
        - os: mac
    "<base>/SlayTheSpire.app/Contents/Resources/runs":
      tags:
        - save
      when:
        - os: mac
    "<base>/SlayTheSpire.app/Contents/Resources/saves":
      tags:
        - save
      when:
        - os: mac
    "<base>/betaPreferences":
      tags:
        - config
        - save
      when:
        - os: windows
        - os: linux
    "<base>/info.displayconfig":
      tags:
        - config
      when:
        - os: windows
        - os: linux
    "<base>/preferences":
      tags:
        - config
        - save
      when:
        - os: windows
        - os: linux
    "<base>/runs":
      tags:
        - save
      when:
        - os: windows
        - os: linux
    "<base>/saves":
      tags:
        - save
      when:
        - os: windows
        - os: linux
    "<base>/twitchconfig.txt":
      tags:
        - config
      when:
        - os: windows
        - os: linux
    "<winLocalAppData>/Packages/HumbleBundle.SlayTheSpire_q2mcdwmzx4qja/LocalCache/Local/Microsoft/WritablePackageRoot/info.displayconfig":
      tags:
        - config
      when:
        - os: windows
          store: microsoft
    "<winLocalAppData>/Packages/HumbleBundle.SlayTheSpire_q2mcdwmzx4qja/LocalCache/Local/Microsoft/WritablePackageRoot/preferences":
      tags:
        - config
      when:
        - os: windows
          store: microsoft
    "<winLocalAppData>/Packages/HumbleBundle.SlayTheSpire_q2mcdwmzx4qja/LocalCache/Local/Microsoft/WritablePackageRoot/saves":
      tags:
        - save
      when:
        - os: windows
          store: microsoft
    "<winLocalAppData>/Packages/HumbleBundle.SlayTheSpire_q2mcdwmzx4qja/LocalCache/Local/Microsoft/WritablePackageRoot/twitchconfig.txt":
      tags:
        - config
      when:
        - os: windows
          store: microsoft
  gog:
    id: 1950754973
  id:
    gogExtra:
      - 1103300729
    lutris: slay-the-spire
    steamExtra:
      - 877620
  installDir:
    SlayTheSpire: {}
  launch:
    "<base>/SlayTheSpire":
      - when:
          - os: linux
            store: steam
    "<base>/SlayTheSpire.app/Contents/MacOS/SlayTheSpire":
      - when:
          - os: mac
            store: steam
        workingDir: "<base>/SlayTheSpire.app/Contents/Resources"
    "<base>/jre/bin/javaw.exe":
      - arguments: "-jar desktop-1.0.jar"
        when:
          - os: windows
            store: steam
  steam:
    id: 646570
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 11. One Step from Eden

- **Status**: multi, no anchor
- **Id**: `steam-960690`
- **Derived Windows candidates**:
  - `{HOME}/AppData/LocalLow/Ristaccia LLC/One Step From Eden`  {"os":"windows"}
  - `{STEAM_USERDATA}/960690/remote/SaveData`  {"store":"steam"}
- **Watch-outs**:
  - has macOS/Linux-only candidates as well

Raw manifest entry:

```yaml
One Step from Eden:
  cloud:
    gog: true
    steam: true
  files:
    "<home>/AppData/LocalLow/Ristaccia LLC/One Step From Eden":
      tags:
        - save
      when:
        - os: windows
    "<root>/userdata/<storeUserId>/960690/remote/SaveData":
      tags:
        - config
        - save
      when:
        - store: steam
    "<xdgConfig>/unity3d/Ristaccia LLC/One Step From Eden/SaveData":
      tags:
        - save
      when:
        - os: linux
    "<xdgConfig>/unity3d/Ristaccia LLC/One Step From Eden/prefs":
      tags:
        - config
      when:
        - os: linux
  gog:
    id: 1112715616
  id:
    lutris: one-step-from-eden
    steamExtra:
      - 1007020
  installDir:
    One Step From Eden: {}
  launch:
    "<base>/OSFE.app":
      - when:
          - os: mac
            store: steam
    "<base>/OSFE.exe":
      - when:
          - bit: 32
            os: windows
            store: steam
          - bit: 64
            os: windows
            store: steam
    "<base>/OSFE.x86_64":
      - when:
          - bit: 64
            os: linux
            store: steam
  registry:
    HKEY_CURRENT_USER/SOFTWARE/Ristaccia LLC/One Step From Eden:
      tags:
        - config
  steam:
    id: 960690
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 12. UnReal World

- **Status**: unresolved — no usable save directory (no files section)
- **Manifest key**: `UnReal World`

Raw manifest entry:

```yaml
UnReal World:
  gog:
    id: 1425108206
  id:
    steamExtra:
      - 505100
  installDir:
    UnRealWorld: {}
  launch:
    "<base>/UrW.app/Contents/Resources/urw3-bin":
      - when:
          - os: mac
            store: steam
        workingDir: "<base>/UrW.app/Contents/Resources"
    "<base>/urw.exe":
      - when:
          - os: windows
            store: steam
    "<base>/urw3-bin":
      - when:
          - os: linux
            store: steam
  steam:
    id: 351700
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 13. Terraria

- **Status**: multi, no anchor
- **Id**: `steam-105600`
- **Derived Windows candidates**:
  - `{DOCUMENTS}/My Games/Terraria`  {"os":"windows"}
  - `{STEAM_USERDATA}/105600/remote`  {"store":"steam"}
  - `{STEAM_USERDATA}/105600/remote/players`  {"store":"steam"}
  - `{STEAM_USERDATA}/105600/remote/worlds`  {"store":"steam"}
- **Watch-outs**:
  - manifest uses globs; they are collapsed and discarded, so activity ranking sees all files
  - has macOS/Linux-only candidates as well
  - many variants (expansions/versions) — confirm the ladder or curate the list

Raw manifest entry:

```yaml
Terraria:
  cloud:
    steam: true
  files:
    "<home>/Library/Application Support/Terraria":
      tags:
        - config
        - save
      when:
        - os: mac
    "<root>/userdata/<storeUserId>/105600/remote/achievements-steam.dat":
      tags:
        - save
      when:
        - store: steam
    "<root>/userdata/<storeUserId>/105600/remote/favorites.json":
      tags:
        - config
      when:
        - store: steam
    "<root>/userdata/<storeUserId>/105600/remote/players/*.plr":
      tags:
        - save
      when:
        - store: steam
    "<root>/userdata/<storeUserId>/105600/remote/worlds/*.wld":
      tags:
        - save
      when:
        - store: steam
    "<winDocuments>/My Games/Terraria":
      tags:
        - save
      when:
        - os: windows
    "<winDocuments>/My Games/Terraria/config.json":
      tags:
        - config
      when:
        - os: windows
    "<winDocuments>/My Games/Terraria/input profiles.json":
      tags:
        - config
      when:
        - os: windows
    "<xdgData>/Terraria/Players/*.plr":
      tags:
        - save
      when:
        - os: linux
    "<xdgData>/Terraria/Worlds/*.wld":
      tags:
        - save
      when:
        - os: linux
    "<xdgData>/Terraria/config.json":
      tags:
        - config
      when:
        - os: linux
    "<xdgData>/Terraria/favorites.json":
      tags:
        - config
      when:
        - os: linux
    "<xdgData>/Terraria/input_profiles.json":
      tags:
        - config
      when:
        - os: linux
  gog:
    id: 1207665503
  id:
    lutris: terraria
  installDir:
    Terraria: {}
  launch:
    "<base>/Terraria":
      - when:
          - os: linux
            store: steam
    "<base>/Terraria.app/Contents/MacOS/Terraria":
      - when:
          - os: mac
            store: steam
    "<base>/Terraria.exe":
      - when:
          - os: windows
            store: steam
  steam:
    id: 105600
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 14. Grim Dawn

- **Status**: multi, duplicate name
- **Id**: `steam-219990`
- **Derived Windows candidates**:
  - `{DOCUMENTS}/My Games/Grim Dawn/save`  {"os":"windows"}
  - `{STEAM_USERDATA}/219990/remote/save`  {"store":"steam"}
- **Watch-outs**:
  - 2 candidates share an accept-name; the first in stable order wins

Raw manifest entry:

```yaml
Grim Dawn:
  cloud:
    gog: true
    steam: true
  files:
    "<root>/userdata/<storeUserId>/219990/remote/save":
      tags:
        - save
      when:
        - store: steam
    "<winDocuments>/My Games/Grim Dawn/Settings":
      tags:
        - config
      when:
        - os: windows
    "<winDocuments>/My Games/Grim Dawn/save":
      tags:
        - save
      when:
        - os: windows
  gog:
    id: 1449651388
  id:
    gogExtra:
      - 1353691821
      - 1536648751
      - 1551979801
      - 1700702378
      - 1812959072
      - 1842678741
      - 2004748256
      - 2023885455
    steamExtra:
      - 483840
      - 565610
      - 642280
      - 897670
      - 1088290
      - 2699230
      - 2701090
  installDir:
    Grim Dawn: {}
  launch:
    "<base>/compat/grim dawn.exe":
      - when:
          - bit: 64
            os: windows
            store: steam
    "<base>/grim dawn.exe":
      - when:
          - os: windows
            store: steam
      - arguments: /d3d9
        when:
          - os: windows
            store: steam
    "<base>/x64/grim dawn.exe":
      - when:
          - bit: 64
            os: windows
            store: steam
  steam:
    id: 219990
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 15. Don't Starve

- **Status**: multi + anchor
- **Id**: `steam-219740`
- **Derived Windows candidates**:
  - `{DOCUMENTS}/Klei/DoNotStarve/save`  {"os":"windows"}
  - `{STEAM_USERDATA}/219740/remote`  {"store":"steam"}
- **Watch-outs**:
  - has macOS/Linux-only candidates as well

Raw manifest entry:

```yaml
"Don't Starve":
  cloud:
    steam: true
  files:
    "<home>/.klei/DoNotStarve":
      tags:
        - config
        - save
      when:
        - os: linux
    "<home>/Documents/Klei/DoNotStarve":
      tags:
        - config
        - save
      when:
        - os: mac
    "<root>/userdata/<storeUserId>/219740/remote":
      tags:
        - save
      when:
        - store: steam
    "<winDocuments>/Klei/DoNotStarve/save":
      tags:
        - save
      when:
        - os: windows
    "<winDocuments>/Klei/DoNotStarve/settings.ini":
      tags:
        - config
      when:
        - os: windows
  gog:
    id: 1207659210
  id:
    gogExtra:
      - 1207664293
      - 1459416807
      - 1459422165
    lutris: dont-starve
  installDir:
    dont_starve: {}
  launch:
    "<base>/bin/dontstarve_steam":
      - when:
          - os: linux
            store: steam
        workingDir: "<base>/bin"
    "<base>/bin/dontstarve_steam.exe":
      - when:
          - os: windows
            store: steam
        workingDir: "<base>/bin"
    "<base>/dontstarve_steam.app":
      - when:
          - os: mac
            store: steam
  steam:
    id: 219740
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 16. Baldur's Gate 3

- **Status**: multi, no anchor
- **Id**: `steam-1086940`
- **Derived Windows candidates**:
  - `{LOCALAPPDATA}/Larian Studios/Baldur's Gate 3/PlayerProfiles/Public/Savegames/Story`  {"os":"windows"}
  - `{STEAM_USERDATA}/1086940/remote`  {"store":"steam"}
- **Watch-outs**:
  - has macOS/Linux-only candidates as well

Raw manifest entry:

```yaml
"Baldur's Gate 3":
  cloud:
    gog: true
    steam: true
  files:
    "<home>/Documents/Larian Studios/Baldur's Gate 3/PlayerProfiles":
      tags:
        - config
      when:
        - os: mac
    "<home>/Documents/Larian Studios/Baldur's Gate 3/PlayerProfiles/Public/Savegames/Story":
      tags:
        - save
      when:
        - os: mac
    "<root>/userdata/<storeUserId>/1086940/remote":
      tags:
        - save
      when:
        - store: steam
    "<winLocalAppData>/Lari'an Studios/Baldur's Gate 3/PlayerProfiles":
      tags:
        - config
      when:
        - os: windows
    "<winLocalAppData>/Larian Studios/Baldur's Gate 3/PlayerProfiles/Public/Savegames/Story":
      tags:
        - save
      when:
        - os: windows
    "<winLocalAppData>/Larian Studios/Baldur's Gate 3/analytics.lsx":
      tags:
        - config
      when:
        - os: windows
    "<winLocalAppData>/Larian Studios/Baldur's Gate 3/graphicSettings.lsx":
      tags:
        - config
      when:
        - os: windows
    "<winLocalAppData>/Larian Studios/Baldur's Gate 3/imgui.ini":
      tags:
        - config
      when:
        - os: windows
    "<xdgData>/Larian Studios/Baldur's Gate 3/PlayerProfiles":
      tags:
        - config
      when:
        - os: linux
    "<xdgData>/Larian Studios/Baldur's Gate 3/PlayerProfiles/Public/Savegames/Story":
      tags:
        - save
      when:
        - os: linux
  gog:
    id: 1456460669
  id:
    gogExtra:
      - 1157299235
    lutris: baldurs-gate-3
    steamExtra:
      - 2378500
  installDir:
    Baldurs Gate 3: {}
  launch:
    "<base>/Baldur's Gate 3.app":
      - when:
          - os: mac
            store: steam
    "<base>/Launcher/LariLauncher.exe":
      - when:
          - os: windows
            store: steam
        workingDir: "<base>/bin"
    "<base>/bin/bg3":
      - when:
          - os: linux
            store: steam
        workingDir: "<base>/bin"
  steam:
    id: 1086940
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 17. Imperator: Rome

- **Status**: multi, no anchor
- **Id**: `steam-859580`
- **Derived Windows candidates**:
  - `{DOCUMENTS}/Paradox Interactive/Imperator/save games`  {"os":"windows"}
  - `{STEAM_USERDATA}/859580/remote/save games`  {"store":"steam"}

Raw manifest entry:

```yaml
"Imperator: Rome":
  cloud:
    gog: true
    steam: true
  files:
    "<home>/.local/share/Paradox Interactive/Imperator/save games":
      tags:
        - config
      when:
        - os: linux
    "<root>/userdata/<storeUserId>/859580/remote/save games":
      tags:
        - save
      when:
        - store: steam
    "<winDocuments>/Paradox Interactive/Imperator/pdx_settings.txt":
      tags:
        - config
      when:
        - os: windows
    "<winDocuments>/Paradox Interactive/Imperator/save games":
      tags:
        - save
      when:
        - os: windows
  gog:
    id: 1198397489
  id:
    gogExtra:
      - 1116644747
      - 1394023851
      - 1410813019
      - 1685395366
      - 1705617373
      - 1831875641
      - 1889034386
      - 2025155059
      - 2131232214
    steamExtra:
      - 978950
      - 978951
      - 1016070
      - 1016080
      - 1070470
      - 1173860
      - 1252870
      - 1437660
  installDir:
    ImperatorRome: {}
  steam:
    id: 859580
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 18. XCOM: Enemy Unknown

- **Status**: multi, no anchor
- **Id**: `steam-200510`
- **Derived Windows candidates**:
  - `{DOCUMENTS}/My Games/XCOM - Enemy Unknown/XComGame/SaveData`  {"os":"windows"}
  - `{DOCUMENTS}/My Games/XCOM - Enemy Within/XComGame/SaveData`  {"os":"windows"}
- **Watch-outs**:
  - has macOS/Linux-only candidates as well

Raw manifest entry:

```yaml
"XCOM: Enemy Unknown":
  cloud:
    gog: true
    steam: true
  files:
    "<home>/Library/Application Support/Feral Interactive/XCOM Enemy Unknown":
      tags:
        - config
        - save
      when:
        - os: mac
    "<winDocuments>/My Games/XCOM - Enemy Unknown/XComGame/Config":
      tags:
        - config
      when:
        - os: windows
    "<winDocuments>/My Games/XCOM - Enemy Unknown/XComGame/SaveData":
      tags:
        - save
      when:
        - os: windows
    "<winDocuments>/My Games/XCOM - Enemy Within/XComGame/Config":
      tags:
        - config
      when:
        - os: windows
    "<winDocuments>/My Games/XCOM - Enemy Within/XComGame/SaveData":
      tags:
        - save
      when:
        - os: windows
    "<xdgData>/feral-interactive/XCOM/WritableFiles":
      tags:
        - config
      when:
        - os: linux
    "<xdgData>/feral-interactive/XCOM/XEW/savedata":
      tags:
        - save
      when:
        - os: linux
    "<xdgData>/feral-interactive/XCOM/savedata":
      tags:
        - save
      when:
        - os: linux
  gog:
    id: 1558688142
  id:
    lutris: xcom-enemy-unknown
    steamExtra:
      - 209811
      - 209812
      - 225340
  installDir:
    XCom-Enemy-Unknown: {}
  launch:
    "<base>/Binaries/Win32/XComGame.exe":
      - when:
          - os: windows
            store: steam
    "<base>/XCOM Enemy Unknown.app":
      - when:
          - os: mac
            store: steam
    "<base>/xcom.sh":
      - arguments: "-arch=x86_64"
        when:
          - bit: 64
            os: linux
            store: steam
  steam:
    id: 200510
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 19. BattleTech

- **Status**: multi, no anchor
- **Id**: `steam-637090`
- **Derived Windows candidates**:
  - `{HOME}/AppData/LocalLow/Harebrained Schemes/BattleTech`  {"os":"windows"}
  - `{STEAM_USERDATA}/637090/remote`  {"store":"steam"}
- **Watch-outs**:
  - manifest uses globs; they are collapsed and discarded, so activity ranking sees all files
  - has macOS/Linux-only candidates as well

Raw manifest entry:

```yaml
BattleTech:
  cloud:
    gog: true
    steam: true
  files:
    "<home>/.config/unity3d/Harebrained Schemes/BATTLETECH":
      tags:
        - config
      when:
        - os: linux
    "<home>/.config/unity3d/Harebrained Schemes/BATTLETECH/C*/SGS*":
      tags:
        - save
      when:
        - os: linux
    "<home>/AppData/LocalLow/Harebrained Schemes/BattleTech/C*/SGS*":
      tags:
        - save
      when:
        - os: windows
    "<home>/Library/Application Support/Harebrained Schemes/BattleTech/C*/SGS*":
      tags:
        - save
      when:
        - os: mac
    "<root>/userdata/<storeUserId>/637090/remote/C*/SGS*":
      tags:
        - save
      when:
        - store: steam
    "<winLocalAppData>/GOG.com/Galaxy/Applications/50593543263669699/Storage/Shared/Files/C*/SGS*":
      tags:
        - save
      when:
        - os: windows
          store: gog
    "<winLocalAppData>/Packages/ParadoxInteractive.Battletech-MainGame_zfnrdv2de78ny/SystemAppData/wgs":
      tags:
        - save
      when:
        - os: windows
          store: microsoft
  gog:
    id: 1482783682
  id:
    gogExtra:
      - 1386090191
    lutris: battletech
    steamExtra:
      - 799750
      - 799751
      - 799790
      - 911930
      - 1047180
  installDir:
    BATTLETECH: {}
  launch:
    "<base>/BattleTech":
      - when:
          - bit: 64
            os: linux
            store: steam
    "<base>/BattleTech.app/Contents/MacOS/BattleTech":
      - when:
          - os: mac
            store: steam
    "<base>/BattleTechLauncher.exe":
      - arguments: "-useCurrentSettings"
        when:
          - bit: 64
            os: windows
            store: steam
  registry:
    HKEY_CURRENT_USER/Software/Harebrained Schemes/BATTLETECH:
      tags:
        - config
  steam:
    id: 637090
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 20. Total War Saga: Thrones of Britannia

- **Status**: unresolved — no usable save directory (no files section)
- **Manifest key**: `Total War Saga: Thrones of Britannia`

Raw manifest entry:

```yaml
"Total War Saga: Thrones of Britannia":
  cloud:
    steam: true
  installDir:
    Total War Saga Thrones of Britannia: {}
  launch:
    "<base>/Thrones of Britannia.app":
      - when:
          - os: mac
            store: steam
    "<base>/ThronesOfBritannia.sh":
      - when:
          - os: linux
            store: steam
    "<base>/launcher/launcher.exe":
      - when:
          - os: windows
            store: steam
        workingDir: "<base>/launcher"
  steam:
    id: 712100
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 21. Total War: Warhammer II

- **Status**: multi, no anchor
- **Id**: `steam-594570`
- **Derived Windows candidates**:
  - `{APPDATA}/The Creative Assembly/Warhammer2/EOS/save_games`  {"os":"windows","store":"epic"}
  - `{APPDATA}/The Creative Assembly/Warhammer2/EOS/save_games_multiplayer`  {"os":"windows","store":"epic"}
  - `{APPDATA}/The Creative Assembly/Warhammer2/save_games`  {"os":"windows"}
  - `{APPDATA}/The Creative Assembly/Warhammer2/save_games_multiplayer`  {"os":"windows"}
- **Watch-outs**:
  - has macOS/Linux-only candidates as well
  - many variants (expansions/versions) — confirm the ladder or curate the list

Raw manifest entry:

```yaml
"Total War: Warhammer II":
  cloud:
    steam: true
  files:
    "<winAppData>/The Creative Assembly/Warhammer2/EOS/save_games":
      tags:
        - save
      when:
        - os: windows
          store: epic
    "<winAppData>/The Creative Assembly/Warhammer2/EOS/save_games_multiplayer":
      tags:
        - save
      when:
        - os: windows
          store: epic
    "<winAppData>/The Creative Assembly/Warhammer2/EOS/scripts":
      tags:
        - config
      when:
        - os: windows
          store: epic
    "<winAppData>/The Creative Assembly/Warhammer2/GDK/scripts":
      tags:
        - config
      when:
        - os: windows
          store: microsoft
    "<winAppData>/The Creative Assembly/Warhammer2/save_games":
      tags:
        - save
      when:
        - os: windows
    "<winAppData>/The Creative Assembly/Warhammer2/save_games_multiplayer":
      tags:
        - save
      when:
        - os: windows
    "<winAppData>/The Creative Assembly/Warhammer2/scripts":
      tags:
        - config
      when:
        - os: windows
    "<xdgData>/feral-interactive/Total War WARHAMMER II/SaveData/Steam Saves (<storeUserId>)/local/Warhammer2/save_games":
      tags:
        - save
      when:
        - os: linux
    "<xdgData>/feral-interactive/Total War WARHAMMER II/SaveData/Steam Saves (<storeUserId>)/scripts":
      tags:
        - config
      when:
        - os: linux
  id:
    steamExtra:
      - 651460
  installDir:
    Total War WARHAMMER II: {}
  launch:
    "<base>/Total War WARHAMMER II.app":
      - when:
          - os: mac
            store: steam
    "<base>/TotalWarhammer2.sh":
      - when:
          - os: linux
            store: steam
  steam:
    id: 594570
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 22. Total War: Three Kingdoms

- **Status**: multi, no anchor
- **Id**: `steam-779340`
- **Derived Windows candidates**:
  - `{APPDATA}/The Creative Assembly/ThreeKingdoms/EOS/save_games`  {"os":"windows","store":"epic"}
  - `{APPDATA}/The Creative Assembly/ThreeKingdoms/save_games`  {"os":"windows"}
- **Watch-outs**:
  - has macOS/Linux-only candidates as well

Raw manifest entry:

```yaml
"Total War: Three Kingdoms":
  cloud:
    steam: true
  files:
    "<home>/.local/share/feral-interactive/Three Kingdoms/VFS/User/AppData/Roaming/The Creative Assembly/ThreeKingdoms/save_games":
      tags:
        - save
      when:
        - os: linux
    "<home>/.local/share/feral-interactive/Three Kingdoms/VFS/User/AppData/Roaming/The Creative Assembly/ThreeKingdoms/scripts/preferences.script.txt":
      tags:
        - config
      when:
        - os: linux
    "<winAppData>/The Creative Assembly/ThreeKingdoms/EOS/save_games":
      tags:
        - save
      when:
        - os: windows
          store: epic
    "<winAppData>/The Creative Assembly/ThreeKingdoms/EOS/scripts/preferences.script.txt":
      tags:
        - config
      when:
        - os: windows
          store: epic
    "<winAppData>/The Creative Assembly/ThreeKingdoms/save_games":
      tags:
        - save
      when:
        - os: windows
    "<winAppData>/The Creative Assembly/ThreeKingdoms/scripts/preferences.script.txt":
      tags:
        - config
      when:
        - os: windows
  gog:
    id: 1717887914
  id:
    gogExtra:
      - 1204326102
      - 1225187978
      - 1231104815
      - 1305988655
      - 1328040588
      - 1788989593
      - 2075966521
      - 2108531839
      - 2117008604
      - 2118933928
    steamExtra:
      - 853360
      - 874310
      - 1102310
      - 1154760
      - 1180600
      - 1209110
      - 1244640
      - 1299590
      - 1299591
      - 1493250
  installDir:
    Total War THREE KINGDOMS: {}
  launch:
    "<base>/Three Kingdoms.app":
      - when:
          - os: mac
            store: steam
    "<base>/ThreeKingdoms.sh":
      - when:
          - os: linux
            store: steam
  steam:
    id: 779340
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 23. Darkest Dungeon

- **Status**: unresolved — no usable save directory (no files section)
- **Manifest key**: `Darkest Dungeon`

Raw manifest entry:

```yaml
Darkest Dungeon:
  gog:
    id: 1450711444
  id:
    gogExtra:
      - 1128594953
      - 1452238359
      - 1452693347
      - 1589545552
      - 1902657506
      - 1957260232
    steamExtra:
      - 445700
      - 580100
      - 702540
      - 735730
      - 1117860
      - 4964110
  installDir:
    DarkestDungeon: {}
  launch:
    "<base>/_linux/darkest.bin.x86":
      - arguments: "-skipvalidation"
        when:
          - bit: 32
            os: linux
            store: steam
    "<base>/_linux/darkest.bin.x86_64":
      - arguments: "-skipvalidation"
        when:
          - bit: 64
            os: linux
            store: steam
    "<base>/_osx/darkest.app":
      - arguments: "-skipvalidation"
        when:
          - os: mac
            store: steam
  steam:
    id: 262060
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 24. Watch Dogs: Legion

- **Status**: unresolved — no usable save directory (unsupported store condition, unsupported <root> path form, unsupported store condition, config-only entries)
- **Manifest key**: `Watch Dogs: Legion`

Raw manifest entry:

```yaml
"Watch Dogs: Legion":
  cloud:
    uplay: true
  files:
    "<root>/savegames/<storeUserId>/3353":
      tags:
        - save
      when:
        - os: windows
          store: uplay
    "<root>/savegames/<storeUserId>/7017":
      tags:
        - save
      when:
        - store: steam
        - store: uplay
    "<winDocuments>/My Games/Watch Dogs Legion/WD3_GamerProfile.xml":
      tags:
        - config
      when:
        - os: windows
  id:
    lutris: watch-dogs-legion
    steamExtra:
      - 2239575
      - 2239577
  installDir:
    WatchDogs_Legion: {}
  launch:
    "<base>/bin/WatchDogsLegion.exe":
      - arguments: "-uplay_steam_mode -sound2d"
        when:
          - store: steam
  registry:
    HKEY_CURRENT_USER/SOFTWARE/Ubisoft/WatchDogsLegion:
      tags:
        - config
  steam:
    id: 2239550
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 25. This War of Mine

- **Status**: multi, no anchor
- **Id**: `steam-282070`
- **Derived Windows candidates**:
  - `{APPDATA}/11bitstudios/This War Of Mine`  {"os":"windows"}
  - `{STEAM_USERDATA}/282070/remote`  {"store":"steam"}
- **Watch-outs**:
  - has macOS/Linux-only candidates as well

Raw manifest entry:

```yaml
This War of Mine:
  cloud:
    epic: true
    gog: true
    origin: true
    steam: true
  files:
    "<home>/.This War of Mine":
      tags:
        - config
        - save
      when:
        - os: linux
    "<home>/Library/Application Support/This War of Mine":
      tags:
        - config
        - save
      when:
        - os: mac
    "<root>/userdata/<storeUserId>/282070/remote":
      tags:
        - save
      when:
        - store: steam
    "<winAppData>/11bitstudios/This War Of Mine":
      tags:
        - config
        - save
      when:
        - os: windows
  gog:
    id: 1207666873
  id:
    gogExtra:
      - 1109065057
      - 1195141526
      - 1224169156
      - 1513615737
      - 1689362899
      - 1722846437
    steamExtra:
      - 348040
      - 481090
      - 750030
      - 750031
      - 974610
      - 1125630
  installDir:
    This War of Mine: {}
  launch:
    "<base>/Storyteller.exe":
      - when:
          - os: windows
            store: steam
    "<base>/This War of Mine":
      - when:
          - os: linux
            store: steam
    "<base>/This War of Mine.app":
      - when:
          - os: mac
            store: steam
    "<base>/x64/This War of Mine.exe":
      - when:
          - os: windows
            store: steam
  steam:
    id: 282070
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 26. Outward

- **Status**: multi, duplicate name
- **Id**: `steam-794260`
- **Derived Windows candidates**:
  - `{INSTALL_DIR}/Outward_Defed/SaveGames`  {"os":"windows"}
  - `{INSTALL_DIR}/SaveGames`  {"os":"windows"}
- **Watch-outs**:
  - 2 candidates share an accept-name; the first in stable order wins
  - a candidate is under the install dir (snapshots game files)

Raw manifest entry:

```yaml
Outward:
  cloud:
    gog: true
  files:
    "<base>/OptionSettings.oos":
      tags:
        - config
      when:
        - os: windows
    "<base>/Outward_Defed/OptionSettings.oos":
      tags:
        - config
      when:
        - os: windows
    "<base>/Outward_Defed/Player0_Keymappings.xml":
      tags:
        - config
      when:
        - os: windows
    "<base>/Outward_Defed/SaveGames":
      tags:
        - save
      when:
        - os: windows
    "<base>/Player0_Keymappings.xml":
      tags:
        - config
      when:
        - os: windows
    "<base>/SaveGames":
      tags:
        - save
      when:
        - os: windows
  gog:
    id: 2147483078
  id:
    gogExtra:
      - 1077792303
      - 1091409855
      - 1181883665
      - 1427038628
      - 1821002034
    steamExtra:
      - 983420
  installDir:
    Outward: {}
  steam:
    id: 794260
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 27. King of Dragon Pass

- **Status**: unresolved — no usable save directory (no files section)
- **Manifest key**: `King of Dragon Pass`

Raw manifest entry:

```yaml
King of Dragon Pass:
  gog:
    id: 1207659096
  id:
    lutris: king-of-dragon-pass
  registry:
    HKEY_CURRENT_USER/Software/A Sharp/King of Dragon Pass:
      tags:
        - config
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 28. Six Ages 2: Lights Going Out

- **Status**: unresolved — name unresolved in the manifest and no addendum entry
- **Manifest key**: none (not in the manifest; needs an addendum entry)


Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 29. HighFleet

- **Status**: multi + anchor
- **Id**: `steam-1434950`
- **Derived Windows candidates**:
  - `{INSTALL_DIR}/Saves`  {"os":"windows","store":"steam"}
  - `{INSTALL_DIR}/SavesSkirmish`  {"os":"windows","store":"steam"}
  - `{INSTALL_DIR}/Ships`  {"os":"windows","store":"steam"}
- **Watch-outs**:
  - a candidate is under the install dir (snapshots game files)

Raw manifest entry:

```yaml
HighFleet:
  cloud:
    steam: true
  files:
    "<base>/Config.ini":
      tags:
        - config
      when:
        - os: windows
    "<base>/Saves":
      tags:
        - save
      when:
        - os: windows
    "<base>/SavesSkirmish":
      tags:
        - save
      when:
        - os: windows
    "<base>/Ships":
      tags:
        - save
      when:
        - os: windows
    "<root>/steamapps/common/HighFleet/Config.ini":
      tags:
        - config
      when:
        - store: steam
    "<root>/steamapps/common/HighFleet/Saves":
      tags:
        - save
      when:
        - store: steam
    "<root>/steamapps/common/HighFleet/SavesSkirmish":
      tags:
        - save
      when:
        - store: steam
    "<root>/steamapps/common/HighFleet/Ships":
      tags:
        - save
      when:
        - store: steam
  gog:
    id: 1589167087
  installDir:
    HighFleet: {}
  launch:
    "<base>/Highfleet.exe":
      - when:
          - os: windows
            store: steam
  steam:
    id: 1434950
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 30. Pentiment

- **Status**: multi, no anchor
- **Id**: `steam-1205520`
- **Derived Windows candidates**:
  - `{HOME}/AppData/LocalLow/Obsidian Entertainment/Pentiment/CloudSync`  {"os":"windows","store":"steam"}
  - `{HOME}/Saved Games/Pentiment`  {"os":"windows","store":"steam"}

Raw manifest entry:

```yaml
Pentiment:
  cloud:
    steam: true
  files:
    "<home>/AppData/LocalLow/Obsidian Entertainment/Pentiment/CloudSync":
      tags:
        - save
      when:
        - os: windows
          store: steam
    "<home>/Saved Games/Pentiment":
      tags:
        - save
      when:
        - os: windows
          store: steam
    "<winLocalAppData>/Packages/Microsoft.OE-Missouri_8wekyb3d8bbwe/SystemAppData/wgs":
      tags:
        - save
      when:
        - os: windows
          store: microsoft
  id:
    steamExtra:
      - 2207830
  installDir:
    Pentiment: {}
  launch:
    "<base>/Pentiment.exe":
      - arguments: "-steam"
        when:
          - store: steam
  registry:
    HKEY_CURRENT_USER/Software/Obsidian Entertainment/Pentiment:
      tags:
        - config
  steam:
    id: 1205520
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 31. 80 Days

- **Status**: unresolved — no usable save directory (config-only entries)
- **Manifest key**: `80 Days`

Raw manifest entry:

```yaml
80 Days:
  files:
    "<base>/setup.ini":
      tags:
        - config
      when:
        - os: windows
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 32. Enter the Gungeon

- **Status**: multi, no anchor
- **Id**: `steam-311690`
- **Derived Windows candidates**:
  - `{HOME}/AppData/LocalLow/Dodge Roll/Enter the Gungeon`  {"os":"windows"}
  - `{STEAM_USERDATA}/311690`  {"store":"steam"}
- **Watch-outs**:
  - manifest uses globs; they are collapsed and discarded, so activity ranking sees all files
  - has macOS/Linux-only candidates as well

Raw manifest entry:

```yaml
Enter the Gungeon:
  cloud:
    epic: true
    gog: true
    steam: true
  files:
    "<home>/.config/unity3d/Dodge Roll/Enter the Gungeon":
      tags:
        - config
        - save
      when:
        - os: linux
    "<home>/AppData/LocalLow/Dodge Roll/Enter the Gungeon":
      tags:
        - save
      when:
        - os: windows
    "<home>/AppData/LocalLow/Dodge Roll/Enter the Gungeon/Slot*.options":
      tags:
        - config
      when:
        - os: windows
    "<home>/Library/Application Support/Dodge Roll/Enter the Gungeon":
      tags:
        - save
      when:
        - os: mac
    "<root>/userdata/<storeUserId>/311690":
      tags:
        - config
        - save
      when:
        - store: steam
  gog:
    id: 1456912569
  id:
    gogExtra:
      - 1459847591
    lutris: enter-the-gungeon
    steamExtra:
      - 457840
      - 457841
      - 457842
  installDir:
    Enter the Gungeon: {}
  launch:
    "<base>/EtG.exe":
      - when:
          - os: windows
            store: steam
    "<base>/EtG.x86":
      - when:
          - bit: 32
            os: linux
            store: steam
    "<base>/EtG.x86_64":
      - when:
          - bit: 64
            os: linux
            store: steam
    "<base>/EtG_OSX.app":
      - when:
          - os: mac
            store: steam
  registry:
    HKEY_CURRENT_USER/SOFTWARE/Dodge Roll/Enter the Gungeon:
      tags:
        - config
  steam:
    id: 311690
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 33. Risk of Rain

- **Status**: dead on Windows
- **Id**: `steam-248820`
- **Derived Windows candidates**:
- **Watch-outs**:
  - has macOS/Linux-only candidates as well

Raw manifest entry:

```yaml
Risk of Rain:
  cloud:
    steam: true
  files:
    "<base>/Prefs.ini":
      tags:
        - config
      when:
        - os: windows
    "<base>/Save.ini":
      tags:
        - save
      when:
        - os: windows
    "<base>/Save_backup.ini":
      tags:
        - save
      when:
        - os: windows
    "<home>/Library/Application Support/com.riskofrain.riskofrain/prefs.ini":
      tags:
        - config
      when:
        - os: mac
    "<home>/Library/Application Support/com.riskofrain.riskofrain/save.ini":
      tags:
        - save
      when:
        - os: mac
    "<xdgConfig>/Risk_of_Rain/prefs.ini":
      tags:
        - config
      when:
        - os: linux
    "<xdgConfig>/Risk_of_Rain/save.ini":
      tags:
        - save
      when:
        - os: linux
  gog:
    id: 1207660563
  id:
    lutris: risk-of-rain
  installDir:
    Risk of Rain: {}
  launch:
    "<base>/Risk of Rain.app":
      - when:
          - os: mac
            store: steam
    "<base>/Risk of Rain.exe":
      - when:
          - os: windows
            store: steam
    "<base>/run.sh":
      - when:
          - os: linux
            store: steam
  steam:
    id: 248820
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

## 34. Risk of Rain Returns

- **Status**: multi, no anchor
- **Id**: `steam-1337520`
- **Derived Windows candidates**:
  - `{APPDATA}/Risk_of_Rain_Returns`  {"os":"windows"}
  - `{STEAM_USERDATA}/1337520/remote`  {"store":"steam"}
- **Watch-outs**:
  - manifest uses globs; they are collapsed and discarded, so activity ranking sees all files

Raw manifest entry:

```yaml
Risk of Rain Returns:
  cloud:
    steam: true
  files:
    "<root>/userdata/<storeUserId>/1337520/remote":
      tags:
        - save
      when:
        - store: steam
    "<winAppData>/Risk_of_Rain_Returns/*_localsave.json":
      tags:
        - save
      when:
        - os: windows
    "<winAppData>/Risk_of_Rain_Returns/user_<storeUserId>":
      tags:
        - config
      when:
        - os: windows
  id:
    steamExtra:
      - 2673200
  installDir:
    Risk of Rain Returns: {}
  launch:
    "<base>/Risk of Rain Returns.exe":
      - when:
          - os: windows
            store: steam
  steam:
    id: 1337520
```

Decision: `[ ] keep as-is`  `[ ] fix catalog`  `[ ] remove`  `[ ] add addendum`
Notes:

