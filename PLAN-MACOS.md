# macOS

Everything needed to make the host, the CLI, packaging and CI fully work on macOS. The UI is out of scope here (PLAN-UI.md); where it touches this plan, it only needs the host to behave as on Windows.

The target is the one PLAN-BUILD.md already fixes: macOS 13+, Apple Silicon only, one `SaveScummer-macos-arm64-<ver>.dmg`. This plan doesn't change any rule in PLAN-HOST.md: it lists the macOS adapters and fixes, and the few places where macOS forces a decision.

Status (2026-09-25): every section below has its adapter, and `cargo xtask check` passes on an M-series Mac with nothing ignored for want of one. FTL, Into the Breach and Six Ages (Steam) are found and their macOS save folders resolve; Six Ages waits for access to other apps' data. What's left is by hand with real games (the "Done when" items that name FTL, Into the Breach, Six Ages, logout or System Settings), the UI's part (PLAN-UI), and the findings noted in each section.


## PRINCIPLES

- **Adapters, not forks.** Each item is a macOS module behind the interface Windows already uses (`ProcessSource`, `integration::start`, `autostart::set`, `sounds::play_now`, `scanner::os`, `xtask::macos`). Shared code changes only where it wrongly assumes Windows. Why: PLAN-HOST's "portable core, thin platform adapters".
- **Public APIs, no private frameworks, no elevated rights.** Nothing asks for admin or for Accessibility unless there is no other way. Why: the app is unsigned and not notarized; every extra permission prompt is a reason to give up on it.
- **Proven on a real Mac.** Each adapter is checked by hand with FTL and Into the Breach, not only with fixtures (PLAN-HOST, "passing on fixtures alone doesn't count").


## WHERE THE CODE GOES

Every OS adapter already has its own file, chosen with `#[cfg_attr(..., path = "...")]` in its `mod.rs`: `platform` integration, autostart, sounds and `watch/volumes`; the monitor's process source (`monitor/src/source/`); the scanner's `os` (`scanner/src/os/`); the `ipc` transport; `snapshots` `fsx`; and `platform::process`. Where macOS still falls back to `unsupported.rs`, the work is a new `macos.rs` next to `windows.rs`. The host's main thread goes through `platform::integration::run_main_loop(wait)`, which today just waits; macOS runs its event loop there.


## THE APP BUNDLE

There's one bundle, with the host as its main executable. `SaveScummer.app/Contents/MacOS/` holds `SaveScummer` (the host, `CFBundleExecutable`), `SaveScummer.UI` and `SaveScummer.CLI`, as in PLAN-BUILD.md. There's no nested helper app: one bundle means one identity, one signature, one "Open Anyway", one name in notifications and permission prompts. Nested helpers were mostly needed for Apple's old login-item API, which `SMAppService` (macOS 13) replaced.

- **Opening the app** starts the host, which shows the UI. Opening it again while the host runs doesn't start a second process: macOS sends "reopen" to the running host, which shows the UI (PLAN-HOST, PROCESSES). The login agent runs the host with `--minimized` (LAUNCH AT LOGIN).
- **Dock icon.** The bundle is `LSUIElement`, so the host has no Dock icon. The UI switches itself to a regular app while its window is open, so the Dock icon exists exactly while the window does. Qt has no public API for this; it's a few lines of Objective-C++.
- **Starting the UI.** The host runs `SaveScummer.UI` directly. Asking macOS to open the bundle would reach the host again.
- **Starting the host from the CLI.** Privacy permissions go to the process macOS holds responsible, and a child inherits it. The host is responsible when a user or launchd starts it, so the CLI starts a missing host through LaunchServices (`open -g -j -a … --args --minimized`), not as its own child. Dev hosts with `--data-dir` can keep being run directly, so tooling still reads the ready line.
- **To verify early:** two processes using AppKit under one bundle ID behave as described, in particular who receives "reopen" while the UI is also open. Either answer is fine as long as both show the window.


## SIGNING

Ad-hoc signed, no Developer ID, for now. An ad-hoc signature gives the app a new identity on every build, so macOS forgets every privacy grant on each upgrade and the user allows access again (PRIVACY PERMISSIONS); the README says so. Why: a Developer ID keeps the identity across upgrades and removes *Open Anyway*, but costs a yearly fee that isn't worth it yet. Revisit if re-allowing access after updates becomes a common complaint.


