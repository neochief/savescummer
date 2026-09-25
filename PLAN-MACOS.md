# macOS

Everything needed to make the host, the CLI, packaging and CI fully work on macOS. The UI is out of scope here (PLAN-UI.md); where it touches this plan, it only needs the host to behave as on Windows.

The target is the one PLAN-BUILD.md already fixes: macOS 13+, Apple Silicon only, one `SaveScummer-macos-arm64-<ver>.dmg`. This plan doesn't change any rule in PLAN-HOST.md: it lists the macOS adapters and fixes, and the few places where macOS forces a decision.

Status (2026-09-25, first build on an M-series Mac): the workspace compiles, the host starts, serves the CLI over its Unix socket, scans, and shuts down cleanly. FTL and Into the Breach (Steam) are found, their macOS save folders resolve, and Save works. After PLAN-HOST-RESTRUCTURE, `cargo xtask check` passes on macOS, with the tests that wait for a macOS adapter ignored and each naming it. Every OS adapter below is still a stub, so the host never sees a game running, has no hotkeys, tray, sounds, notifications or login item, and can't be packaged.


## PRINCIPLES

- **Adapters, not forks.** Each item is a macOS module behind the interface Windows already uses (`ProcessSource`, `integration::start`, `autostart::set`, `sounds::play_now`, `scanner::os`, `xtask::macos`). Shared code changes only where it wrongly assumes Windows. Why: PLAN-HOST's "portable core, thin platform adapters".
- **Public APIs, no private frameworks, no elevated rights.** Nothing asks for admin or for Accessibility unless there is no other way. Why: the app is unsigned and not notarized; every extra permission prompt is a reason to give up on it.
- **Proven on a real Mac.** Each adapter is checked by hand with FTL and Into the Breach, not only with fixtures (PLAN-HOST, "passing on fixtures alone doesn't count").


## PREPARATION

[PLAN-HOST-RESTRUCTURE.md](PLAN-HOST-RESTRUCTURE.md) comes first: the host as the app's entry point, the new executable names, and every OS adapter in its own file (`platform` integration, autostart, sounds and volumes; the monitor's process source; the scanner's `os`; the `ipc` transport; `fsx`; `platform::process`), with the main-loop seam. After it, each section below is mostly new `macos.rs` files next to existing `windows.rs` ones.


## DECISIONS

These change what gets built, so they come first. Decisions 1 and 2 are made; 3 and 4 are open.

1. **Hotkeys (decided 2026-09-25): ⌥F5 runs Save and ⌥F9 runs Load.**
   - F5 and F9 keep the quicksave and quickload habit Mac players know from PC ports. There's no macOS-wide convention for quicksave; ⌘S and ⌘L (what emulators like OpenEmu use) can't be taken globally, since that would break Save in every other app.
   - Why not Ctrl, as on Windows: macOS reserves ⌃F1–⌃F8 for keyboard navigation, and ⌃F5 ("Move focus to the window toolbar") is on by default.
   - Nearby slips are harmless: ⌥F4 does nothing on macOS (quitting is ⌘Q). The one to know is ⌘F5, next to ⌥F5, which turns VoiceOver on; pressing it again turns it off. An accidental Load can be undone with Revert.
   - By default the Mac F-row is brightness, media and dictation keys, so most users press fn+⌥+F5 unless they turned on "Use F1, F2, etc. keys as standard function keys". The README says so.
   - The host's hotkey table becomes per platform, not a constant. Making the keys configurable is a later, separate feature.
