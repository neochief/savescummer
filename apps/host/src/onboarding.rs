//! First-launch setup (PLAN-onboarding.md): once per profile, at the
//! app's first deliberate opening, setup that needs nothing from the user
//! happens silently, and one small screen offers the OS permissions that
//! need the user's explicit action. Nothing here grants access by itself or
//! stands in for live access state: rows report what the OS confirmed.
//!
//! The settings' `first_launch_state` is the lifecycle: `pending` until the
//! window first opens, `started` once that opening was consumed (before any
//! side effect), `finished` after Skip, Continue, closing the window, or
//! when there was nothing to show. A `started` left by a host that died is
//! read as finished: setup never runs twice.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use savescummer_core::{ErrorKind, Failure};
use savescummer_ipc::{Onboarding, OnboardingAction, OnboardingKind, OnboardingRow, OnboardingStatus, Phase};
use savescummer_platform::autostart::Status;
use savescummer_platform::integration::ShortcutSetup;
use savescummer_storage::{self as db, Storage};

use crate::host::{Host, Inner, new_id};
use crate::model::{PORTAL_SHORTCUTS_LEGACY, SETTING_FIRST_LAUNCH, SETTING_PORTAL_SHORTCUTS};
use crate::privacy::Answer;

/// How long setup waits for the shortcuts portal to say what it needs. A
/// portal that isn't running yet (the first call after sign-in, or after its
/// settings changed) takes seconds to start; until then the UI shows its
/// loading state.
const SHORTCUTS_WAIT: Duration = Duration::from_secs(30);

/// Where this profile's first launch stands, in this host run.
#[derive(Debug, Default)]
pub enum Stage {
    /// Not a first launch (or it's over).
    #[default]
    Done,
    /// The window hasn't opened yet.
    Pending,
    /// This run's one session.
    Active(Session),
}

#[derive(Debug)]
pub struct Session {
    id: String,
    /// The rows aren't known yet.
    inspecting: bool,
    rows: Vec<Row>,
    /// A row's OS request runs: one at a time.
    requesting: bool,
    /// Some requested permission was confirmed.
    confirmed: bool,
}

#[derive(Debug, Clone)]
struct Row {
    kind: OnboardingKind,
    status: OnboardingStatus,
    message: Option<String>,
    action: Option<OnboardingAction>,
}

impl Row {
    fn new(kind: OnboardingKind, action: OnboardingAction) -> Row {
        Row { kind, status: OnboardingStatus::NeedsAction, message: None, action: Some(action) }
    }

    fn id(&self) -> &'static str {
        id(self.kind)
    }
}

fn id(kind: OnboardingKind) -> &'static str {
    match kind {
        OnboardingKind::GameAccess => "game_access",
        OnboardingKind::LoginApproval => "login_approval",
        OnboardingKind::Shortcuts => "shortcuts",
    }
}

/// A row after its request.
struct Update {
    status: OnboardingStatus,
    message: Option<&'static str>,
    action: Option<OnboardingAction>,
    /// The OS confirmed a grant.
    confirmed: bool,
}

/// Reads (and on an old or new profile, writes) the lifecycle at startup,
/// before this run is recorded. A profile used before this feature existed
/// is finished without setup; one holding only settings (an installer's
/// `--autostart`) is new. A marker that can't be written means no setup.
pub fn init(storage: &mut Storage) -> Stage {
    let state = db::setting(storage.conn(), SETTING_FIRST_LAUNCH);
    let mut migrated = false;
    let (stage, write) = match state.as_ref().map(|s| s.as_deref()) {
        Ok(Some("pending")) => (Stage::Pending, None),
        Ok(Some("finished")) => (Stage::Done, None),
        // A host that died during setup, or a value from the future.
        Ok(Some(_)) => (Stage::Done, Some("finished")),
        Ok(None) => match db::has_been_used(storage.conn()) {
            Ok(true) => {
                migrated = true;
                (Stage::Done, Some("finished"))
            }
            Ok(false) => (Stage::Pending, Some("pending")),
            Err(e) => {
                crate::trace(&format!("first launch: can't read the profile: {e}"));
                return Stage::Done;
            }
        },
        Err(e) => {
            crate::trace(&format!("first launch: can't read its state: {e}"));
            return Stage::Done;
        }
    };
    // Versions before this one bound the portal's shortcuts at every
    // start, so the user already answered the desktop's dialog: the next
    // start binds the current ones again (see `feedback::start`).
    let legacy = |c: &db::Connection| {
        if migrated && db::setting(c, SETTING_PORTAL_SHORTCUTS)?.is_none() {
            db::set_setting(c, SETTING_PORTAL_SHORTCUTS, PORTAL_SHORTCUTS_LEGACY)?;
        }
        Ok(())
    };
    if let Some(value) = write
        && let Err(e) = storage.write(|c| {
            db::set_setting(c, SETTING_FIRST_LAUNCH, value)?;
            legacy(c)
        })
    {
        crate::trace(&format!("first launch: can't record {value}: {e}"));
        return Stage::Done;
    }
    stage
}