## PRIVACY PERMISSIONS

macOS asks the user before an app reads some locations (TCC). A prompt nobody asked for looks like snooping; a prompt during a fullscreen game may never be seen, and the read that caused it blocks until someone answers, so a Save or Load would hang halfway. The rule: **a prompt only ever follows a user action in the app, and background work only touches locations already granted.**

- **No way to check without asking.** There is no public API that says whether access is granted, and the grants database is itself protected. Reading the location is the test, and that read is what prompts. So the host decides from the path alone, before any access to it, even an existence check.
- **Protected paths**, one table in `platform`, compared with the real path (placeholders filled in, symlinks resolved):

  | Path | Category | macOS pane |
  |---|---|---|
  | `~/Documents`, `~/Desktop`, `~/Downloads` | that folder | Files and Folders |
  | `~/Library/Mobile Documents` | iCloud Drive | Files and Folders |
  | `/Volumes/…` | removable or network volumes | Files and Folders |
  | another app's `~/Library/Containers/…` or `~/Library/Group Containers/…` | app data | App Data (macOS 14+) |
  | inside any `*.app` | app bundles | App Management |

  Anything else, including most macOS saves under `~/Library/Application Support`, is unprotected. The table covers catalog and custom games alike; there is no per-game list.
- **Granted categories.** The host remembers which categories it has read successfully, in its data folder, keyed by its own code signature hash (`SecCodeCopySigningInformation`, `kSecCodeInfoUnique`). A new build means an empty list. A permission error (`EPERM`) on a granted category removes it.
- **Only the host reads save folders.** macOS charges access to the process it holds responsible: a read from the CLI would prompt for Terminal and grant Terminal. The UI and the CLI send paths; the host reads (THE APP BUNDLE covers how the host itself is started).
- **What may prompt.** Only these, all user actions: first run, Scan games, `AddGame` and `Configure` with a new save location, and a new `RequestAccess { game }` command behind the UI's *Allow access*. Each reads the location right away. The probe lists the folder; for app bundles it also creates and removes a temporary file, since App Management only guards writes. A success adds the category and activates every waiting game in it. An immediate `EPERM` means the user denied it before, and macOS won't ask again: the reply says *denied*, and the UI offers to open the pane from the table (`x-apple.systempreferences:com.apple.preference.security?Privacy_…`).
- **Inactive games.** A game whose save path is in a category that isn't granted is listed but inactive: no watching, no scan reads, not on the ACTIVE STACK, no markers. The protocol reports it with its category. The UI shows *Allow access* in place of the game's controls (PLAN-UI).
- **Found in the background.** When a background scan finds an inactive game, or a category is revoked or cleared by an upgrade, the host sends one notification per category ("SaveScummer needs access to Documents for 2 games"), not one per game and not again for the same category in the same run. Clicking it opens that game in the UI. If notifications are off, the UI still shows the state.
- **Hotkeys never prompt.** If the frontmost app is an inactive game, ⌥F5 and ⌥F9 play the failure cue and repeat the notification; they never fall through to another game on the stack. For an active game, every path the operation touches is checked against the table before the first read; one that isn't granted fails the operation before anything changes.
- **Safety net.** A protected location the table misses would block a read on the prompt. File operations for Save and Load run under a timeout: past it, the operation is reported failed while the blocked thread is left to finish, and the game's operation lock stays held until it does, so no second Load runs over a half-finished one.
- **Steam libraries on external disks** are in the removable volumes category. Until it's granted, the scanner skips them and their games are inactive; the mount trigger in FILE WATCHING only scans a volume once the category is granted.
- **Tests.** The table is part of the environment, so e2e tests mark a fixture folder as protected and use a fake probe that answers granted, denied or hangs.
- **To verify on a Mac:** whether an FSEvents stream on a protected folder prompts, or silently gets no events; whether `stat` of a path inside a protected folder prompts; whether picking a folder in the UI's open panel grants the host anything (not relied on); what denial returns for each category. Add **Six Ages** (group container) and **Slay the Spire** (inside its `.app`) to TEST-REAL-GAMES.md.
- **Confirmed (macOS 15.7, 2026-09-25):** Six Ages' container is protected even though the Steam build is unsigned and the folder has no container-manager metadata: listing, reading and writing it each raise a `kTCCServiceSystemPolicyAppData` request. The unified log (`/usr/bin/log stream --predicate 'subsystem == "com.apple.TCC"'`; in zsh `log` is a builtin) shows each request's service, the responsible app (`AUTHREQ_ATTRIBUTION … responsible_path=`) and the answer (`AUTHREQ_RESULT … authValue=2` allowed, `0` denied), which is what the macOS permission tests build on.