2. **The bundle (decided 2026-09-25): one bundle, the host as its main executable.**
   - `SaveScummer.app/Contents/MacOS/` holds `SaveScummer` (the host, `CFBundleExecutable`), `SaveScummer.UI` and `SaveScummer.CLI`, as in PLAN-BUILD.md. No nested helper app.
   - Opening the app starts the host, which shows the UI. Opening it again while the host runs doesn't start a second process: macOS sends "reopen" to the running host, which shows the UI (PLAN-HOST, PROCESSES). The login item runs the host with `--minimized`.
   - The bundle is `LSUIElement`, so the host has no Dock icon. The UI switches itself to a regular app while its window is open, so the Dock icon exists exactly while the window does. Qt has no public API for this; it's a few lines of Objective-C++.
   - The host starts the UI by running `SaveScummer.UI` directly. Asking macOS to open the bundle would reach the host again.
   - Why not a helper app: one bundle means one identity, one signature, one "Open Anyway", one name in notifications and permission prompts. Nested helpers were mostly needed for Apple's old login-item API, which `SMAppService` (macOS 13) replaced.
   - Privacy permissions go to the process macOS holds responsible, and a child inherits it. The host is responsible when a user or launchd starts it. So the CLI starts a missing host through LaunchServices (`open -g -j -a … --args --minimized`), not as its own child; dev hosts with `--data-dir` can keep being run directly, so tooling still reads the ready line.
   - To verify early: two processes using AppKit under one bundle ID behave as described, in particular who receives "reopen" while the UI is also open. Either answer is fine as long as both show the window.
3. **Loading while the game holds a save open.** On Windows, renaming an open file fails, so Load stage 2 refuses safely (E-C1). On macOS the rename always succeeds and the game keeps writing to the renamed `.ssold` file. The Load goes through, and the game may later overwrite the restored save.
   - Recommended: accept this. It's the same "the user makes the game pick up the restored state" rule, and the usual flow (die, main menu, Load) works. Record it in PLAN-HOST (LOAD, and the "Operations" list under TESTING), in PLAN-ERRORS (E-C1 and its test row in section 7: Windows only), and make the e2e test Windows only.
   - The other lock-based cases in PLAN-ERRORS (E-A6, E-B1, E-C2, E-C15, E-D1) still happen on macOS for other reasons (permissions, immutable files, a volume going away), but the tests can't make them with a held file. See TESTS.
   - Option: check open files with `proc_pidfdinfo` before stage 2, and refuse like Windows. This costs a scan of the game's open files on every Load.
4. **Privacy permissions (TCC).** Access to `~/Documents`, `~/Desktop`, removable and network volumes asks the user once per app. An ad-hoc signed app gets a new identity on every build, so each upgrade may ask again.
   - Two catalog games need prompts beyond that, both still to confirm on a Mac. **Six Ages** saves in `~/Library/Group Containers/group.com.a-sharp.Six-Ages`, and recent macOS asks before one app reads another app's containers. **Slay the Spire** keeps its saves inside `SlayTheSpire.app/Contents/Resources/`, and from macOS 13 "App Management" can block changing another app's bundle, which would fail a Load with a permission error. Add both to TEST-REAL-GAMES.md.
   - Recommended: accept it for now and say so in the README. Getting rid of it needs a Developer ID signature, which is a separate decision (cost, yearly renewal).


## PROCESS MONITORING

`crates/monitor`: `system_source()` returns `Unsupported` on everything but Windows, which lists no processes. This is the biggest gap: no game ever shows as running, so the ACTIVE STACK is empty, hotkeys have nothing to target, and start and close markers are never written.

- **Process list:** `proc_listallpids`, then per pid `proc_pidpath` (executable path) and `proc_pidinfo(PROC_PIDTBSDINFO)` (parent pid, start time). A process of another user or a protected one simply has no path (`exe: None`), as on Windows. No extra rights needed.
- **`may_have_changed`:** exits of watched pids are cheap to ask about (`kill(pid, 0)`, or a kqueue `EVFILT_PROC`/`NOTE_EXIT` on each). A new frontmost app counts as a change.
- **Foreground:** the pid of `NSWorkspace.frontmostApplication`. Why not `CGWindowList`: it needs Screen Recording permission on current macOS. `NSWorkspace` likely updates this only while the main run loop runs (to confirm), so the run loop has to run whenever monitoring does, including under `--no-integrations` in the e2e tests, not only for the menu bar and hotkeys.
- **App bundles:** the catalog lists macOS executables as bundles (`FTL.app`), but the process path is `FTL.app/Contents/MacOS/FTL`. Matching must treat a process whose path is inside a listed `.app` as that executable. Known games are already covered by the install-folder rule; custom games with `--exe /Applications/Foo.app` aren't.
- **Case:** `normalize` lowercases on macOS. Correct for the default file system, wrong on a case-sensitive volume; acceptable, but the matching must compare real paths (`/private/var` vs `/var`, see FILE WATCHING).
- The Windows source's CPU budget applies (PLAN-HOST: a full look every `FULL_EVERY` polls). Measure an idle host's CPU on macOS with the `scale` e2e test.

