# First-launch setup and permissions

**Status:** implemented (`apps/host/src/onboarding.rs` coordinates it). The ASCII screens below define the agreed UI; the conversation sketches are illustrative. Native checks in section 8 are still to be done on macOS, Windows and Wayland desktops.

## 1. Purpose and scope

On SaveScummer's first interactive launch, perform setup that needs no user interaction silently. Show one small screen only if the user needs to explicitly request an OS permission or open the OS's approval settings.

This is a first-launch feature, not a recurring readiness screen.

### Product decisions

- Configure autostart silently when there is no previous choice or existing configuration to preserve.
- Respect `--autostart on|off`, installer choices, Settings choices, and externally disabled startup entries. The existing flag is `--autostart`, not `--autolaunch`.
- Preserve configured or cleared shortcuts and existing OS grants/denials. Do not reset permissions or restore default bindings as part of setup.
- Register shortcuts silently where possible. Defer any registration that may open an approval/configuration dialog until an explicit action.
- Request macOS file access only for protected locations needed by games detected on this machine.
- With no detected games, offer game access only when a known game library or discovery location is protected. A scan still in progress is not an empty library.
- Show only applicable permission rows. If there are none, go directly to the normal app.
- Initially label the exit button **Skip**. Change it to **Continue** after at least one requested permission is confirmed as granted. It does not require every row to succeed.
- Opening a dialog or System Settings does not count as approval. Denial, cancellation, timeout, and failure do not count either.
- Both Skip and Continue finish onboarding permanently for this app-data profile. Closing the onboarding window also dismisses it.
- Keep permission acquisition separate from onboarding completion. Finishing onboarding does not grant access or imply every feature works.

### Out of scope

No tutorial, GIF, game-play instructions, onboarding carousel, autostart preference checkbox, OS-notification setup, recurring onboarding, or general installation/repair wizard. No automatic package installation, keyboard drivers, tray extensions, Accessibility request, Automation request, or blanket Full Disk Access request.

Notification removal is separate work. Existing Settings and per-game access remedies remain available after onboarding; a later permission problem never reopens this screen.

## 2. What exists today

The implementation should reuse the existing platform operations and change their orchestration.

| Area | Current implementation | Needed change |
| --- | --- | --- |
| Launch flag | `apps/host/src/options.rs` and `lib.rs`: `--autostart on\|off` writes/removes the OS entry and exits before a normal host run. | Persist the choice without consuming first launch. |
| Settings choice | `queries::settings` writes `SETTING_LAUNCH` (`launch_on_startup`) to the settings table. | Use the same durable choice for flag, Settings, and first-launch decisions. |
| Actual startup state | `queries::refresh_launch` reads OS registration and macOS approval status. | Keep actual state separate from saved intent; expose it to onboarding. |
| macOS startup prompts | `privacy::first_run_asks` and the end of host startup can call `privacy::after_scan(..., true)` automatically. | Remove this automatic first-run permission request; the screen's action must initiate it. |
| macOS access | `privacy::request_access`, platform privacy probes, and `GuidancePanel` already support requesting access and opening settings. | Reuse the access handling for a grouped setup action; report verified results. |
| Linux shortcuts | `integration::linux::start` currently starts portal binding immediately, with an X11 fallback. | Separate inspection/silent initialization from potentially prompting binding. Publish live results. |
| Linux app entry | `menu_entry::keep_installed` creates/updates our AppImage desktop entry and icon. | Keep this silent and preserve entries owned by a package or another tool. |
| Integration errors | Host integration startup primarily logs shortcut errors. | Publish structured capability/result state rather than deriving UI state from log text. |

Relevant architecture remains unchanged: the host owns behavior and persistence; the UI displays state and sends commands. See [PLAN-HOST.md](PLAN-HOST.md) and [PLAN-UI.md](PLAN-UI.md).

## 3. Define first launch explicitly

### Eligibility

First launch means the first deliberate opening of the normal app window for a new production app-data profile. It is not the first process invocation or the first game launch.

