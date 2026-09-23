# PLAN-INFRA.md — Build, Distribution, and Release Infrastructure

Status: **Phase A complete; Phase B implemented; Phase C implemented** (CI + draft release workflow)
Last reviewed: 2026-09-23

This document is the handoff plan for reorganizing the **dev/build/release
infrastructure** of SaveScummer. It is a *plan*, not an implementation. A fresh
session should be able to pick it up and execute the phases below without
re-deriving the decisions.

It complements `PLAN.md`, which remains authoritative for **application
behavior**. This file only governs **build, packaging, distribution, and
release tooling**. It supersedes the "Reorganize build and distribution output"
task sketched at the bottom of `PLAN.md` (around line 840) and expands it into a
three-phase roadmap.

---

## 1. Context (for a fresh session)

SaveScummer is a Windows desktop app for backing up/restoring game saves.
Three executables, one per canonical identity (see `PLAN.md`):

- `SaveScummer.exe` — C++/Qt 6 Widgets desktop client
- `SaveScummer.Host.exe` — Rust background host (owns SQLite, monitoring, ops)
- `SaveScummer.CLI.exe` — Rust command-line client

Plus a native Windows Explorer shell extension (`SaveScummer-explorer`),
currently built **separately** and registered by hand.

Key backend facts:
- Rust workspace (`Cargo.toml`), version `0.1.0` in `[workspace.package]`.
- Qt 6.5.3 lives locally at `.runtime/Qt/6.5.3/msvc2019_64` (this machine only;
  scripts do not download SDKs). Renders the app needs Qt *installed*.
- CMake 3.21+ project (`CMakeLists.txt`), default generator `Visual Studio 16 2019`.
- PowerShell 7 (`pwsh`) is required (VS Code tasks already assume it).
- `git` tags: **none exist yet**. First release will create `v0.1.0`.
- GitHub CLI `gh` is installed at `C:\Program Files\GitHub CLI\gh.exe`
  (auth state must be verified; see Phase A item A8).
- `scripts/check.ps1` runs fmt/clippy/tests/build; `build.ps1` (root) is the
  single entry point coordinating Rust + Qt + packaging.

### Current disk usage (audit, 2026-09-22)

| Path | Size | Contents / verdict |
|---|---|---|
| `target/` | 2.9 GB | Cargo cache — normal, keep. |
| `build/` | 154 MB | `dev/`, `release/`, plus a **legacy `build/desktop`** CMake default that two scripts still fall back to. |
| `.runtime/Qt` | 211 MB | Vendored Qt SDK — legitimate machine-local tool. |
| `.runtime/qt-tools` | 110 MB | Vendored CMake — legitimate machine-local tool. |
| `.runtime/test-temp` | **520 MB** | Leftover TEMP dir from `build.ps1 -Test` runs — **never cleaned**. Bug. |
| `.runtime/*-smoke-*` etc. | ~1 MB | ~12 one-off scratch dirs (artwork/package/sound/rename smoke, icon-gen, icon-verify). |

`target/`, `build/`, `.runtime/` are all already gitignored; nothing
artifact-like is tracked. Good baseline.

---

## 2. Target layout (the end state, Phase A)

Three-tier policy, plus a `dist/` root for distributables (adopting the
`PLAN.md` sketch):

```
savescummer/
├── target/            Cargo cache only. Never touched directly by scripts.
├── build/             Intermediates (everything regenerable on this machine):
│   ├── dev/             dev CMake tree, logs (host.*.log), session.json, build-report.json,
│   │                    and the dev portable package (<sv>-dev.zip, includes PDBs)
│   └── release/         release CMake tree, build-report.json
│   └── tmp/             disposable scratch (all ad-hoc/smoke outputs; incl. test-temp)
├── dist/              DISTRIBUTABLES (release output; gitignored; flat, OS-tagged names —
│   │                  future platform artifacts land here too, see D10):
│   ├── SaveScummer-windows-x64/            expanded portable application (release)
│   ├── SaveScummer-windows-x64-<ver>.zip   distributable archive (one top-level dir inside)
│   └── (future) SaveScummer-<os>-<arch>-<ver>.<ext> for macOS/Linux release artifacts
├── packaging/
│   └── windows/
│       ├── licenses/   (moved from packaging/licenses; LGPL/GPL + README)
│       └── installer/  (Phase B: Inno Setup script, icons, config)
├── scripts/           powershell helpers (flat; no subfolders)
└── (untouched) crates/, apps/, integrations/, protocol/, catalog/, docs/
```