Done when:

1. A fixture game added in the background with a protected save path shows as inactive, a notification names its category, and nothing prompts.
2. *Allow access* prompts once; after allowing it, every game in that category becomes active. After denying it, the UI offers the right System Settings pane.
3. Adding a custom game in `~/Documents` prompts while the form is open, and the prompt names SaveScummer, not Terminal, also when added from the CLI.
4. ⌥F9 in front of an inactive game plays the failure cue, notifies, and changes no files.
5. Installing a new build drops the grants; after the next start the host sends one notification per category and doesn't prompt.
6. Revoking the grant in System Settings while the host runs makes the next scan mark the games inactive, and it doesn't crash any operation.


## PROCESS MONITORING

`crates/monitor`: `system_source()` returns `Unsupported` on everything but Windows, which lists no processes. This is the biggest gap: no game ever shows as running, so the ACTIVE STACK is empty, hotkeys have nothing to target, and start and close markers are never written.

- **Process list:** `proc_listallpids`, then per pid `proc_pidpath` (executable path) and `proc_pidinfo(PROC_PIDTBSDINFO)` (parent pid, start time). A process of another user or a protected one simply has no path (`exe: None`), as on Windows. No extra rights needed.
- **`may_have_changed`:** exits of watched pids are cheap to ask about (`kill(pid, 0)`, or a kqueue `EVFILT_PROC`/`NOTE_EXIT` on each). A new frontmost app counts as a change.
- **Foreground:** the pid of `NSWorkspace.frontmostApplication`. Why not `CGWindowList`: it needs Screen Recording permission on current macOS. Confirmed: without the main run loop it never changes, and it starts tracking once someone observes app activations, so the run loop runs whenever monitoring does, including under `--no-integrations` in the e2e tests, not only for the menu bar and hotkeys. The source observes `NSWorkspaceDidActivateApplicationNotification`, which also counts as a change.
- **Re-parented children:** when a launcher exits, macOS hands its child to launchd (parent 1), where Windows keeps the original parent. The source remembers each process's first parent (by pid and start time), and a new child of a running game (`proc_listchildpids`) triggers a full look at the next poll, so the child is seen while its parent is still known.
- **App bundles:** the catalog lists macOS executables as bundles (`FTL.app`), but the process path is `FTL.app/Contents/MacOS/FTL`. Matching must treat a process whose path is inside a listed `.app` as that executable. Known games are already covered by the install-folder rule; custom games with `--exe /Applications/Foo.app` aren't.
- **Case:** `normalize` lowercases on macOS. Correct for the default file system, wrong on a case-sensitive volume; acceptable, but the matching must compare real paths (`/private/var` vs `/var`, see FILE WATCHING).
- The Windows source's CPU budget applies (PLAN-HOST: a full look every `FULL_EVERY` polls). Measure an idle host's CPU on macOS with the `scale` e2e test.

Done when:

1. Starting FTL from Steam puts it on the stack with a start marker; quitting writes a close marker.
2. FTL running before the host starts is on the stack without an invented start.
3. Alt-tabbing (⌘-Tab) between FTL and Into the Breach moves the focused one to the top.
4. The `monitor`, `playing`, `protocol`, `scanning` and `steam` e2e suites pass on macOS.


## HOTKEYS, MENU BAR AND NOTIFICATIONS

`crates/platform/src/integration/`: the non-Windows `start` returns "not supported on this platform yet", so there's no tray, no hotkeys and no notifications.