| Invocation or event | Starts/consumes onboarding? |
| --- | --- |
| User launches the installed app and opens its normal window | Yes, if this profile is still pending. |
| User opens the main window of an already-running background host | Yes, if pending. |
| Installer invokes `--autostart on` or `--autostart off` | No. Record the choice and exit. |
| Host starts with `--minimized`, including login startup | No. Leave pending until the user opens the main window. |
| CLI starts/uses a host without showing its window | No. |
| `--help`, `--version`, or argument failure | No. |
| Application update, changed code signature, new game, revoked permission | No new onboarding. |
| Existing profile upgraded from a version without onboarding | No. Preserve existing behavior and configuration. |
| Development build, `--demo`, or `--no-integrations` | No real first-launch OS setup. UI fixtures may simulate the screen. |

Production first-launch OS setup uses the normal platform data directory. An alternate `--data-dir` must not create or alter real startup/desktop entries as a side effect of onboarding. Tests use disposable profiles and fake platform adapters.

### Durable lifecycle

Use the existing host settings store for a dedicated first-launch state, independent of `privacy.json` and all granted-permission caches. Proposed values:

```text
first_launch_state = pending | started | finished
launch_on_startup   = absent | "1" | "0"       # existing settings key
```

- `pending`: no interactive first-launch session has begun.
- `started`: the first-launch opportunity has been consumed; this host session may present and operate its screen.
- `finished`: setup was skipped, continued, closed, or needed no screen.

The host atomically changes `pending` to `started` before first-launch side effects. Keep the active session identity in memory. Only that session can present the screen.

`started` left by a crashed/terminated host is treated as consumed on the next run and normalized to `finished`. Do not replay onboarding or the silent autostart default after a crash. This deliberately favors at-most-once behavior. A transient UI connection loss may resume the same live window/session; a closed window or a later host run must not create a new session.

If the initial marker cannot be persisted, do not perform one-time OS setup or open a prompting flow. Keep the ordinary app available where possible and report the storage failure through existing error handling. On finish, retain the in-memory consumed state even if the final persistence write fails; the already-persisted `started` marker still prevents a later replay.

Initialize this state before adding the current `host_runs` record. For migration, an old profile with prior host runs or existing game/checkpoint history is an existing installation: mark it finished without setup. A profile containing only installer-created settings, including `launch_on_startup`, is still eligible. A new background-only profile must get an explicit pending marker, so its later host-run records do not incorrectly turn it into an existing installation.

Do not infer first launch from an empty game list, missing privacy grants, an absent startup entry, the current app version, or a changed executable path.

### First-launch control flow

```text
normal host startup
    |
    +-- flag-only invocation ----------------> record choice; exit
    |
    +-- initialize/migrate first-launch state
    |
    +-- start ordinary non-prompting services
    |
    +-- no deliberate window opening --------> remain pending; no onboarding
    |
    +-- already consumed --------------------> normal app
    |
    `-- pending + deliberate window opening
            |
            +-- persist started; create one active session
            +-- run silent setup, preserving prior choices
            +-- finish a non-prompting discovery/capability pass
            +-- compute applicable permission rows
            |
            +-- no rows ---------------------> persist finished; normal app
            |
            `-- rows exist ------------------> permission screen
                                                   |
                                                   +-- explicit row action
                                                   |       -> OS interaction
                                                   |       -> verify result
                                                   |       -> update row/button
                                                   |
                                                   `-- Skip / Continue / close
                                                           -> persist finished
                                                           -> normal app / close