/// The window is being opened deliberately (a user launch, the tray, a
/// second launch, `show-ui`): on a pending first launch, the session
/// starts. Its setup runs in the background; the UI shows its loading state
/// until the rows are known.
pub fn begin(host: &Arc<Host>) {
    let id = {
        let mut inner = host.lock();
        if !matches!(inner.onboarding, Stage::Pending) {
            return;
        }
        // Consumed before any side effect: at most once, even after a crash.
        if let Err(e) = host.db().write(|c| db::set_setting(c, SETTING_FIRST_LAUNCH, "started")) {
            crate::trace(&format!("first launch: can't record it started, so no setup: {e}"));
            inner.onboarding = Stage::Done;
            return;
        }
        let id = new_id("onboarding");
        inner.onboarding = Stage::Active(Session {
            id: id.clone(),
            inspecting: true,
            rows: Vec::new(),
            requesting: false,
            confirmed: false,
        });
        host.publish(&mut inner);
        id
    };
    crate::trace("first launch: setting up");
    let setup = host.clone();
    let session = id.clone();
    std::thread::spawn(move || prepare(&setup, &session));
    let watched = host.clone();
    std::thread::spawn(move || watch(&watched, &id));
}

/// Silent setup, then the rows the user can act on. With none, setup is
/// over at once.
fn prepare(host: &Arc<Host>, id: &str) {
    // A scan still running isn't an empty library; the integrations start
    // just after the first one.
    while !host.started.load(Ordering::SeqCst) {
        if host.lock().phase == Phase::ShuttingDown {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let mut rows = Vec::new();
    if !crate::privacy::onboarding_waiting(host).is_empty() {
        rows.push(Row::new(OnboardingKind::GameAccess, OnboardingAction::AllowAccess));
    }
    if os_setup(host) && launch_needs_approval(host) {
        rows.push(Row::new(OnboardingKind::LoginApproval, OnboardingAction::OpenSettings));
    }
    if shortcuts_need_setup(host) {
        rows.push(Row::new(OnboardingKind::Shortcuts, OnboardingAction::SetUp));
    }
    let mut inner = host.lock();
    let Some(session) = active(&mut inner, id) else { return };
    if rows.is_empty() {
        drop(inner);
        crate::trace("first launch: nothing to ask");
        end(host);
        return;
    }
    crate::trace(&format!("first launch: asking for {}", rows.iter().map(Row::id).collect::<Vec<_>>().join(", ")));
    session.rows = rows;
    session.inspecting = false;
    host.publish(&mut inner);
}

/// Real OS setup: never in the demo, without integrations (tests), or for
/// a data folder of its own, which must not touch the installed app's
/// startup entry.
fn os_setup(host: &Host) -> bool {
    !host.opts.demo
        && !host.opts.no_integrations
        && host.opts.data_dir.as_deref().is_none_or(|dir| dir == savescummer_platform::data_dir())
}

/// Turns startup on when nobody chose and nothing is there, then tells
/// whether the intended startup waits for the user's approval (macOS Login
/// Items). An entry found waiting is not taken as a wish to start.
fn launch_needs_approval(host: &Host) -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let intended = {
        let _choice = host.launch_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut storage = host.db();
        crate::autostart::apply_default(&mut storage, &exe, host.opts.data_dir.as_deref());
        crate::autostart::recorded(storage.conn()).ok().flatten() == Some(true)
    };
    crate::queries::refresh_launch(host);
    intended && savescummer_platform::autostart::inspect(&exe) == Status::NeedsApproval
}

fn shortcuts_need_setup(host: &Host) -> bool {
    // Shortcuts cleared in Settings stay cleared.
    if host.lock().shortcuts.iter().all(Option::is_none) {
        return false;
    }
    // Polled, never waited for under the integration's lock, which Settings
    // and shutting down need.
    let deadline = Instant::now() + SHORTCUTS_WAIT;
    loop {
        let setup = {
            let integration = host.integration.lock().unwrap_or_else(|e| e.into_inner());
            integration.as_ref().map_or(ShortcutSetup::Ready, |i| i.shortcut_setup(Duration::ZERO))
        };
        match setup {
            ShortcutSetup::Unknown if Instant::now() < deadline && host.lock().phase != Phase::ShuttingDown => {
                std::thread::sleep(Duration::from_millis(100));
            }
            ShortcutSetup::Unknown => {
                crate::trace("first launch: the shortcuts portal didn't answer in time; not asking");
                return false;
            }
            setup => return setup == ShortcutSetup::Needed,
        }
    }
}

/// The live session `id`, if it still is.
fn active<'a>(inner: &'a mut Inner, id: &str) -> Option<&'a mut Session> {
    match &mut inner.onboarding {
        Stage::Active(session) if session.id == id => Some(session),
        _ => None,
    }
}