- **A main-thread run loop.** Everything here needs Cocoa's main thread and run loop. Tokio already runs on its own worker threads; the host's main thread only waits for shutdown through `run_main_loop` (WHERE THE CODE GOES). On macOS that wait moves to a thread and the main thread runs `NSApplication` (activation policy *accessory*) until shutdown.
- **Reopen:** the app delegate's reopen handler (and `applicationShouldHandleReopen`) goes to the host's "show the UI" path, the same one the tray and a second launch use.
- **Logout and `launchctl bootout`.** Both end the host with SIGTERM, and AppKit's own quit path at logout ends the process too. The host handles neither today, so PLAN-HOST's safe Exit is skipped and a Load in progress is cut off. Route SIGTERM and `applicationShouldTerminate` to the same shutdown the menu's Exit uses, and answer AppKit only once shutdown has reached its safe point.
- **Cross-platform crates for the tray, menu and hotkeys.** `tray-icon`, `muda` and `global-hotkey` replace the hand-written Windows code and serve macOS from the same code. Do the swap here, not earlier, so Windows and macOS are tested together. What the crates don't cover stays per OS: reopen, notifications, the activation policy.
  - Done so far: macOS uses `global-hotkey`; its menu-bar item is `objc2` AppKit code, because `tray-icon` takes one bitmap where the template needs both 1x and 2x representations. Windows keeps its hand-written code until the swap can be tested on a Windows machine; Windows notifications are tray balloons tied to its own icon, which `tray-icon` doesn't expose.
- **The keys: ⌥F5 runs Save and ⌥F9 runs Load.** F5 and F9 keep the quicksave and quickload habit Mac players know from PC ports. There's no macOS-wide convention for quicksave; ⌘S and ⌘L (what emulators like OpenEmu use) can't be taken globally, since that would break Save in every other app. Ctrl, as on Windows, is out: macOS reserves ⌃F1–⌃F8 for keyboard navigation, and ⌃F5 ("Move focus to the window toolbar") is on by default.
  - Nearby slips are harmless: ⌥F4 does nothing on macOS (quitting is ⌘Q). The one to know is ⌘F5, next to ⌥F5, which turns VoiceOver on; pressing it again turns it off. An accidental Load can be undone with Revert.
  - By default the Mac F-row is brightness, media and dictation keys, so most users press fn+⌥+F5 unless they turned on "Use F1, F2, etc. keys as standard function keys". The README says so.
  - The host's hotkey table becomes per platform, not a constant. Making the keys configurable is a later, separate feature.
- **Registering them:** Carbon `RegisterEventHotKey` (what `global-hotkey` uses). It needs no Accessibility or Input Monitoring permission, works while fullscreen games are in front, and a key already taken by another app fails registration, which is reported through `hotkey_errors()` like on Windows. One operation per press: Carbon hotkeys never auto-repeat while held, so presses need no filtering, and nothing waits for a key-up macOS may lose (the screen locking while the key is down).
- **Menu-bar icon:** `NSStatusItem` with the template image in `assets/macos/` (`SaveScummerTemplate.png` and `@2x`, black on transparent; the SVG there is the editing source, see its README). Build one `NSImage` from both PNGs as representations, mark it `isTemplate`, and set its size to 22 × 22 points, so AppKit tints it for light and dark menu bars and picks the Retina version itself. Embed the PNGs in the host with `include_bytes!`, as the Windows host embeds `icon.ico`, so the icon doesn't depend on the bundle's layout. The host keeps its `NSStatusItem` alive for its whole run. Click shows the UI; the menu has Main window and Exit, as PLAN-HOST says.
- **Notifications:** `UNUserNotificationCenter`. It works only from a bundled app and asks for permission once; ask the first time a failure or a missing privacy grant needs a notification, not at startup.
- **Inactive games:** a hotkey in front of a game waiting for privacy access fails with the cue and never targets another game (PRIVACY PERMISSIONS).
- **Bindings:** `objc2` and its AppKit/Foundation crates, rather than hand-written `msg_send!`.

Done when:

1. ⌥F5 in fullscreen FTL makes a checkpoint and plays the start and completion cues; ⌥F9 restores it.
2. Holding the key makes one operation.
3. A key taken by another app shows up in the host log and in `hotkey_errors`, and the other key still works.
4. The menu-bar icon is sharp on Retina and non-Retina displays, follows light and dark menu bars, and Exit shuts the host down safely. Adjust the template's optical spacing here if it looks off next to system icons (its README says it hasn't been tried in a live menu bar).
5. A failed Load with the window hidden shows a notification.
6. Logging out while a Load runs lets the Load finish; the host's run ends cleanly in `host-runs`.


## SOUNDS

`crates/platform/src/sounds/`: `play_now` does nothing off Windows.

- Play the built-in WAVs from memory with `NSSound(data:)` (or AVFoundation), synchronously on the sound worker, keeping the same gap between cues. No temporary files, no `afplay`. Off the main thread `isPlaying` stays set past the end, so the wait is bounded by the sound's length plus a little output latency.
- No audio device, or a failure, is ignored, as on Windows.