```

The normal app may show its ordinary loading state while inspection is incomplete. Do not flash a permission screen before its rows are known, or treat a timed-out capability check as verified success. An inspection failure is not itself a permission request; finish onboarding with the existing failure/remedy path where appropriate.

## 4. Autostart: silent default with preserved intent

### Shared choice storage

Use `SETTING_LAUNCH` as a tri-state choice: absent means no recorded choice, `1` means on, and `0` means off. Do not introduce a competing UI-local startup preference.

Both the flag and Settings must durably record the explicit requested choice. In particular, `--autostart off` must leave a record even when there is no OS entry to remove. An absent entry alone cannot distinguish never configured from explicitly disabled.

Record explicit intent before applying the OS change; report storage/OS failures rather than claiming success. A failed OS change must not let first-launch defaults subsequently overwrite an explicit off request. Actual availability continues to come from the OS, not from this setting.

The flag must still configure startup and exit without a window, a game scan, shortcut binding, privacy probing, or a `host_runs` entry. If the host is already running, serialize the flag's change through that host; otherwise use the shared storage/platform path without launching the normal host. Avoid a race in which first-launch defaults enable startup after a concurrent explicit off command.

### Decision table

| State at first-launch setup | Behavior |
| --- | --- |
| Recorded on or off choice exists | Preserve it and the current OS configuration. Do not rerun the default setter. |
| Our startup entry is already enabled | Preserve it; do not rewrite it merely for onboarding. |
| Our startup entry is externally disabled | Preserve disabled state; do not silently re-enable it. |
| Existing entry belongs to another copy/tool | Do not overwrite it or claim it is ours. |
| No recorded choice and no existing configuration | Attempt the default on registration once, silently where supported. |
| Current build/location does not support reliable startup | Do not register an unusable target; use normal Settings/error handling. |

Read the latest persisted choice immediately before deciding to apply the default. The first-launch default is never a recurring reconciliation task. After setup, a user disabling autostart in the app or OS must stay disabled.

Platform inspection needs more information than a single `is_enabled` boolean: distinguish absent, enabled, disabled/awaiting approval, foreign ownership, and unavailable. For macOS, do not infer that an existing `RequiresApproval` state means the user wants startup enabled. Offer approval only when this setup just requested registration or a recorded on choice establishes that intent; an explicit off choice suppresses the row.

### Per-OS action

- **Windows:** reuse the per-user HKCU Run entry. No administrator access or permission screen is needed. Preserve the installer's `--autostart on|off` result. An entry turned off in Task Manager (its `Explorer\StartupApproved\Run` value, first byte odd) is disabled for first-launch setup's inspection only; the Settings checkbox keeps reading the `Run` value, so it can never get stuck off.
- **Linux:** write our user-level XDG autostart `.desktop` file. No administrator access or permission prompt. Target the persistent AppImage file, not its temporary mounted host binary.
- **macOS:** register the bundled `SMAppService` agent. If enabled, no row. If approval is required for an intended on registration, show Start at login / Open settings. Read `SMAppService.status` to confirm approval. Do not register a copy running from a temporary location as a permanent login target: App Translocation (an app opened from Downloads without being moved) or a read-only volume such as the installer's disk image. This limits only first-launch setup's default; an explicit choice in Settings registers any bundle, including one kept on a writable external drive.

Existing maintenance of an already-enabled Linux entry after an AppImage update remains ordinary maintenance. It must not re-enable a disabled entry or rerun first-launch setup.

## 5. Permission screen

Use the existing window and app styling. No additional navigation or permanent setup page. Keep rows to a name, one short explanation, and an action/status. No completed silent-setup checklist.

### macOS: both actions needed

```text
+-----------------------------------------------------------------+
| (sleepy skeleton illustration)                                  |
|                                                                 |
| Before you play                                                 |
|                                                                 |
| --------------------------------------------------------------  |
| Allow access to your games                     [ Allow access ] |
| Needed to find your games and back up and restore               |
| your progress.                                                  |
| --------------------------------------------------------------  |
| Start at login optional                       [ Open settings ] |
| Keeps your hotkeys ready whenever you play.                     |
|                                                                 |
|                            [ Skip ]                             |
+-----------------------------------------------------------------+
```

If only one row applies, omit the other row. Do not show a disabled or already-configured row when the screen initially opens.

### macOS: one action confirmed, another still available

```text
+-----------------------------------------------------------------+
| (sleepy skeleton illustration)                                  |
|                                                                 |
| Before you play                                                 |
|                                                                 |
| --------------------------------------------------------------  |
| Allow access to your games                          [ Allowed ] |
| Needed to find your games and back up and restore               |
| your progress.                                                  |
| --------------------------------------------------------------  |
| Start at login optional                       [ Open settings ] |
| Keeps your hotkeys ready whenever you play.                     |
|                                                                 |
|                          [ Continue ]                           |
+-----------------------------------------------------------------+
```

Allowed is a noninteractive confirmation. Keep a completed row in place during this screen's lifetime to avoid moving the remaining controls. Continue still requires a click; do not automatically advance.

### Linux: desktop needs shortcut approval/configuration

```text
+-----------------------------------------------------------------+
| (sleepy skeleton illustration)                                  |
|                                                                 |
| Before you play                                                 |
|                                                                 |
| --------------------------------------------------------------  |
| Allow keyboard shortcuts                             [ Set up ] |
| Needed to save and load from inside your game.                  |
|                                                                 |
|                            [ Skip ]                             |
+-----------------------------------------------------------------+
```

After confirmed shortcut registration, replace Set up with Allowed and Skip with Continue. Linux has no autostart or game-file permission row for the current AppImage implementation.

### Windows, or any OS with no applicable rows

```text
launch -> silent setup -> normal app
                          (no permission screen)