fn is_active(host: &Host, id: &str) -> bool {
    active(&mut host.lock(), id).is_some()
}

/// What the UI shows: nothing outside a session.
pub fn snapshot(inner: &Inner) -> Option<Onboarding> {
    let Stage::Active(session) = &inner.onboarding else { return None };
    Some(Onboarding {
        session: session.id.clone(),
        inspecting: session.inspecting,
        rows: session
            .rows
            .iter()
            .map(|row| OnboardingRow {
                id: row.id().to_string(),
                kind: row.kind,
                status: row.status,
                message: row.message.clone(),
                action: row.action,
            })
            .collect(),
        any_permission_confirmed: session.confirmed,
    })
}

/// Runs a row's OS request and answers with its result. Only the live
/// session's rows; one request at a time.
pub fn request(host: &Arc<Host>, session: &str, row: &str) -> Result<serde_json::Value, Failure> {
    let (kind, action, before) = {
        let mut inner = host.lock();
        let live = active(&mut inner, session)
            .ok_or_else(|| Failure::new(ErrorKind::InvalidRequest, "first-launch setup is over"))?;
        if live.inspecting {
            return Err(Failure::new(ErrorKind::InvalidRequest, "first-launch setup isn't ready"));
        }
        if live.requesting {
            return Err(Failure::new(ErrorKind::InvalidRequest, "another permission request is open"));
        }
        let entry = live
            .rows
            .iter_mut()
            .find(|r| r.id() == row)
            .ok_or_else(|| Failure::new(ErrorKind::NotFound, "no such permission"))?;
        let (Some(action), before) = (entry.action, entry.status) else {
            return Ok(serde_json::json!({ "row": row, "status": entry.status }));
        };
        let kind = entry.kind;
        entry.status = OnboardingStatus::Requesting;
        entry.message = None;
        live.requesting = true;
        host.publish(&mut inner);
        (kind, action, before)
    };
    let update = match kind {
        OnboardingKind::GameAccess => game_access(host, session, action, before),
        OnboardingKind::LoginApproval => login_approval(action),
        OnboardingKind::Shortcuts => shortcuts(host),
    };
    let status = update.status;
    let mut inner = host.lock();
    let Some(live) = active(&mut inner, session) else {
        // Dismissed meanwhile: whatever was granted stays granted.
        crate::trace(&format!("first launch: {row} answered after setup ended"));
        return Err(Failure::new(ErrorKind::InvalidRequest, "first-launch setup is over"));
    };
    live.requesting = false;
    live.confirmed |= update.confirmed;
    if let Some(entry) = live.rows.iter_mut().find(|r| r.kind == kind) {
        entry.status = update.status;
        entry.message = update.message.map(str::to_string);
        entry.action = update.action;
    }
    host.publish(&mut inner);
    Ok(serde_json::json!({ "row": row, "status": status }))
}