Done when:

1. Starting FTL from Steam puts it on the stack with a start marker; quitting writes a close marker.
2. FTL running before the host starts is on the stack without an invented start.
3. Alt-tabbing (⌘-Tab) between FTL and Into the Breach moves the focused one to the top.
4. The `monitor`, `playing`, `protocol`, `scanning` and `steam` e2e suites pass on macOS.


## HOTKEYS, MENU BAR AND NOTIFICATIONS

`crates/platform/src/integration.rs`: the non-Windows `start` returns "not supported on this platform yet", so there's no tray, no hotkeys and no notifications.

- **A main-thread run loop.** Everything here needs Cocoa's main thread and run loop. Tokio already runs on its own worker threads; the host's main thread only waits for shutdown (`apps/host/src/lib.rs`, `shutdown_rx.recv()`). On macOS that wait moves to a thread and the main thread runs `NSApplication` (activation policy *accessory*) until shutdown, through the `run_main_loop` seam (PREPARATION).
- **Reopen:** the app delegate's reopen handler (and `applicationShouldHandleReopen`) goes to the host's "show the UI" path, the same one the tray and a second launch use.
- **Logout and `launchctl bootout`.** Both end the host with SIGTERM, and AppKit's own quit path at logout ends the process too. The host handles neither today, so PLAN-HOST's safe Exit is skipped and a Load in progress is cut off. Route SIGTERM and `applicationShouldTerminate` to the same shutdown the menu's Exit uses, and answer AppKit only once shutdown has reached its safe point.
- **Cross-platform crates for the tray, menu and hotkeys.** `tray-icon`, `muda` and `global-hotkey` replace the hand-written Windows code and serve macOS from the same code. Do the swap here, not earlier, so Windows and macOS are tested together. What the crates don't cover stays per OS: reopen, notifications, the activation policy.
- **Global hotkeys:** Carbon `RegisterEventHotKey` (what `global-hotkey` uses). It needs no Accessibility or Input Monitoring permission, works while fullscreen games are in front, and a key already taken by another app fails registration, which is reported through `hotkey_errors()` like on Windows. Key repeat must not re-trigger (one operation per press).
- **Menu-bar icon:** `NSStatusItem` with the template image in `assets/macos/` (`SaveScummerTemplate.png` and `@2x`, black on transparent; the SVG there is the editing source, see its README). Build one `NSImage` from both PNGs as representations, mark it `isTemplate`, and set its size to 22 × 22 points, so AppKit tints it for light and dark menu bars and picks the Retina version itself. Embed the PNGs in the host with `include_bytes!`, as the Windows host embeds `icon.ico`, so the icon doesn't depend on the bundle's layout. The host keeps its `NSStatusItem` alive for its whole run. Click shows the UI; the menu has Main window and Exit, as PLAN-HOST says.
- **Notifications:** `UNUserNotificationCenter`. It works only from a bundled app and asks for permission once; ask the first time a failure needs a notification, not at startup.
- **Bindings:** `objc2` and its AppKit/Foundation crates, rather than hand-written `msg_send!`.

Done when:

1. ⌥F5 in fullscreen FTL makes a checkpoint and plays the start and completion cues; ⌥F9 restores it.
2. Holding the key makes one operation.
3. A key taken by another app shows up in the host log and in `hotkey_errors`, and the other key still works.
4. The menu-bar icon is sharp on Retina and non-Retina displays, follows light and dark menu bars, and Exit shuts the host down safely. Adjust the template's optical spacing here if it looks off next to system icons (its README says it hasn't been tried in a live menu bar).
5. A failed Load with the window hidden shows a notification.
6. Logging out while a Load runs lets the Load finish; the host's run ends cleanly in `host-runs`.


## SOUNDS

`crates/platform/src/sounds.rs`: `play_now` does nothing off Windows.

- Play the built-in WAVs from memory with `NSSound(data:)` (or AVFoundation), synchronously on the sound worker, keeping the same gap between cues. No temporary files, no `afplay`.
- No audio device, or a failure, is ignored, as on Windows.