```

### Button and result behavior

| Result so far in this screen | Footer button |
| --- | --- |
| Nothing confirmed | Skip |
| Request opened or waiting for an answer | Skip |
| Request denied/cancelled/failed | Skip, unless another permission already succeeded |
| At least one permission confirmed, even if others remain | Continue |
| All displayed permissions confirmed | Continue |

Use confirmation events from the host. Do not switch the label on button click, window blur, return from System Settings, or an elapsed timeout alone.

For a grouped Game saves request, a confirmed grant for one necessary category counts toward Continue even if another category remains blocked. Mark the whole row Allowed only after all currently required locations have been verified; otherwise retain its action with a short "Some locations still need access" message.

During a request, disable duplicate actions and serialize requests so two OS dialogs are not launched together. Keep the exit action available. On dismissal, stop scheduling further prompts, ignore late results for this screen, and keep any permissions already granted. A native dialog already displayed may have to be dismissed by the user; do not claim that closing our screen revokes or closes it.

A denied or failed row gets one concise inline message and the applicable retry/settings action. Do not add an error page or block use of unrelated features. Button labels, row updates, and failures must be accessible to keyboard and screen-reader users.

## 6. Per-OS flows

### Windows

1. On the eligible first interactive launch, consume the first-launch opportunity.
2. Read the saved startup choice and existing OS entry. Preserve installer/Settings/flag/external choices. If genuinely unconfigured, register startup silently.
3. Register the configured global shortcuts normally. A conflict is a shortcut problem, not an OS permission request.
4. Perform ordinary game discovery. There is no blanket Windows game-data permission to request through this screen.
5. With no permission rows, finish onboarding and show the normal app.

Do not add an autostart question or a "setup complete" screen. Folder access errors or security-software blocks continue through normal game/error handling.

### macOS

1. Consume the first-launch opportunity and apply the same startup-choice precedence.
2. For a genuinely unconfigured, eligible app bundle, attempt login-item registration. Record an approval row only if registration needs user approval and intended startup is on.
3. Register shortcuts through the existing implementation; it does not require an Accessibility setup request.
4. Discover installed games without reading unapproved protected locations. Use known paths and the existing privacy guard to identify required access.
5. Add game access when a known library/discovery location or a detected game's saves need protected access. A blocked library can hide every game in it, so include that location even with no detected games. Do not probe unrelated app containers merely to force an OS prompt.
6. If no rows remain, finish and open the app. Otherwise show the one screen.
7. One Allow access action requests known library locations first, rescans after each approval, and continues to newly discovered protected saves. Serialize native requests and stop scheduling them if the screen is dismissed. Open settings for login approval navigates to Login Items.
8. Verify outcomes, update rows, and change Skip to Continue after any confirmed grant. Finish only when the user exits the screen.

#### What Game saves covers

The row can include Files and Folders access for game discovery or saves (for example Documents or external volumes), other apps' protected data containers, and App Management only if a detected game really stores saves inside an `.app` bundle and restore requires writing there. These are separate OS protections represented by one app-level row; one click may lead to multiple sequential native requests. Approval for one container or drive must not imply approval for another. Partial approval keeps the remaining access action available.

Other apps' data access is not Automation permission. Do not request Automation, Accessibility, or Full Disk Access for these current features.

The host performing the actual save-file work must perform the access request. A permission granted to a UI/helper process is not sufficient evidence that the host can access the files.

#### Verification and permission lifetime

- Login-item approval: refresh OS status when the window returns to the foreground; mark granted only when the service reports enabled. Opening settings alone leaves the row pending.
- File access: the requested operation's result and subsequent scoped verification establish whether the required location is accessible. A missing location, unrelated I/O failure, or a successful read of an unprotected ancestor must not be mistaken for proof of permission.
- There is no universal, non-prompting "all game access granted" query. Do not repeatedly probe unknown protected paths on a timer or every focus event. If a recheck could itself prompt, keep it behind an explicit Allow access / Check again action.
- A remembered first-launch grant is not permanent authority. Apple documents other-app-container access as process-lifetime permission. Onboarding state must never stand in for live file-access state or cause subsequent background work to bypass the privacy guard.
- Scope confirmation to the locations/OS permission scope actually tested. The existing category cache must not make broader promises than the OS does, including across processes or future games.

After this first-launch session, new games and later access failures use existing per-game guidance. They never recreate onboarding.

### Linux

1. Consume the first-launch opportunity and apply startup-choice precedence.
2. If unconfigured, create the user autostart entry silently. Respect `Hidden=true`, `X-GNOME-Autostart-enabled=false`, saved off intent, and ownership of existing entries.
3. Create/update our AppImage menu entry and icon silently, before portal use. Preserve package-managed or foreign entries.
4. Inspect the shortcut environment and initialize only operations known not to prompt.
5. If shortcut approval/configuration is needed and supported, show the single Keyboard shortcuts row. Otherwise finish onboarding and show the normal app.
6. Set up initiates the desktop's shortcut operation. Confirm the requested active bindings from the response, rather than treating a created session or closed dialog as success.
7. A confirmed result changes Skip to Continue. Cancellation, rejection, or incomplete binding leaves the action available with the actual result.

#### Linux shortcut conditions

| Environment | Silent behavior | Onboarding |
| --- | --- | --- |
| X11 | Register configured shortcuts normally. | No row; conflicts use existing settings/error handling. |
| Wayland, every configured shortcut id allowed before (listed by the portal, or recorded by the host after a confirmed bind) | Bind/reuse configured shortcuts silently. | No row when already usable. |
| Wayland with a supported portal where binding may prompt | Inspect capabilities/prior bindings without starting a prompting bind. | Set up explicitly initiates that bind. |
| No supported portal | Retain available X11/XWayland fallback. | No fictional permission action or universal installer button. |
| Shortcuts explicitly cleared in Settings | Keep them cleared. | No row asking to recreate them. |

Do not equate Wayland with a mandatory dialog. Portal behavior varies by backend. `BindShortcuts` may present a configuration dialog; portal availability and session creation do not prove that a new bind is silent or active, and only the bind's response confirms active bindings. The evidence for a silent rebind is that exactly these ids were allowed before (below); anything else defers the potentially prompting call to Set up. Validate supported backend behavior with native checks.

Each shortcut id carries its key (`save:CTRL+F5`), so a changed shortcut is a new id the desktop asks about again, and an unchanged one rebinds silently.

**GNOME 50 (verified 2026-10-03):** `ListShortcuts` in a new session lists nothing, although GNOME remembers allowed ids per app (`/org/gnome/settings-daemon/global-shortcuts/`) and binding them again shows no dialog. So "allowed before" also comes from the host's own record:

- After every confirmed bind (startup rebind, Set up, a Settings change), the host stores the bound ids in the settings (`portal_shortcuts`), per profile. The next start binds silently when every configured id is recorded or listed.
- A recorded rebind the desktop refuses (the user revoked the shortcuts in the desktop's settings, so it asked once and was cancelled) clears the record: later starts don't ask again and wait for Set up or a Settings change.
- Profiles from before onboarding are recorded as `legacy` at migration, meaning their current shortcuts: those versions bound at every start, so the user already answered the dialog.

Accepted trade-off: shortcuts revoked in the desktop's settings make the next start show the desktop's dialog once.

XWayland fallback covers X11-focused windows, not every native Wayland window. Do not report that fallback as a confirmed global portal grant. A delayed successful portal reply must update the host's live state and release duplicate fallback grabs as appropriate.

If setup is skipped, do not immediately call the same prompting bind from another startup path. Save the opt-out from this first-launch request; ordinary non-prompting registration/reuse continues on later launches. A shortcut changed in Settings is bound through the portal even when the portal doesn't carry the shortcuts yet, which may show the desktop's dialog; Settings waits for the answer, and a declined dialog keeps the new shortcuts on the X11/XWayland grabs. Missing Linux components remain ordinary support/repair cases, outside this screen.

## 7. Behavior and code ownership

### Host coordinator

Add a small host-owned first-launch coordinator, responsible for eligibility, one-time persistence, silent setup ordering, applicable rows, and dismissal. Call it from the actual window-opening flow so it also works when a minimized host is already running. Do not implement it only at the end of `run()` or only in React local storage.

Keep normal startup operations separate from the one-time coordinator:

```text
normal startup, every run
  -> recovery, monitoring, non-prompting discovery
  -> ordinary shortcut/session registration where silent
  -> maintenance of existing owned desktop/startup entries