/// Allow access: asks for known libraries and then the saves they reveal,
/// one scope at a time, stopping when setup ends. Open settings goes to the pane of one
/// still refused; Check again asks again (macOS doesn't prompt twice, so
/// it only reads).
fn game_access(host: &Arc<Host>, id: &str, action: OnboardingAction, before: OnboardingStatus) -> Update {
    if action == OnboardingAction::OpenSettings {
        let waiting = crate::privacy::onboarding_waiting(host);
        // A refused category's pane: that's where the user turns access on.
        let pane = waiting
            .iter()
            .find(|(c, path)| host.privacy.is_denied(path, *c))
            .or_else(|| waiting.first())
            .map(|(c, _)| c.settings_url());
        if let Some(url) = pane
            && let Err(e) = savescummer_platform::open_folder(Path::new(url))
        {
            crate::trace(&format!("first launch: can't open the privacy settings: {e}"));
        }
        return Update {
            status: before,
            message: Some("Turn on access in System Settings, then check again."),
            action: Some(OnboardingAction::CheckAgain),
            confirmed: false,
        };
    }
    let mut asked = std::collections::BTreeSet::new();
    let (mut granted, mut denied) = (false, false);
    loop {
        // A granted library reveals games and their save locations in the
        // rescan; they are included in this same explicit action.
        let next = crate::privacy::onboarding_waiting(host)
            .into_iter()
            .find(|(c, path)| !asked.contains(&host.privacy.scope(path, *c)));
        let Some((category, path)) = next else { break };
        if !is_active(host, id) {
            break;
        }
        asked.insert(host.privacy.scope(&path, category));
        match host.privacy.ask(&path, category) {
            Answer::Granted => {
                granted = true;
                let scan = crate::privacy::granted(host);
                while host.scans.wait(scan, Duration::from_millis(100)).is_none() {
                    if !is_active(host, id) || host.lock().phase == Phase::ShuttingDown {
                        return Update { status: before, message: None, action: Some(action), confirmed: granted };
                    }
                }
            }
            Answer::Denied => denied = true,
            Answer::Unverified => {}
        }
    }
    let remaining = crate::privacy::onboarding_waiting(host);
    let some_denied = remaining.iter().any(|(c, path)| host.privacy.is_denied(path, *c));
    let retry = if some_denied { OnboardingAction::OpenSettings } else { OnboardingAction::AllowAccess };
    if remaining.is_empty() {
        Update { status: OnboardingStatus::Granted, message: None, action: None, confirmed: true }
    } else if granted || before == OnboardingStatus::Partial {
        Update {
            status: OnboardingStatus::Partial,
            message: Some("Some locations still need access."),
            action: Some(retry),
            confirmed: granted,
        }
    } else if denied || some_denied {
        Update {
            status: OnboardingStatus::Denied,
            message: Some("macOS didn't allow access."),
            action: Some(OnboardingAction::OpenSettings),
            confirmed: false,
        }
    } else {
        Update {
            status: OnboardingStatus::Failed,
            message: Some("Couldn't verify access to some locations. Check that they're available, then try again."),
            action: Some(OnboardingAction::AllowAccess),
            confirmed: false,
        }
    }
}

/// Open settings: Login Items, where only the user approves. Approval is
/// confirmed by the service's status, read when the window is in front
/// again ([`recheck`]).
fn login_approval(action: OnboardingAction) -> Update {
    let _ = action;
    if let Err(e) = savescummer_platform::autostart::open_approval_settings() {
        crate::trace(&format!("first launch: can't open Login Items: {e}"));
        return Update {
            status: OnboardingStatus::Failed,
            message: Some("Couldn't open System Settings."),
            action: Some(OnboardingAction::OpenSettings),
            confirmed: false,
        };
    }
    match login_status() {
        Some(Status::Enabled) => {
            Update { status: OnboardingStatus::Granted, message: None, action: None, confirmed: true }
        }
        _ => Update {
            status: OnboardingStatus::NeedsAction,
            message: Some("Turn on SaveScummer in Login Items."),
            action: Some(OnboardingAction::OpenSettings),
            confirmed: false,
        },
    }
}

fn login_status() -> Option<Status> {
    std::env::current_exe().ok().map(|exe| savescummer_platform::autostart::inspect(&exe))
}

/// Set up: binds the shortcuts through the desktop's portal, which may
/// show its dialog, and waits for the user's answer.
fn shortcuts(host: &Host) -> Update {
    let answer = {
        let integration = host.integration.lock().unwrap_or_else(|e| e.into_inner());
        integration.as_ref().map(|i| i.set_up_shortcuts())
    };
    let result = match answer {
        Some(answer) => answer.recv().unwrap_or_else(|_| Err("the shortcuts portal stopped".into())),
        None => Err("hotkeys are unavailable".into()),
    };
    match result {
        Ok(()) => Update { status: OnboardingStatus::Granted, message: None, action: None, confirmed: true },
        Err(e) => {
            crate::trace(&format!("first launch: shortcuts: {e}"));
            let cancelled = e.contains("cancelled");
            Update {
                status: if cancelled { OnboardingStatus::Denied } else { OnboardingStatus::Failed },
                message: Some(if cancelled {
                    "The desktop's dialog was cancelled."
                } else {
                    "The desktop didn't allow the shortcuts."
                }),
                action: Some(OnboardingAction::SetUp),
                confirmed: false,
            }
        }
    }
}