Done when the six cues play for their hotkey outcomes on a Mac, with the "Play sounds" setting honored.


## LAUNCH AT LOGIN

`crates/platform/src/autostart.rs`: `set` returns "not supported yet" and `is_enabled` is always false.

- As PLAN-BUILD.md says: write `~/Library/LaunchAgents/com.savescummer.host.plist` pointing at `Contents/MacOS/SaveScummer` with `--minimized` (and `--data-dir` when given), then `launchctl bootstrap gui/<uid>`; `off` does `launchctl bootout` and removes the file, only if it points at this host.
- `RunAtLoad` true, `KeepAlive` false (a crashed host isn't restarted behind the user's back), `ProcessType` Interactive.
- Dev builds keep refusing, as everywhere.
- Consider `SMAppService.agent` (macOS 13+), which shows the item under the app's name in *Login Items* instead of as an unnamed background item. It needs the plist inside the bundle (`Contents/Library/LaunchAgents/`), which the one-bundle layout allows.

Done when PLAN-BUILD.md macOS "Done when" items 5 and 7 pass: on writes and loads the agent and the host starts at login in the menu bar without a window; off removes it.


## FILE WATCHING

`crates/platform/src/watch.rs` uses `notify` with FSEvents. All three watcher tests fail on macOS: no callback ever arrives.

- Likely cause (to confirm): FSEvents reports real paths (`/private/var/folders/...`), while the filter holds the requested paths (`/var/folders/...`, where `/var` is a symlink). Canonicalize watched paths and filter entries before comparing.
- Watching a single file through its parent (`libraryfolders.vdf`) must work with FSEvents' folder-level events.
- Removable and network volumes: FSEvents holds nothing open, so there's no equivalent of the Windows drive-release code; `volume_of` can stay `None`. A volume mounting under `/Volumes` should trigger a scan so a Steam library on an external disk is picked up.
- PLAN-HOST's rule applies: the first scan that touches a TCC-protected location follows a user action.

Done when the three watcher tests pass, and installing or uninstalling a Steam game on a Mac updates the library without a scan request.


## SCANNER

`crates/scanner/src/lib.rs`, `mod os` for non-Windows:

- **Catalog placeholders:** macOS rows may only use `{HOME}`, `{INSTALL_DIR}` and the Steam placeholders (`{DOCUMENTS}` is Windows-only and `{XDG_*}` Linux-only by design, PLAN-CATALOG 4.4). The Long Dark's macOS row is `{XDG_DATA_HOME}/Hinterland/TheLongDark`, so the game is unsupported on macOS. Fix it through the addendum, and make `catalog --check` flag such rows.
- **Steam account:** today it comes from `loginusers.vdf`, which is only the most recent login, not the running client (the account-switch rules in PLAN-HOST need the running one). macOS Steam has `~/Library/Application Support/Steam/registry.vdf`, but on this Mac it held no `ActiveUser` with Steam closed. Check it with Steam running; if it's there, set the environment's `steam_registry_file` for macOS (`scanner/src/os/unix.rs`) and the Linux reader covers it. Otherwise find where macOS Steam records the running account.
- **GOG Galaxy:** `gog_games` is empty. GOG Galaxy on macOS installs into `/Applications` by default and records installs in `/Users/Shared/GOG.com/Galaxy/Storage/galaxy-2.0.db` (SQLite). Read-only, and skip it if the file is missing or locked.
- **Epic:** manifests are in `~/Library/Application Support/Epic/EpicGamesLauncher/Data/Manifests`; `detect()` only builds the Windows `ProgramData` path.
- **Loose installs:** add `/Applications` and `~/Applications` as standalone roots. There are no uninstall entries on macOS, so `uninstall` detection stays Windows only.

Done when a Mac with Steam, GOG and Epic copies of catalog games finds all of them.


## FILE OPERATIONS

`crates/snapshots/src/fsx.rs` and friends:

- **Ejected disks read as `Missing`.** When a disk under `/Volumes/X` is ejected, macOS removes the mount-point folder, so the lookup fails with a plain "not found" while `/Volumes` stays readable, and `presence` says `Missing`. A Save then records the target as absent and a Steam library on that disk looks uninstalled. Treat a path whose `/Volumes/<name>` part doesn't exist, or isn't a mount point, as `Unknown`. Why: `Missing` can lead to a Load deleting files; `Unknown` never does (PLAN-HOST, "Unsure is not uninstalled").
- **Error numbers** now come from `libc` constants (done in PLAN-HOST-RESTRUCTURE), so a dead SMB share is `Unknown` on macOS too, and a path under a file is `Missing`.
- **File identity isn't stable across remounts.** `identity` is `dev` plus inode, and on macOS `st_dev` depends on the order disks were attached. After a replug or reboot, every checkpoint in a store on an external disk looks replaced (retired and re-registered, losing labels and Revert), a second install gets a new game ID, and an interrupted Load can't verify its files. Use the volume UUID (`getattrlist`, `ATTR_VOL_UUID`) plus inode.
- **Finder's own files.** Browsing the store in Finder writes `.DS_Store` (and `._*` files on exFAT and SMB). Every entry is part of a checkpoint's signature, so one look in Finder retires the checkpoint as edited from outside. Ignore `.DS_Store` and `._*` in signatures, copies and Load's matching, and record it in PLAN-HOST (What a checkpoint holds).
- **`rename_noreplace`:** the Unix version checks and then renames, so a file created in between is replaced. Use `renamex_np(RENAME_EXCL)` on macOS (and `renameat2(RENAME_NOREPLACE)` on Linux).
- **Copies:** consider `clonefile` on APFS for checkpoints: instant and no extra disk space until the game changes the file. This solves PLAN-HOST's "very large saves" known issue on Macs. Optional.
- **Extended attributes and resource forks** aren't copied by `copy_file`. Game saves don't use them; confirm with the macOS catalog games and write it down.
- **`in_use`:** Unix never reports it; see decision 3.

Done when the `snapshots` unit tests pass; a Load on a save folder on an ejected disk image (`hdiutil attach`, then `detach`) is refused as unavailable; a checkpoint store on a disk image survives detach, reattach and reboot with every checkpoint intact; and opening checkpoint folders in Finder changes nothing.


## HOST AND CLI

- **Socket path length.** The endpoint is `<data-dir>/host.sock`, and macOS limits socket paths to 104 bytes. The real data folder is fine, but a long `--data-dir` fails with "path must be shorter than SUN_LEN" (it did during this check, from a temp folder). Fall back to a short, per-user path when the socket path is too long: `$TMPDIR/savescummer-<hash>.sock`, where the hash is the same one Windows uses in its pipe name.
- **Socket access.** Only the signed-in user may connect: create the socket with mode `0600` (or in a `0700` folder) and check the peer's uid (`getpeereid`) on accept.
- **Stale sockets.** `bind` removes whatever is at the path first. That's safe only because `host.lock` is taken before it; keep that order and add a test.
- **Done in PLAN-HOST-RESTRUCTURE:** a host the CLI starts gets its own process group (`platform::process::detach`), and `hard_exit` uses `_exit`. Left for macOS: starting it through LaunchServices (decision 2).
- **Launched from Finder, not Terminal.** A bare executable double-clicked in Finder opens Terminal. Users only ever open the bundle, which runs the host as an app; say so in the README for anyone poking inside it.

Done when a CLI started from a terminal leaves a host that survives closing the terminal, and a 200-character `--data-dir` works.


## TESTS

Rust and e2e tests must pass on macOS without being skipped wholesale. PLAN-HOST-RESTRUCTURE gets `cargo xtask check` green on macOS by ignoring the tests that wait for a macOS adapter, each with its reason; every section here un-ignores its own.