Done when the six cues play for their hotkey outcomes on a Mac, with the "Play sounds" setting honored.


## LAUNCH AT LOGIN

`crates/platform/src/autostart/`: `set` returns "not supported yet" and `is_enabled` is always false.

- **`SMAppService.agent` (macOS 13+),** not a plist written into `~/Library/LaunchAgents`. Why: macOS lists the item under the app's name and icon in *Login Items* (the *Allow in the Background* list, not *Open at Login*: that one is for apps that open a window), the user can turn it off there like any other app, and there's no file outside the bundle to go stale or point at an old copy.
- **The agent's plist ships inside the bundle:** `Contents/Library/LaunchAgents/com.savescummer.SaveScummer.host.plist`, with `BundleProgram` `Contents/MacOS/SaveScummer`, `ProgramArguments` ending in `--minimized`, `RunAtLoad` true, `KeepAlive` false (a crashed host isn't restarted behind the user's back) and `ProcessType` Interactive.
- **`on`** calls `register()`, **`off`** calls `unregister()`; `is_enabled` reads `status`. The registration belongs to this bundle, so `off` can't remove another copy's entry.
- **Turned off in System Settings:** `status` becomes *requires approval*. The host reports launch at login as off, and the UI's checkbox offers to open the pane (`SMAppService.openSystemSettingsLoginItems()`), since the app can't turn it back on by itself.
- **No custom data folder.** The plist is fixed at build time, so it can't carry `--data-dir`: `--autostart on` with a `--data-dir` other than the default refuses on macOS with a clear message. Dev hosts keep refusing, as everywhere.
- **To verify first:** that `register()` works from the ad-hoc signed bundle (SIGNING), and that the registration survives dragging a new build over the old app. If either fails, fall back to writing the plist into `~/Library/LaunchAgents` and loading it with `launchctl bootstrap gui/<uid>`.

Done when PLAN-BUILD.md macOS "Done when" items 5 and 7 pass: turning it on makes the host start at login in the menu bar without a window, and it shows as SaveScummer in *Login Items → Allow in the Background* (confirmed 2026-09-26 on macOS 15); turning it off, from the app or from System Settings, stops it.


## FILE WATCHING

`crates/platform/src/watch/` uses `notify` with FSEvents. All three watcher tests fail on macOS (ignored there for now): no callback ever arrives.

- Confirmed cause: FSEvents reports real paths (`/private/var/folders/...`), while the filter held the requested paths (`/var/folders/...`, where `/var` is a symlink). Watched paths and filter entries are canonicalized now. FSEvents can also report a folder's own creation just after the watch starts; the tests let that pass first.
- Watching a single file through its parent (`libraryfolders.vdf`) must work with FSEvents' folder-level events.
- Removable and network volumes: FSEvents holds nothing open, so there's no equivalent of the Windows drive-release code; `volume_of` can stay `None`. A volume mounting under `/Volumes` should trigger a scan so a Steam library on an external disk is picked up.
- Protected locations are never watched or scanned before their category is granted (PRIVACY PERMISSIONS).

Done when the three watcher tests pass, and installing or uninstalling a Steam game on a Mac updates the library without a scan request.


## SCANNER

`crates/scanner/src/os/unix.rs`, shared with Linux:

- **Catalog placeholders:** macOS rows may only use `{HOME}`, `{INSTALL_DIR}` and the Steam placeholders (`{DOCUMENTS}` is Windows-only and `{XDG_*}` Linux-only by design, PLAN-CATALOG 4.4). The Long Dark's macOS row is `{XDG_DATA_HOME}/Hinterland/TheLongDark`, so the game is unsupported on macOS. Fix it through the addendum, and make `catalog --check` flag such rows.
- **Steam account:** today it comes from `loginusers.vdf`, which is only the most recent login, not the running client (the account-switch rules in PLAN-HOST need the running one). macOS Steam has `~/Library/Application Support/Steam/registry.vdf`, but it holds no `ActiveUser`, even with Steam running and logged in (checked 2026-09-25): only `SteamPID` and settings such as `AutoLoginUser`. The running account does show in `Steam/logs/connection_log.txt` (`[Logged On …] [U:1:<account>]`); reading it is still open, so `loginusers.vdf` stays the source.
- **GOG Galaxy:** `gog_games` is empty. GOG Galaxy on macOS installs into `/Applications` by default and records installs in `/Users/Shared/GOG.com/Galaxy/Storage/galaxy-2.0.db` (SQLite). Read-only, and skip it if the file is missing or locked.
- **Epic:** manifests are in `~/Library/Application Support/Epic/EpicGamesLauncher/Data/Manifests`; `detect()` only builds the Windows `ProgramData` path.
- **Loose installs:** add `/Applications` and `~/Applications` as standalone roots. There are no uninstall entries on macOS, so `uninstall` detection stays Windows only.