first interactive launch, once
  -> default autostart decision, only when unconfigured
  -> collect permission requests
  -> optional permission screen
```

Reuse the existing settings and platform adapters. Add explicit inspect/request separation where an existing startup function can prompt. Do not build a second privacy subsystem or duplicate per-game policy.

### Refactoring scope

Keep the current OS-specific architecture. Limit cleanup to concrete duplication and the changes onboarding requires; a broad rewrite or a large line-count reduction is not a goal. Do not introduce a generic permission manager or shared integration lifecycle. File access, login approval, and shortcut sessions keep their own behavior and lifetimes.

- **First-launch state:** remove `Privacy::first_run`, `first_run_asks`, and `end_first_run` when the coordinator replaces them. The settings-backed lifecycle is the sole source of onboarding completion; the privacy cache records access only.
- **File access:** extend the existing `Privacy::ask` and platform probe with verified, scoped results and correct grant lifetime. Preserve the separate scan, configuration, and per-game workflows: scans skip previously denied categories, per-game requests prioritize store/recovery access, and configuration asks before committing a proposed path. Keep refresh/publication at the appropriate caller boundary rather than introducing a general request framework.
- **Autostart:** share intent persistence and application of startup choices across flags, Settings, and onboarding, with the precedence and serialization defined in section 4. Keep OS registration in the existing adapters. The shared operation must also work without constructing a normal `Host` for flag-only invocations. Update Settings' rollback behavior deliberately to preserve recorded intent on OS failure.
- **Linux portal:** adapt the existing binding worker to separate inspection from potentially prompting calls and publish verified live results. The worker reports each confirmed bind's ids to the host, which records them (section 6) and hands them back before the next start. Preserve X11/XWayland fallback, delayed replies, and Settings' wait-for-result behavior. A failed replacement must retain the previous portal session. Do not replace the worker or make every caller asynchronous merely to share onboarding code.
- **Shortcut duplication:** extract only the duplicated macOS/Linux conversion, event-to-action mapping, and registration/rollback algorithms into small internal helpers. Pass existing managers and bindings into those helpers. Keep manager ownership, event listeners, thread dispatch, and fallback handling in the platform modules; leave Windows' native integration separate.

Leave unrelated desktop-entry module reorganization outside this task. The onboarding coordinator consumes existing capabilities and operations; it does not take ownership of their resources or permission caches.

### UI/IPC contract

Expose one optional onboarding snapshot in the normal host state. Proposed shape, with final Rust/TypeScript naming left to implementation:

```text
onboarding: null | {
  session,
  inspecting,                  # rows not known yet: the UI shows its loading state
  rows: [{ id, kind, status, message?, action? }],
  any_permission_confirmed
}