**Purpose of each root:**

| Root | Definition | Cleanable by `build.ps1 clean`? |
|---|---|---|
| `target/` | Cargo only. | Only with `-Deep`. |
| `build/` | Everything regenerable: CMake trees, logs, reports, scratch, dev packages. | Yes — default. |
| `dist/` | Release distributables (regenerable via `build.ps1 release`). | Yes — default. |
| `.runtime/` | Only long-lived machine-local state: vendored SDKs, dev app data. Nothing disposable. | **Never.** |

---

## 3. Decisions log (with rationale)

| # | Decision | Rationale |
|---|---|---|
| D1 | Distributables move under `dist/`; `build/` becomes intermediates-only. | Matches the existing `PLAN.md` sketch ("Move all distributable output to dist\"). Gives a single obvious place to point the GitHub release script and clean. |
| D2 | Application version is **single-sourced from `Cargo.toml`** (`[workspace.package] version`). CMake reads it; PowerShell scripts read it; the git tag must equal `v<version>`. | One source of truth; removes the current Cargo/CMake drift risk; tag↔version is the natural check before releasing. |
| D3 | **Versioned** artifact names: release zip `dist/SaveScummer-windows-x64-<ver>.zip`; dev zip `build/dev/SaveScummer-windows-x64-<ver>-dev.zip`. The ZIP contains **one top-level `SaveScummer-windows-x64/` directory**. | GitHub release assets must be unique/versioned; the top-level folder keeps extraction clean. (Deviation from the unversioned `SaveScummer-windows-x64.zip` in the `PLAN.md` sketch — deliberate, see rationale. Optional sub-decision: name the top-level folder `SaveScummer-windows-x64-<ver>/` instead if version collision on extraction ever becomes a concern.) |
| D4 | Tiered clean: `build.ps1 clean` wipes `build/` + `dist/`; `clean -Deep` also wipes `target/`; `clean` **never touches `.runtime/`** and gracefully stops the recorded dev host first. | Matches the three-tier policy; dev data and vendored SDKs survive. |
| D5 | Scratch policy: ad-hoc/smoke outputs go to `build/tmp/` and are covered by `clean`. The `-Test` temp moves from `.runtime/test-temp` → `build/tmp/test-temp` and is emptied at each run. | Fixes the 520 MB leak *and* makes `clean` cover scratch. |
| D6 | Retire the legacy `build/desktop` CMake default and delete `scripts/run-desktop.ps1`. `scripts/build-desktop.ps1` gains a `-Mode dev/release` (default `dev`) that selects `build/<mode>/desktop` + the matching configuration; explicit overrides still win. | One place per mode; no competing "legacy" path (per `PLAN.md`: "Remove legacy lower-level output defaults and competing package locations"). |
| D7 | Release publishing is **local-first and draft-first**: `scripts/release-github.ps1` uses `gh release create --draft` so the human reviews the release page and clicks Publish. GitHub Actions is deferred to Phase C (not this task). | Draft-first is the safety gate against publishing something broken; local-first keeps it runnable before CI exists. CI (Phase C) calls the *same* scripts, so nothing is redone. |
| D8 | 1.0 distribution ("do it properly"): **Inno Setup per-user installer** is the primary channel; the portable ZIP stays as the secondary channel; the Explorer extension ships with the app and is registered **on by default** (installer checkbox to disable). | Installer is the correct way to install/uninstall a DLL loaded into Explorer.exe; default-on matches the headline feature (TortoiseGit-style). **Installer is Phase B, not this task.** |
| D9 | Explorer DLL is **not** added to the flow in Phase A. It is wired into the release build and package in Phase B. | Keeps Phase A focused on infra; the DLL build is currently separate (build-explorer.ps1) and wants installer plumbing. |
| D10 | **Multiplatform artifact convention, pinned now.** `dist/` stays **flat**; every artifact is OS/arch-tagged: `SaveScummer-<os>-<arch>-<ver>.<ext>` — `SaveScummer-windows-x64-0.1.0.zip`, later `SaveScummer-macos-arm64-0.1.0.tar.gz` (and `-x86_64-`), `SaveScummer-linux-x86_64-0.1.0.tar.gz`. GitHub release assets reuse these exact filenames. Each artifact keeps a per-platform expanded folder/container; internals stay the canonical executable names from PLAN.md. | OS-tagged filenames make flat `dist/` unambiguous across platforms and give the Phase C CI matrix a uniform output contract (`<target>/dist/*` per OS). Naming is trivial to extend later; deciding it now prevents a Phase A layout that must be reworked. |
| D11 | **Shared packaging core.** `package-windows.ps1` refactors (A3) extract the platform-*neutral* parts — version reading, staging layout, `.savescummer-package.json` manifest, SHA256SUMS generation, README.txt/identify templates, final rename/move semantics — into a dot-sourced `scripts/package-common.ps1` (or small `.psm1`). Future `package-macos.ps1` / `package-linux.ps1` consume it and add only OS-specific steps (Qt deploy, dmgs/AppImage, dylibs, etc.). | PLAN.md's own rules reject duplicated policy across modules. If the shared core is extracted in Phase A, the future per-OS scripts stay small and cannot drift from the Windows conventions. |

---

## 4. Phase A — infrastructure cleanup + local release process

Everything below is Phase A scope. Implement in roughly this order; each item
is independent enough to review separately. Keep changes consistent with the
repo's style (PowerShell 7, `$ErrorActionPreference='Stop'`, clear messages).

### A1 — `.gitignore`
Add `/dist/` (alongside the existing `target/`, `build/`, `.runtime/`).

### A2 — `build.ps1`: `clean` mode + test-temp fix
- Add `clean` to the `$Mode` `ValidateSet` (`dev`, `release`, `clean`) and a
  `[switch]$Deep`. `-Run`/`-Demo` are invalid for `clean` (throw).
- Handle `clean` **early**, right after param validation and before any
  prerequisite/cargo/Qt logic — cleaning must not require a toolchain.
- `clean` behavior:
  1. If `build/dev/session.json` exists, gracefully stop the **recorded dev
     host** using the existing shutdown block (reuse the logic currently in the
     dev-mode pre-build section; consider extracting a `Stop-RecordedDevHost`
     helper so the rebuild path and `clean` share it).
  2. `Remove-Item build -Recurse -Force` and `Remove-Item dist -Recurse -Force`
     (guard each with a `Test-Path`).
  3. If `-Deep`: also `Remove-Item target -Recurse -Force`.
  4. Print a concise summary of what was removed. `clean` must **never** touch
     `.runtime/`.
- `-Test` temp: change lines ~97-99 from `.runtime/test-temp` to
  `build/tmp/test-temp`; **remove it if it exists before each run** (so it
  cannot accumulate), then create it fresh (`.runtime/test-temp` is deleted in
  the one-time cleanup, A12).
- After packaging, print final artifact paths for both modes (already partially
  done; ensure the release path now points at `dist/`).

### A3 — `scripts/package-windows.ps1`: dist output, versioned names, top-level folder
- **First extract the platform-neutral packaging core** (D11): version reading,
  staging layout, `.savescummer-package.json` + `SHA256SUMS.txt` generation,
  README.txt/identity templates, safe final rename/move, into a dot-sourced
  `scripts/package-common.ps1`. `package-windows.ps1` consumes it and keeps only
  Windows-specific steps (cmake `--install`, Qt DLL deploy, vswhere/VC CRT
  discovery, PDB handling, `Stop-PackageProcess`). Future per-OS scripts reuse
  the core unchanged (D10/D11).
- Read the version from `Cargo.toml` (this moves into the shared core):
  ```powershell
  $manifest = Get-Content -LiteralPath (Join-Path $root 'Cargo.toml') -Raw
  if (-not ($manifest -match '(?m)^\s*version\s*=\s*"(\d+\.\d+\.\d+)"')) {
      throw 'Cannot read the application version from Cargo.toml.'
  }
  $version = $Matches[1]
  ```
  Being the **first three-part version field** in the file keeps this pointing
  at `[workspace.package]` (dependency versions like `"1"` / `"0.37"` don't match).
- Output paths:
  - Release: staged package → `dist/SaveScummer-windows-x64/`; zip →
    `dist/SaveScummer-windows-x64-<ver>.zip`; SHA256SUMS.txt inside the package.
  - Dev: staged package → `build/dev/SaveScummer-windows-x64/`; zip →
    `build/dev/SaveScummer-windows-x64-<ver>-dev.zip` (dev keeps PDBs).
- Restructure the staging directory so the **zip root contains exactly one
  top-level `SaveScummer-windows-x64/` folder** (create it inside `$stage`,
  move `bin/`, `licenses/`, `README.txt`, `.savescummer-package.json`,
  `SHA256SUMS.txt` under it), then `Compress-Archive` that folder. Verify the
  archive root has exactly one entry (pwsh 7 uses forward slashes; ok).
- The `$package` guard (currently "must be a `SaveScummer` folder inside
  `repository/build`") must now **allow `dist/` for release and `build/dev/`
  for dev**, and reject anything else. Update the `.savescummer-package.json`
  marker + `Stop-PackageProcess` prefix logic to the new package location.
- Update the generated `README.txt` to mention the versioned zip.
- Update the final "Executable/Archive" output lines to print the new paths.

### A4 — `scripts/build-desktop.ps1`: `-Mode` instead of a legacy default
- Remove the `build/desktop` default `BuildDirectory`.
- Add `-Mode dev|release` (default `dev`); when not overridden, set
  `BuildDirectory = build/<mode>/desktop` and the matching configuration
  (`RelWithDebInfo` for `dev`, `Release` for `release`). Explicit
  `-BuildDirectory` / `-Configuration` arguments still win.
- Keep the existing CMake/ctest logic unchanged.

### A5 — delete `scripts/run-desktop.ps1`
Legacy helper targeting `build/desktop` only. Delete it; it is superseded by
`build.ps1 dev -Run`. Update docs (A9).

### A6 — version single-source in CMake
In `CMakeLists.txt`, replace the hardcoded `project(SaveScummer VERSION 0.1.0 …)`
with a read from `Cargo.toml` (place the `file(READ)` before `project()`; it
needs no compiler):
```cmake
file(READ "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml" _savescummer_manifest)
string(REGEX MATCH "version = \"([0-9]+\\.[0-9]+\\.[0-9]+)\"" _ver "${_savescummer_manifest}")
if(NOT _ver)
    message(FATAL_ERROR "Cannot read the SaveScummer version from Cargo.toml.")
endif()
project(SaveScummer VERSION ${CMAKE_MATCH_1} LANGUAGES CXX)
```
(First three-part version field = `[workspace.package]`; document this
invariant with a comment.)

### A7 — screenshots live under the active build tree
Currently `apps/desktop/tests/desktop_test.cpp` hardcodes
`SOURCE_DIR + "/build/desktop/screenshots/"` in 7 places (`SOURCE_DIR` is
`${PROJECT_SOURCE_DIR}`, defined in `apps/desktop/CMakeLists.txt`). Fix so the
path derives from the CMake build dir instead:
- In `apps/desktop/CMakeLists.txt`, add to the test target's compile
  definitions a `SAVESCUMMER_SCREENSHOT_DIR="${CMAKE_BINARY_DIR}/screenshots"`
  (alongside the existing `FIXTURE_DIR` / `SOURCE_DIR`).
- In `desktop_test.cpp`, replace the 7 hardcoded uses (including the
  `mkpath`) with the value of `SAVESCUMMER_SCREENSHOT_DIR`, falling back to an
  env override (`qEnvironmentVariable`) for flexibility.
- Result: screenshots at `build/<mode>/desktop/screenshots`. Update docs (A9).

### A8 — new `scripts/release-github.ps1` (local, draft-first)
Zero-required-parameter script that publishes the release ZIP as a **draft**:
1. Read `$version` from `Cargo.toml` (same regex as A3).
2. Preconditions (each fails with an actionable message):
   - `gh` on PATH, origin remote exists (`git remote get-url origin`).
   - `gh auth status` succeeds (else instruct `gh auth login`).
   - Tag `v$version` exists (`git rev-parse -q --verify refs/tags/v$version`);
     if missing, print `git tag v<version> && git push origin v<version>` and
     exit non-zero.
   - Tag points at `HEAD` (`git rev-list -n1 v$version` == `git rev-parse HEAD`).
   - Artifact `dist/SaveScummer-windows-x64-<ver>.zip` exists; else instruct
     `./build.ps1 release`.
3. Write sidecar `dist/SaveScummer-windows-x64-<ver>.zip.sha256` (hash of the
   zip) if absent.
4. Publish draft with `gh` semantics:
   - If `gh release view v$version` fails (not created): `gh release create
     v$version --draft --generate-notes --title "SaveScummer v$version" <zip> <sha256>`.
   - If it exists **and is a draft**: `gh release upload v$version <zip> <sha256> --clobber`
     (re-running after a rebuild updates the draft).
   - If it exists **and is published**: error — refuse to clobber a live release.
5. Print the draft URL from `gh release view v$version --json url`.
Design it to be purely additive/local so Phase C's CI workflow can call the
same script (or its logic) unchanged.

### A9 — docs: `docs/building.md` + `README.md`
`docs/building.md`:
- Commands section: add `./build.ps1 clean` and `clean -Deep` (three-tier
  policy; `.runtime/` untouched).
- dev/release table + "Output" list: rewrite to `build/<mode>/desktop` CMake
  trees + `build/release/` report as *intermediates*, and `dist/` +
  `dist/SaveScummer-windows-x64-<ver>.zip` as the distributables (note dev
  package stays in `build/dev/` incl. PDBs).
- Add a **"Publishing a GitHub release"** section: version is single-sourced
  from `Cargo.toml`; tag must equal `v<version>`; commands `git tag v0.1.0`,
  `./build.ps1 release`, `./scripts/release-github.ps1`; draft-first review
  before Publish; mention `gh auth login` once as prerequisite; note CI comes
  later and calls the same steps.
- "Lower-level entry points": remove the `run-desktop.ps1` sentence; describe
  `build-desktop.ps1 -Mode`.
- Update the non-Windows CMake example to not reuse the legacy `build/desktop`
  path (e.g. `build/local-desktop`).

`README.md`:
- Update release-output paths (lines ~18-19 and ~100-101) to
  `dist/SaveScummer-windows-x64-<ver>.zip` (versioned).
- Add `./build.ps1 clean` to the build/test command list; add a one-line
  pointer to `docs/building.md` for release publishing.
- Update the screenshots path (line ~72) to `build/<mode>/desktop/screenshots`.

### A10 — `.vscode`
Verify only — **no changes expected**: `tasks.json` calls `build.ps1` and
`scripts/vscode-session.ps1` (no stale paths); `launch.json` uses
`.runtime/vscode` + `.runtime/Qt` (both stay). Confirm after edits that nothing
references the legacy path.

### A11 — `packaging/` → per-platform home
Move `packaging/licenses/*` → `packaging/windows/licenses/` and update the
`package-windows.ps1` copy path (A3). This is Phase-B prep. Add
`packaging/windows/README.md` explaining the layout and reserving
`packaging/windows/installer/` for Phase B. (Nothing else is created yet.)

### A12 — one-time artifact cleanup
Delete the following (inspected 2026-09-22; all regenerable/derived):
- `.runtime/test-temp` (520 MB; superseded by `build/tmp/test-temp`).
- `.runtime/artwork-smoke-*`, `.runtime/package-check-*`,
  `.runtime/package-smoke-*`, `.runtime/sound-smoke-*`, `.runtime/rename-smoke`
  (ad-hoc smoke scratch).
- `.runtime/icon-gen`, `.runtime/icon-gen-cropped`, `.runtime/icon-verify`
  (PNG size exports derived from `assets/icon.svg`; before deleting, confirm
  `assets/icon.svg` exists — it does).
Keep: `.runtime/dev`, `.runtime/explorer-dev`, `.runtime/vscode`,
`.runtime/Qt`, `.runtime/qt-tools`.
Then run `./build.ps1 clean` to clear `build/` debris (old zips, the legacy
`build/desktop` tree, old screenshots).

---

## 5. Verification / acceptance checklist (Phase A)

Run through `scripts/check.ps1` (fmt/clippy/tests/build) **and** the following:

1. `git status` shows no artifacts tracked; `target/ build/ dist/ .runtime/`
   all gitignored.
2. `./build.ps1 clean` removes `build/` + `dist/` (gracefully stopping the
   recorded dev host first), and `clean -Deep` also removes `target/`;
   `.runtime/` is untouched either way.
3. `./build.ps1 dev -Run` builds and runs; artifacts land under `build/dev/`.
4. `./build.ps1 dev -Test` runs; test temp is recreated fresh at
   `build/tmp/test-temp`.
5. `./build.ps1 release` produces `dist/SaveScummer-windows-x64-<ver>.zip`
   and the expanded `dist/SaveScummer-windows-x64/`; the zip root has exactly
   one `SaveScummer-windows-x64/` entry; `SHA256SUMS.txt` is inside; zip name
   embeds the version; `build/release/build-report.json` still written.
6. CMake configures with the Cargo-sourced version (build output / report shows
   `0.1.0`).
7. Qt tests write screenshots under `build/<mode>/desktop/screenshots`.
8. `./scripts/release-github.ps1` fails cleanly with an actionable message when
   the tag is missing (expected pre-tag). After the user tags `v0.1.0` it
   creates a **draft** release — only actually run when the user wants to
   publish.
9. Repo-wide grep finds **no** stale references to `build/desktop`,
   `run-desktop.ps1`, or the unversioned zip name (`SaveScummer-windows-x64.zip`).
10. `.runtime/` contains only `dev`, `explorer-dev`, `vscode`, `Qt`, `qt-tools`.
11. All modified `.ps1` files parse cleanly (`pwsh -NoProfile -Command
    "$null=[System.Management.Automation.Language.Parser]::ParseFile('<path>',
    [ref]$null,[ref]$e); $e"` returns no errors).

---

## 6. Phase B — 1.0 proper Windows distribution (implemented)

Status: implemented 2026-09-22. The 1.0 Windows distribution is an Inno Setup
per-user installer (primary) plus the portable ZIP (secondary); both carry the
Explorer extension. Decisions taken while implementing:

- **Installer artifact**: `dist/SaveScummer-windows-x64-<ver>-setup.exe`
  (`Get-InstallerArtifactName` in `scripts/package-common.ps1`).
- **Build trigger**: `./build.ps1 release` always builds the Explorer extension,
  packages the portable ZIP, and then compiles the installer. If Inno Setup is
  absent it prints a warning and still produces the portable package.
- **Explorer registration**: the installer writes the `HKCU\Software\Classes`
  CLSID and `Directory\shellex\ContextMenuHandlers\SaveScummer` keys directly
  (no `pwsh` dependency on the target). The portable package opts in through
  `Enable/Disable Explorer integration.cmd` + `bin\register-explorer.ps1`.
  The DLL uses `restartreplace`, because `explorer.exe` keeps it mapped and
  Windows cannot overwrite a loaded DLL in place.
- **Sign-in entry**: the installer's opt-in `startup` task writes the
  `HKCU\...\Run\SaveScummer` value in the exact form the host uses
  (`--minimized --data-dir ... --desktop ...`), so sign-in autostart works
  immediately after install. The app's own preference stays off by default; on
  first start the host reads the registry value and adopts its current state
  (`StartupRegistration::is_enabled`), after which the checkbox changes it. The
  host never takes over an entry that points at another build or recreates a
  removed one. Matching parses the command line and compares the resolved host
  and data-directory paths (case and 8.3-safe), so a longer path that merely
  contains the host name cannot match. The uninstaller removes the entry only
  when it still references `{app}`.
- **Data safety**: the uninstaller removes only the application folder and the
  per-user registrations; `%LOCALAPPDATA%\SaveScummer` is never touched.
- **Graceful upgrade**: `[Code] PrepareToInstall` runs the installed
  `SaveScummer.CLI.exe --no-start shutdown` before files are replaced, so an
  accepted operation can finish.
- **Identity**: all three executables already carry version resources; the
  Explorer DLL now has one too (`integrations/windows-explorer/savescummer-explorer.rc.in`).
  The host/CLI build scripts now declare `rerun-if-changed=Cargo.toml`, because
  winresource emits none and a stale resource had shipped
  `SaveScummer_Host.exe` instead of the canonical `SaveScummer.Host.exe`.
- **Signing**: `.iss` has a `SignedBuild`/`SignTool` slot; releases are unsigned
  and documented as such.
- **Publishing policy**: `release-github.ps1` requires the installer by default
  and accepts `-AllowMissingInstaller` for a deliberate portable-only release,
  matching `build.ps1 release`'s warn-and-continue behavior when Inno Setup is
  absent.

New/changed files: `packaging/windows/installer/savescummer.iss`,
`packaging/windows/portable/*.cmd`, `scripts/build-installer.ps1`,
`scripts/setup-innosetup.ps1`, `scripts/package-common.ps1`,
`scripts/package-windows.ps1`, `scripts/release-github.ps1`, `build.ps1`,
`integrations/windows-explorer/{CMakeLists.txt,savescummer-explorer.rc.in}`,
`apps/host/build.rs`, `apps/cli/build.rs`,
`crates/platform/src/{desktop.rs,windows.rs}`, `apps/host/src/lib.rs`,
`apps/host/src/feedback_tests.rs` (adoption tests).

### Phase B acceptance checklist

1. `./scripts/setup-innosetup.ps1` makes `ISCC.exe` available.
2. `./build.ps1 release` builds the Explorer extension, then produces
   `dist/SaveScummer-windows-x64-<ver>.zip` and
   `dist/SaveScummer-windows-x64-<ver>-setup.exe`.
3. Installing the setup to the default location needs no administrator rights and
   places `bin\{SaveScummer,SaveScummer.Host,SaveScummer.CLI}.exe` and
   `bin\savescummer-explorer.dll` under `%LOCALAPPDATA%\Programs\SaveScummer`.
4. With the Explorer task on, right-clicking a configured DIR / eligible copy
   shows Save / Load from the installed extension (under Windows 11 **Show more
   options**); a fresh Explorer process is needed to load the DLL.
5. The sign-in task writes the same value the app's **Launch on startup** toggle
   would, so autostart works right after install; on its first start the host
   adopts that value into the (default-off) preference, and unchecking removes
   the entry.
6. Installing over a running installed host asks it to shut down gracefully and
   succeeds; the previous version is replaced in place (same `AppId`).
7. Uninstalling removes the application folder and the Explorer keys, removes
   the sign-in value only while it still references the installed copy (an entry
   repointed elsewhere is preserved), and leaves `%LOCALAPPDATA%\SaveScummer`
   intact.
8. The portable ZIP still runs standalone and registers the extension only via
   its helper scripts.
9. `./scripts/release-github.ps1` attaches the installer, the portable archive
   and both `.sha256` sidecars to a single draft release; it refuses to publish
   without the installer unless `-AllowMissingInstaller` is passed.
10. Installing over an installation whose Explorer extension is registered
    completes the DLL replacement on the requested restart. A first install with
    the extension task disabled never registers the DLL and needs no restart.

### Known limitations

- The classic `IContextMenu` handler appears under Windows 11 **Show more
  options**; the modern `IExplorerCommand` menu is not implemented.
- Explorer holds a loaded DLL, so an extension update or removal completes after
  an Explorer restart or sign-out (`restartreplace` requests one).
- The Explorer bridge resolves the host at `%LOCALAPPDATA%\SaveScummer`, so the
  installed host must keep the default data directory.
- Releases are unsigned; SmartScreen may warn.

## 7. Phase C — CI + cross-platform (implemented 2026-09-23)

Implemented:

- `.github/workflows/ci.yml` (every push, pull request and manual dispatch):
  - **Rust checks (Windows)** — `scripts/check.ps1`; required.
  - **Qt desktop (Windows)** — `./build.ps1 dev -Test -Generator 'Visual Studio
    17 2022'` (runner images ship VS 2022, not the repository default VS 2019).
  - **Rust checks (ubuntu-latest, macos-latest)** — non-blocking
    `cargo fmt`/`check`/`test`, measuring porting progress before those
    platforms qualify.
- `.github/workflows/release.yml` (`v*` tag + manual dispatch): the Windows job
  runs `./build.ps1 release -Test`, installs Inno Setup with Chocolatey and
  uploads the OS/arch-tagged ZIP and installer; one publish job downloads every
  platform's artifacts into `dist/` and calls `scripts/release-github.ps1`
  (draft-first), so local and CI publishing share one implementation.
  Dispatching with an empty tag builds artifacts without touching a release;
  the manual dispatch button appears once the workflow is on the default branch.
- Release assets are installer-only by default: `scripts/release-github.ps1`
  attaches the installer, with `-IncludePortable` and `-IncludeChecksums`
  opt-ins (the portable archive is still built and kept as a workflow artifact).
  GitHub always adds auto-generated source archives to a release; they cannot be
  disabled or deleted.
- `scripts/setup-qt.ps1` — one Qt bootstrap for Windows, macOS and Linux:
  aqtinstall into a virtualenv under `.runtime/qt-tools/venv`, output in
  `.runtime/Qt/<version>/<kit>`, idempotent. Windows kit directories are
  normalized to the Qt online-installer name (`msvc2019_64`) so the existing
  `build.ps1` default `-QtPrefix` matches. CI caches `.runtime/Qt` with a key
  that hashes the script. Python 3.8+ is required; runners provide it.
- CI-discovered fix: `apps/desktop/compat/msvc-stdext.h` — Qt 6.5 headers use
  MSVC's `stdext` array-iterator helpers, which VS 2022 17.8 deprecated and
  later toolsets (including the runner images' VS 2026) removed. The header is
  force-included for MSVC 19.38+ and documented in `docs/building.md`.

Deviation from the outline: Qt comes from `scripts/setup-qt.ps1` instead of
`jurplel/install-qt-action`, so one multiplatform mechanism serves CI and local
machines and no third-party action is added.

Still open (later platform qualification): macOS/Linux desktop jobs and their
`.tar.gz`/`.dmg` packaging, code signing and notarization secrets
(repo secrets), and the native Linux/macOS adapters themselves.

---

## 8. Risks, pre-flight, and constraints

- **Plan documents:** `PLAN.md` is authoritative for application behavior.
  Change its behavior sections only with explicit user approval, and update
  `PLAN-INFRA.md` and `docs/building.md` together with any tooling change so the
  documents never lag the implementation.
- `Cargo.toml` version is `0.1.0`; tag `v0.1.0` exists and a draft release was
  built from it. Publishing stays a manual decision; to rebuild the draft from a
  different commit, delete and re-push the tag (the publish job updates the
  existing draft with `--clobber`).
- `gh` is installed and authenticated (`gh auth status` verified as `neochief`);
  the release scripts re-check before publishing.
- The Qt SDK is machine-local under `.runtime/Qt`. `scripts/setup-qt.ps1`
  installs it on Windows, macOS and Linux with aqtinstall, and CI calls the same
  script and caches the result; Python 3.8+ is the only new prerequisite.
- Do **not** rename Cargo targets or the canonical executable filenames
  (`SaveScummer.exe`, `SaveScummer.Host.exe`, `SaveScummer.CLI.exe`) — they are
  part of the application contract (`PLAN.md`).
- Keep all scripts pwsh-7 compatible (repo already requires pwsh).
- **Build-entry stance for non-Windows**: `build.ps1` intentionally remains the
  Windows orchestration layer (it already owns process/registry/redist logic).
  macOS/Linux should not be forced through it; their builds run via CI
  (`.github/workflows/ci.yml`), thin per-platform wrappers, or direct
  Cargo/CMake/ctest commands. The **shared
  packaging core (D11)** is the only cross-platform logic that gets genuinely
  reused, so it must be platform-neutral by construction (never call vswhere,
  registry, or Windows-only deploy steps).
- Do **not** add task runners (just/make/cake), move `target/`, or subdivide
  `scripts/` into folders — intentionally out of scope (respect existing
  convention).
- Explorer build scripts (`build-explorer.ps1`, `register-explorer.ps1`,
  `dev-explorer.ps1`) reference `.runtime/qt-tools/cmake` — unaffected by
  Phase A; leave them alone.

## 9. Out of scope for Phase A

- Installer (Phase B), CI workflows (Phase C), code signing, macOS/Linux
  packaging.
- Any change to application behavior, the IPC protocol, catalog, or tests
  beyond the screenshot-path fix (A7).