Done when a Mac with Steam, GOG and Epic copies of catalog games finds all of them.


## FILE OPERATIONS

`crates/snapshots/src/fsx/` and friends:

- **Disconnected drives, checkpoint identity, exclusive renames and file managers' files:** decided in PLAN-HOST ("A drive the host has seen stays expected", Backups changed outside the app, LOAD stage renames, What a checkpoint holds) and PLAN-ERRORS (E-N3, E-C14). The macOS parts: remembered mount points under `/Volumes` or anywhere else, identity by signature only, `renamex_np(RENAME_EXCL)`, and `.DS_Store`.
- **Copies:** consider `clonefile` on APFS for checkpoints: instant and no extra disk space until the game changes the file. This solves PLAN-HOST's "very large saves" known issue on Macs. Optional.
- **Extended attributes and resource forks** aren't copied by `copy_file`. Game saves don't use them. Checked 2026-09-25 on FTL and Into the Breach: their save files carry only `com.apple.provenance`, which macOS adds on its own, and no resource forks.
- **`in_use`:** an open file blocks neither rename nor delete on macOS, so Unix never reports it and nothing is retried there (PLAN-HOST, *Interference from the running game*). Open saves are caught by the open-file check instead (OPEN FILE CHECK).

Done when the `snapshots` unit tests pass; a Load on a save folder on an ejected disk image (`hdiutil attach`, then `detach`) is refused as unavailable; a checkpoint store on a disk image survives detach, reattach and reboot with every checkpoint intact; and opening checkpoint folders in Finder changes nothing.


## OPEN FILE CHECK

Decided in PLAN-HOST (LOAD, *Interference from the running game*) and PLAN-ERRORS (E-C16, E-X5). Why it's needed: on Windows a handle opened without delete sharing makes Load stage 2 fail; on macOS the rename always succeeds, the game keeps its handle on the renamed `.ssold` file, and it may later write over the restored save.

- **Where:** a platform adapter next to process monitoring (`crates/monitor/src/open_files/`), asked by the Load just before stage 2 with the game's pids and the device and inode of every file stage 2 will rename. It answers *found* (which files, which pids), *none* (every pid inspected), or *incomplete* (with the pids and reasons it couldn't inspect), and *found* wins over *incomplete*.
- **How:** per pid, `proc_pidinfo(PROC_PIDLISTFDS)` (grow the buffer and repeat if it was full), then `proc_pidfdinfo(PROC_PIDFDVNODEINFO)` for each `PROX_FDTYPE_VNODE` descriptor, comparing `vst_dev` and `vst_ino`. A pid that exited meanwhile holds nothing. A descriptor closed between the two calls is skipped.
- **Limits, acknowledged:** `libproc` has been around since early Mac OS X and is what `lsof` and Activity Monitor use, but Apple documents it as private and subject to change. It sees only the user's own processes; protected or hardened processes can refuse (`EPERM`), which is *incomplete*, never a crash and never a refusal of the Load. Memory-mapped files without an open descriptor aren't seen.
- **Cost:** one pass over the game's descriptors per check, a few per Load at most; measure it on a game with many open files.

Done when a Load with the fake game holding an affected save refuses before stage 2 (no `.ssold` created), a hold released within the budget lets the Load through, an uninspectable pid (fake inspector) proceeds and logs, and another game can Save while one waits.


## HOST AND CLI

- **Socket path length:** a data folder too long for a socket is refused at start with a clear message (PLAN-HOST, PROTOCOL).
- **Socket access.** Only the signed-in user may connect: create the socket with mode `0600` (or in a `0700` folder) and check the peer's uid (`getpeereid`) on accept.
- **Stale sockets.** `bind` removes whatever is at the path first. That's safe only because `host.lock` is taken before it; keep that order and add a test.
- **Starting the host from the CLI:** it already gets its own process group (`platform::process::detach`). Left for macOS: starting it through LaunchServices (THE APP BUNDLE).
- **Launched from Finder, not Terminal.** A bare executable double-clicked in Finder opens Terminal. Users only ever open the bundle, which runs the host as an app; say so in the README for anyone poking inside it.