/// The window is in front again (back from System Settings): a waiting
/// login approval is read again. Reading the status never prompts.
pub fn recheck(host: &Host) {
    let waiting = {
        let inner = host.lock();
        let Stage::Active(session) = &inner.onboarding else { return };
        session.rows.iter().any(|r| r.kind == OnboardingKind::LoginApproval && r.status != OnboardingStatus::Granted)
    };
    if !waiting || login_status() != Some(Status::Enabled) {
        return;
    }
    crate::queries::refresh_launch(host);
    let mut inner = host.lock();
    let Stage::Active(session) = &mut inner.onboarding else { return };
    if let Some(row) = session.rows.iter_mut().find(|r| r.kind == OnboardingKind::LoginApproval) {
        row.status = OnboardingStatus::Granted;
        row.message = None;
        row.action = None;
        session.confirmed = true;
    }
    host.publish(&mut inner);
}

/// Skip, Continue or closing the window: setup is over for good. A stale
/// session's finish is refused; finishing twice is fine.
pub fn finish(host: &Host, session: &str) -> Result<serde_json::Value, Failure> {
    match &host.lock().onboarding {
        Stage::Active(live) if live.id != session => {
            return Err(Failure::new(ErrorKind::InvalidRequest, "not the current setup session"));
        }
        Stage::Active(_) => {}
        Stage::Done | Stage::Pending => return Ok(serde_json::json!({ "finished": true })),
    }
    crate::trace("first launch: finished");
    end(host);
    Ok(serde_json::json!({ "finished": true }))
}

/// Ends the session now, and records it. In memory it's over even when the
/// record fails: the recorded `started` already keeps it from coming back.
fn end(host: &Host) {
    {
        let mut inner = host.lock();
        inner.onboarding = Stage::Done;
        host.publish(&mut inner);
    }
    if let Err(e) = host.db().write(|c| db::set_setting(c, SETTING_FIRST_LAUNCH, "finished")) {
        crate::trace(&format!("first launch: can't record it finished: {e}"));
    }
}

/// Closing the window ends setup too. The UI's connection going away for
/// longer than a reconnect takes (or never coming) counts as closed.
fn watch(host: &Host, id: &str) {
    let grace = Duration::from_secs(host.opts.onboarding_grace_secs);
    let mut seen = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let connected = {
            let mut inner = host.lock();
            if active(&mut inner, id).is_none() {
                return;
            }
            inner.ui_connections > 0
        };
        if connected {
            seen = Instant::now();
        } else if seen.elapsed() >= grace {
            crate::trace("first launch: the window is gone; setup is over");
            end(host);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage() -> (tempfile::TempDir, Storage) {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(&dir.path().join("host.db")).unwrap();
        (dir, storage)
    }

    fn state(storage: &Storage) -> Option<String> {
        db::setting(storage.conn(), SETTING_FIRST_LAUNCH).unwrap()
    }

    #[test]
    fn a_new_profile_is_pending_and_stays_so_until_the_window_opens() {
        let (_dir, mut storage) = storage();
        // An installer's choice doesn't make it an old profile.
        storage.write(|c| db::set_setting(c, crate::model::SETTING_LAUNCH, "0")).unwrap();
        assert!(matches!(init(&mut storage), Stage::Pending));
        assert_eq!(state(&storage).as_deref(), Some("pending"));
        // Background runs keep it pending, though they're recorded.
        storage.write(|c| db::start_run(c, "host-1", "2026-10-01T00:00:00Z")).unwrap();
        assert!(matches!(init(&mut storage), Stage::Pending));
    }

    #[test]
    fn a_profile_used_before_onboarding_existed_never_gets_it() {
        let (_dir, mut storage) = storage();
        storage.write(|c| db::start_run(c, "host-1", "2026-10-01T00:00:00Z")).unwrap();
        assert!(matches!(init(&mut storage), Stage::Done));
        assert_eq!(state(&storage).as_deref(), Some("finished"));
        let portal = db::setting(storage.conn(), SETTING_PORTAL_SHORTCUTS).unwrap();
        assert_eq!(portal.as_deref(), Some(PORTAL_SHORTCUTS_LEGACY), "its shortcuts were allowed before");
    }

    #[test]
    fn a_session_a_crash_interrupted_is_over() {
        let (_dir, mut storage) = storage();
        storage.write(|c| db::set_setting(c, SETTING_FIRST_LAUNCH, "started")).unwrap();
        assert!(matches!(init(&mut storage), Stage::Done));
        assert_eq!(state(&storage).as_deref(), Some("finished"));
        assert!(matches!(init(&mut storage), Stage::Done));
    }
}