kind:   game_access | login_approval | shortcuts
status: needs_action | requesting | granted | partial | denied | failed
action: allow_access | open_settings | check_again | set_up

commands:
  request_onboarding_permission(session, row)   # answers once the OS result is known
  finish_onboarding(session)
```

Row messages and actions come from the host; the UI owns only each kind's name and explanation, and marks the login row with a dimmed "optional": the app works without it. Game access: Allow access asks for each waiting scope in turn, rescanning after approval; a refusal offers Open settings, which opens the pane of a refused category, then Check again, which reads again (macOS never prompts twice). Start at login: Open settings opens Login Items; the row turns Allowed only when the service reports enabled, read again whenever the window regains focus.

- Host validates that the session is active and the requested row is applicable; UI visibility is not authorization.
- Use the existing state/event publication path. OS callback/worker results update the host and then the UI.
- Group game-access targets in the host; the UI must not invent paths or OS permission categories.
- Derive the footer label from confirmed results, not attempted actions or silent setup successes.
- Exiting through Skip or Continue invokes the same finish operation. Closing the window does likewise: the UI finishes on its close request, and the host also finishes a session whose UI connection stays gone (or never comes) for a grace period (30 s; `--onboarding-grace-secs` for tests), which still lets a transient disconnect resume.
- Reject stale/new prompting requests after finish. Late callbacks may update ordinary capability state, but cannot resurrect onboarding or launch a subsequent queued prompt.
- Add matching UI mock scenarios and protocol types. Keep platform checks out of the component except for presentation supplied by the host.

### Existing code to adapt

- `apps/host/src/lib.rs`, `feedback.rs`, `server.rs`: lifecycle, interactive opening, request gating, publication, dismissal, and flag handling.
- `apps/host/src/model.rs`, `queries.rs`, `crates/storage/src/lib.rs`: shared startup intent and dedicated first-launch state/migration.
- `crates/platform/src/autostart/*`: inspect registration/ownership/disabled status and reuse setters.
- `apps/host/src/privacy.rs`, `crates/platform/src/privacy/*`: remove automatic first-run prompting; scoped request/results and correct grant lifetime.
- `crates/platform/src/integration/linux.rs` and `linux/portal.rs`: separate capability inspection and binding; report asynchronous outcomes.
- `crates/platform/src/integration/macos.rs` and `linux.rs`: extract duplicated shortcut algorithms within the limits above, preserving platform resource ownership and threading.
- `crates/ipc/src/types.rs`, `apps/ui/src/types.ts`, bridge/mock code, and `App.tsx`: optional onboarding snapshot, actions, and one conditional screen.
- `packaging/windows/savescummer.iss`: keep the existing `--autostart on|off` contract; both outcomes must become durable choices.

## 8. Acceptance checks

Use fake OS adapters for deterministic lifecycle/decision tests and native release builds for actual OS permission behavior. Do not reset the developer's real permissions or startup configuration as part of automated tests.

### Lifecycle and preserved choices

1. Fresh profile: invoke `--autostart off`; verify the choice is stored, no ordinary host run is recorded, and no window/prompt opens. Launch the app; verify startup remains off and onboarding can still run for other permissions.
2. Repeat with `--autostart on`; verify the registration is preserved, not reapplied as a new default. macOS approval may still be offered if genuinely required.
3. Fresh profile with no choice: open the app; verify the silent default is attempted once. Dismiss setup, disable startup, and launch twice more; verify it remains disabled and setup never returns.
4. Seed an externally disabled Linux desktop entry or a foreign entry; launch a fresh profile; verify no re-enable/overwrite. Repeat the equivalent ownership/disabled cases on other platforms.
5. Start a fresh host minimized or via a normal CLI command; verify no onboarding/prompt/default-on setup is consumed. Explicitly open its window; verify the first-launch flow begins once.
6. Upgrade an old profile with host-run/game history but no onboarding marker; verify no onboarding and no startup-default change. An installer-only profile must still qualify.
7. Complete with Skip, with Continue, and by closing the window. Reopen the window and restart the host in each case; verify no onboarding.
8. Terminate the host after `started` is persisted. Restart; verify no repeated setup attempt/screen. A new game or changed app signature also must not restart onboarding.
9. Race an explicit off flag against initial setup using controlled adapters; verify explicit intent wins and no later default-on operation follows it.
10. Run development/demo/no-integrations/alternate-data-directory cases; verify no real startup entry, desktop installation, or OS request is created.

### Screen and result handling

1. With no applicable permissions, verify the normal app opens without an empty setup screen.
2. With one row, verify singular heading, one action, and Skip. With two macOS rows, verify the two-row ASCII layout.
3. Open a permission request without granting it; verify Skip remains. Return from settings without changing anything; verify Skip remains.
4. Confirm either macOS permission while the other is still pending; verify Continue appears and the remaining action stays available. Confirm the Linux bindings; verify the same transition.
5. Deny/cancel/time out before any success; verify Skip and a usable remedy. A previous success must keep Continue even when a later action fails.
6. Grant only part of grouped game access; verify Continue, but not a falsely completed Game saves row.
7. Dismiss while a request is pending; deliver its result afterward; verify no new screen or subsequent prompt, while real granted access is retained.

### Native platform checks

- **Windows:** test installer on/off and a release app launched without the installer. Verify silent per-user startup setup and no permission screen; a shortcut conflict must not fabricate a permission row.
- **macOS:** test zero games with and without a blocked known library, readable saves, protected saves, and both pending game access and login approval. Approve a blocked library and verify the same action discovers its games and requests protected saves; test two independent drives and containers, partial approval, and dismissal before the next request. Verify no access request before the row action; confirm the host obtains access; test denial, settings return without approval, actual approval, and restart with transient access no longer valid. Existing per-game guidance handles later needs without onboarding.
- **Linux/X11:** verify silent autostart/menu setup and shortcut registration. No permission screen for those successful operations.
- **Linux/Wayland:** test supported GNOME/KDE portal environments and an unavailable backend. GNOME 50: first launch with ids already allowed shows the row and Set up binds without a dialog (verified); the next start must rebind silently from the host's record; revoking the shortcuts in GNOME Settings must ask once at the next start and, if cancelled, never again. Record whether each backend binds silently or prompts; verify the screen follows capabilities rather than OS/session name alone. Verify cancellation, remembered bindings, changed/cleared shortcuts, incomplete binding, fallback limitations, and late successful responses.

When implementing Rust changes, run `cargo fmt --all`, then `cargo clippy -q --workspace --all-targets --locked -- -D warnings`, and relevant behavioral tests. Run the UI checks for UI/IPC changes. This spec alone does not require builds or tests.

## 9. Platform references

These explain platform constraints; actual first-launch policy is defined above.

- [Apple: other-app-container permission lifetime](https://developer.apple.com/forums/thread/742147)
- [Apple: What is new in privacy, app-container access](https://developer.apple.com/videos/play/wwdc2023/10053/?time=1066)
- [Apple: SMAppService](https://developer.apple.com/documentation/servicemanagement/smappservice)
- [XDG Desktop Portal: Global Shortcuts](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html)