Done when a CLI started from a terminal leaves a host that survives closing the terminal, and a 200-character `--data-dir` works.


## TESTS

Rust and e2e tests must pass on macOS without being skipped wholesale. Today `cargo xtask check` is green on macOS because the tests that wait for a macOS adapter are ignored there, each naming its section of this plan; every section un-ignores its own.

- **Scale test numbers.** `tests/e2e/tests/scale.rs` measures memory and CPU through `ps` off Windows, and reports write counts as 0. Replace with `proc_pid_rusage`, which also has write counts.
- **The e2e world is a Windows world.** `World` writes `"platform": "windows"` and the fake game is `*.exe`. Keep that world (it exercises the rules), and add a macOS world: `platform: macos`, `~/Library/Application Support` saves, the fake game inside a `.app` bundle, so monitoring and matching of bundles are tested.
- **Per-OS behavior.** Tests of uninstall keys and of stage 2 failing on a held save (E-C1) are `#[cfg(windows)]`; the open-file refusal before stage 2 (E-C16) runs on macOS (and Linux). Each has a note pointing here.
- **Locked files without locks.** The fake game's `hold` on macOS is a plain `File::open`, which blocks no rename or delete but is exactly what the open-file check finds. For the other PLAN-ERRORS cases that need a rename or delete to fail, set `chflags uchg` on the file (both then fail with "operation not permitted"). Use a detached disk image where Windows tests use a VHD, and make E-C4's "Windows blocked…" wording per OS.
- **The fake game as an app.** On macOS it must be a real `.app` that shows a window and activates; otherwise it never becomes the frontmost app, and focus tests can't pass.
- **Timing.** FSEvents batches with a latency; the watcher tests' waits may need to allow for it.
- **Real-game checks.** Add the macOS entries to TEST-REAL-GAMES.md: FTL, Into the Breach, Terraria (`Players`/`Worlds` names), and the Steam Cloud checks from PLAN-HOST on macOS.

Done when `cargo xtask check` passes on an Apple Silicon Mac.


## BUILD AND PACKAGING

`xtask/src/macos.rs`: `fill_package` and `release_file` are `bail!(NOT_YET)`, so `build --package`, `run`, `host start` and `dist` all fail. PLAN-BUILD.md (macOS) already describes what to build; the work is:

- **Build flags:** set `MACOSX_DEPLOYMENT_TARGET` from `pins::MIN_MACOS` for every Rust build.
- **The bundle:** `SaveScummer.app` with the layout from THE APP BUNDLE, `packaging/macos/Info.plist.in` (`CFBundleExecutable` `SaveScummer`, `LSUIElement`), the login agent's plist (LAUNCH AT LOGIN), the app icon as `.icns` generated from `assets/icon.svg` (the full-color Dock and Finder icon; the menu-bar template in `assets/macos/` is a separate asset), licenses, manifest and checksums in `Contents/Resources/`.
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
- **README:** the macOS install section already exists; add the hotkeys and the fn note (HOTKEYS), permission prompts and re-allowing access after each update (PRIVACY PERMISSIONS), and screenshots for *Open Anyway*.
- **PLAN-HOST.md:** fill in the macOS column of "Per OS" and the PLATFORMS table in PLAN.md.


## ORDER

Each step leaves the host more usable on its own:

1. Small fixes: socket path, ejected disks, file identity, Finder files, `rename_noreplace`, the Long Dark catalog row and the Steam account.
2. Process monitoring. Games show as running, and history gets start and close markers. Then the open-file check, which needs the game's pids.
3. File watching.
4. Packaging: the bundle, signing, DMG, dev session. Everything after this is tested from a real bundle.
5. The run loop, hotkeys, menu bar, notifications and sounds.
6. Privacy permissions. The table, inactive games and the probe can be built and tested with fixtures earlier; the real prompts need the bundle and notifications.
7. Launch at login.
8. GOG and Epic on macOS.
9. CI and docs.

The work is done when every "Done when" above passes on an Apple Silicon Mac with the minimum macOS, and the checks in PLAN-HOST's "By hand" list pass with FTL and Into the Breach.