- **Scale test numbers.** `tests/e2e/tests/scale.rs` measures memory and CPU through `ps` off Windows, and reports write counts as 0. Replace with `proc_pid_rusage`, which also has write counts.
- **The e2e world is a Windows world.** `World` writes `"platform": "windows"` and the fake game is `*.exe`. Keep that world (it exercises the rules), and add a macOS world: `platform: macos`, `~/Library/Application Support` saves, the fake game inside a `.app` bundle, so monitoring and matching of bundles are tested.
- **Windows-only behavior.** Tests of uninstall keys and of refusing a Load while a save is open (decision 3) are `#[cfg(windows)]`, with a note pointing here.
- **Locked files without locks.** The fake game's `hold` on macOS is a plain `File::open`, which blocks nothing. For the PLAN-ERRORS cases that need a rename or delete to fail, set `chflags uchg` on the file (both then fail with "operation not permitted"). Use a detached disk image where Windows tests use a VHD, and make E-C4's "Windows blocked…" wording per OS.
- **The fake game as an app.** On macOS it must be a real `.app` that shows a window and activates; otherwise it never becomes the frontmost app, and focus tests can't pass.
- **Timing.** FSEvents batches with a latency; the watcher tests' waits may need to allow for it.
- **Real-game checks.** Add the macOS entries to TEST-REAL-GAMES.md: FTL, Into the Breach, Terraria (`Players`/`Worlds` names), and the Steam Cloud checks from PLAN-HOST on macOS.

Done when `cargo xtask check` passes on an Apple Silicon Mac.


## BUILD AND PACKAGING

`xtask/src/macos.rs`: `fill_package` and `release_file` are `bail!(NOT_YET)`, so `build --package`, `run`, `host start` and `dist` all fail. PLAN-BUILD.md (macOS) already describes what to build; the work is:

- **Build flags:** set `MACOSX_DEPLOYMENT_TARGET` from `pins::MIN_MACOS` for every Rust build.
- **The bundle:** `SaveScummer.app` with the layout from decision 2, `packaging/macos/Info.plist.in` (`CFBundleExecutable` `SaveScummer`, `LSUIElement`), the app icon as `.icns` generated from `assets/icon.svg` (the full-color Dock and Finder icon; the menu-bar template in `assets/macos/` is a separate asset), licenses, manifest and checksums in `Contents/Resources/`.
- **Signing:** ad-hoc `codesign --force --deep --sign -` as the last step, then `codesign --verify --deep --strict`.
- **The DMG:** `hdiutil create -format UDZO` with the app and an `Applications` link.
- **Dev session:** `run` and `host start` start the host from the dev bundle; `procs` stopping must recognize the host inside a bundle.
- **Version resources:** `apps/host/build.rs` and `apps/cli/build.rs` use `winresource`; they must stay no-ops on macOS (they are today) while `Info.plist` carries the version.

Done when PLAN-BUILD.md macOS "Done when" items 1–4 and 6 pass.


## SLEEP AND RESUME

Rust's `Instant` doesn't advance while a Mac sleeps, so the 15-minute scan and any heartbeat or timeout based on it run late after a wake. Listen for `NSWorkspaceDidWakeNotification` and scan on wake (a Steam library disk may have come or gone meanwhile). Add a macOS entry to PLAN-HOST's by-hand "sleep and resume" check.


## CI AND DOCS

- **CI:** add the `macos-latest` job to `ci.yml` (`check`, `build --test --package`) and the macOS build job to `release.yml`. PLAN-BUILD already lists both.
- **docs/building.md:** macOS prerequisites (Xcode command-line tools, rustup), and remove "macOS modules are stubbed".
- **README:** the macOS install section already exists; add the hotkeys and the fn note (decision 1), permission prompts (decision 4), and screenshots for *Open Anyway*.
- **PLAN-HOST.md:** fill in the macOS column of "Per OS" and the PLATFORMS table in PLAN.md.


## ORDER

Each step leaves the host more usable on its own:

1. PREPARATION, and decisions 3 and 4.
2. Small fixes: socket path, ejected disks, file identity, Finder files, `rename_noreplace`, the Long Dark catalog row and the Steam account.
3. Process monitoring. Games show as running, and history gets start and close markers.
4. File watching.
5. Packaging: the bundle, signing, DMG, dev session. Everything after this is tested from a real bundle.
6. The run loop, hotkeys, menu bar, notifications and sounds.
7. Launch at login.
8. GOG and Epic on macOS.
9. CI and docs.

The work is done when every "Done when" above passes on an Apple Silicon Mac with the minimum macOS, and the checks in PLAN-HOST's "By hand" list pass with FTL and Into the Breach.
