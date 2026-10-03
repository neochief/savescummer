//! Global shortcuts through the desktop portal
//! (`org.freedesktop.portal.GlobalShortcuts`), for Wayland sessions, where
//! no program can grab keys itself. The compositor delivers the shortcuts
//! whatever window is in front, native Wayland ones included. GNOME 48+ and
//! KDE Plasma 6 have it.
//!
//! The desktop asks the user once to allow the shortcuts and remembers the
//! answer per shortcut id, keeping the key it was given the first time even
//! when a later bind prefers another. So each id carries its key
//! (`save:CTRL+F5`): a changed shortcut is a new id the desktop asks about
//! again, with the new key, and an unchanged one binds silently.
//!
//! Binding may show that dialog, so starting only looks ([`Command::Inspect`]):
//! when every shortcut id was allowed before, binding them again is silent
//! and starting binds them; otherwise binding waits for an explicit request
//! (first-launch setup, or a changed shortcut in Settings). Allowed before
//! means the portal lists the id, or the host recorded it after an earlier
//! bind: GNOME lists nothing in a new session, though it remembers.
//!
//! The portal runs on its own thread with an async-io executor; [`Portal`]
//! talks to it over a channel.

use std::sync::mpsc;
use std::time::Duration;

use ashpd::desktop::Session;
use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use futures_channel::mpsc as async_mpsc;
use futures_util::future::{Either, select};
use futures_util::{FutureExt, StreamExt};

use super::{HotkeyAction, Key, Shortcut, Shortcuts};

/// The app id the portal knows us by. Registering it works only where a
/// desktop entry with this name is installed (the AppImage installs one,
/// see `menu_entry`). Otherwise the portal names the host after the systemd
/// scope it runs in: the app that started it (a terminal, the file
/// manager), or none.
const APP_ID: &str = "com.savescummer.SaveScummer";

/// How long a rebind waits for the user to answer the desktop's dialog.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(90);

enum Command {
    /// Binds silently if the desktop remembers every id; answers whether
    /// the shortcuts are bound now.
    Inspect(Shortcuts, Vec<String>, mpsc::Sender<Result<bool, String>>),
    Bind(Shortcuts, Option<mpsc::Sender<Result<(), String>>>),
}

#[derive(Clone)]
pub struct Portal {
    commands: async_mpsc::UnboundedSender<Command>,
}

/// Whether to try the portal: in a Wayland session. Under X11 the key
/// grabs work everywhere and need no dialog.
pub fn wanted() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some_and(|d| !d.is_empty())
}

/// Starts the portal thread and inspects `shortcuts` (see the module docs);
/// `allowed` are the ids the host recorded as bound before. The receiver
/// gets whether they are bound now, never after a dialog. On the portal
/// thread, `on_press` runs for each press and `on_bound` with the ids of
/// each successful bind, for the host to record.
pub fn start(
    shortcuts: Shortcuts,
    allowed: Vec<String>,
    on_press: fn(HotkeyAction),
    on_bound: fn(Vec<String>),
) -> Result<(Portal, mpsc::Receiver<Result<bool, String>>), String> {
    let (commands, commands_rx) = async_mpsc::unbounded();
    let (first_tx, first_rx) = mpsc::channel();
    let _ = commands.unbounded_send(Command::Inspect(shortcuts, allowed, first_tx));
    std::thread::Builder::new()
        .name("shortcuts-portal".into())
        .spawn(move || async_io::block_on(run(commands_rx, on_press, on_bound)))
        .map_err(|e| format!("the shortcuts portal thread didn't start: {e}"))?;
    Ok((Portal { commands }, first_rx))
}

impl Portal {
    /// Binds `shortcuts` in place of the current ones, waiting for the
    /// user if the desktop asks. On failure the current ones stay.
    pub fn bind(&self, shortcuts: Shortcuts) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.commands
            .unbounded_send(Command::Bind(shortcuts, Some(tx)))
            .map_err(|_| "the shortcuts portal stopped".to_string())?;
        match rx.recv_timeout(ANSWER_TIMEOUT + Duration::from_secs(5)) {
            Ok(result) => result,
            Err(_) => Err("the shortcuts portal didn't answer".into()),
        }
    }
}

/// The portal thread. It ends when [`Portal`] is dropped (the channel
/// closes), closing the session, which releases the shortcuts.
async fn run(
    mut commands: async_mpsc::UnboundedReceiver<Command>,
    on_press: fn(HotkeyAction),
    on_bound: fn(Vec<String>),
) {
    let _ = ashpd::register_host_app(APP_ID.parse().expect("a valid app id")).await;
    let setup = async {
        let portal = GlobalShortcuts::new().await?;
        let activated = portal.receive_activated().await?;
        Ok::<_, ashpd::Error>((portal, activated))
    };
    let (portal, mut activated) = match setup.await {
        Ok(setup) => setup,
        Err(e) => {
            // Answer every waiting request, then stop.
            let error = format!("the desktop has no shortcuts portal ({e})");
            commands.close();
            while let Some(command) = commands.next().await {
                match command {
                    Command::Inspect(_, _, reply) => drop(reply.send(Err(error.clone()))),
                    Command::Bind(_, reply) => drop(reply.map(|reply| reply.send(Err(error.clone())))),
                }
            }
            return;
        }
    };
    // The bound session and its shortcut ids, in action order.
    let mut bound: Option<(Session<GlobalShortcuts>, Vec<String>)> = None;
    loop {
        match select(commands.next(), activated.next()).await {
            Either::Left((Some(Command::Inspect(shortcuts, allowed, reply)), _)) => {
                let result = match remembered(&portal, shortcuts, &allowed).await {
                    Ok(true) => {
                        let rebound = replace(&mut bound, bind(&portal, shortcuts).await).await;
                        // Bound: recorded again. Refused (revoked in the
                        // desktop's settings, so it asked): forgotten, so
                        // the next start doesn't ask again.
                        on_bound(if rebound.is_ok() { bound_ids(shortcuts) } else { Vec::new() });
                        rebound.map(|()| true)
                    }
                    other => other,
                };
                let _ = reply.send(result);
            }
            Either::Left((Some(Command::Bind(shortcuts, reply)), _)) => {
                // A failed bind keeps the session bound before.
                let result = replace(&mut bound, bind(&portal, shortcuts).await).await;
                if result.is_ok() {
                    on_bound(bound_ids(shortcuts));
                }
                reply.map(|reply| reply.send(result));
            }
            Either::Left((None, _)) => break,
            Either::Right((Some(press), _)) => {
                let Some((_, ids)) = &bound else { continue };
                match ids.iter().position(|id| id == press.shortcut_id()) {
                    Some(0) => on_press(HotkeyAction::Save),
                    Some(_) => on_press(HotkeyAction::Load),
                    None => {}
                }
            }
            Either::Right((None, _)) => break,
        }
    }
    if let Some((session, _)) = bound {
        let _ = session.close().await;
    }
}

type Bound = (Session<GlobalShortcuts>, Vec<String>);

/// Keeps a newly bound session in place of the old one, which closes.
async fn replace(bound: &mut Option<Bound>, new: Result<Bound, String>) -> Result<(), String> {
    if let Some((old, _)) = bound.replace(new?) {
        let _ = old.close().await;
    }
    Ok(())
}

/// The ids for `shortcuts`, in action order: an unset one has an id no key
/// can match, which keeps the positions.
fn ids(shortcuts: Shortcuts) -> Vec<String> {
    let mut ids = Vec::new();
    for shortcut in shortcuts {
        let id = shortcut.map(|shortcut| shortcut_id(&shortcut, ids.is_empty())).unwrap_or_default();
        ids.push(id);
    }
    ids
}

/// The ids of the shortcuts that are set.
pub fn bound_ids(shortcuts: Shortcuts) -> Vec<String> {
    ids(shortcuts).into_iter().filter(|id| !id.is_empty()).collect()
}

/// Whether every one of `shortcuts`' ids was allowed before: recorded by
/// the host (`allowed`), or listed by the desktop in a session of its own
/// that lists and closes. Binding exactly those again needs no dialog.
async fn remembered(portal: &GlobalShortcuts, shortcuts: Shortcuts, allowed: &[String]) -> Result<bool, String> {
    let wanted = ids(shortcuts);
    if wanted.iter().all(|id| id.is_empty() || allowed.contains(id)) {
        return Ok(true);
    }
    let session =
        portal.create_session(Default::default()).await.map_err(|e| format!("the shortcuts portal failed: {e}"))?;
    let listed = portal.list_shortcuts(&session, Default::default()).await.and_then(|request| request.response());
    let _ = session.close().await;
    let listed = listed.map_err(|e| format!("the shortcuts portal failed: {e}"))?;
    Ok(wanted.iter().filter(|id| !id.is_empty()).all(|id| listed.shortcuts().iter().any(|s| s.id() == id)))
}

/// Binds `shortcuts` in a new session, giving up if the user doesn't
/// answer the desktop's dialog in time.
async fn bind(portal: &GlobalShortcuts, shortcuts: Shortcuts) -> Result<Bound, String> {
    let session =
        portal.create_session(Default::default()).await.map_err(|e| format!("the shortcuts portal failed: {e}"))?;
    let ids = ids(shortcuts);
    let new: Vec<NewShortcut> = shortcuts
        .into_iter()
        .zip(&ids)
        .zip(["Save a checkpoint", "Load the latest checkpoint"])
        .filter_map(|((shortcut, id), description)| {
            shortcut.map(|shortcut| NewShortcut::new(id, description).preferred_trigger(trigger(&shortcut).as_str()))
        })
        .collect();
    if new.is_empty() {
        return Ok((session, ids));
    }
    let request = async {
        portal.bind_shortcuts(&session, &new, None, Default::default()).await.and_then(|request| request.response())
    };
    let timer = async {
        async_io::Timer::after(ANSWER_TIMEOUT).await;
        None
    };
    let answer = match select(Box::pin(request.map(Some)), Box::pin(timer)).await {
        Either::Left((answer, _)) | Either::Right((answer, _)) => answer,
    };
    let error = match answer {
        Some(Ok(answer)) => {
            let missing =
                ids.iter().filter(|id| !id.is_empty()).find(|id| !answer.shortcuts().iter().any(|s| s.id() == *id));
            match missing {
                None => return Ok((session, ids)),
                Some(_) => "the desktop didn't bind every shortcut".to_string(),
            }
        }
        Some(Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled))) => {
            "the desktop's shortcut dialog was cancelled".into()
        }
        Some(Err(e)) => format!("the desktop refused the shortcuts ({e})"),
        None => "the desktop's shortcut dialog wasn't answered in time".into(),
    };
    let _ = session.close().await;
    Err(error)
}

/// `save:CTRL+F5` or `load:CTRL+F9`: unique per key (see the module docs).
fn shortcut_id(shortcut: &Shortcut, save: bool) -> String {
    format!("{}:{}", if save { "save" } else { "load" }, trigger(shortcut))
}

/// The shortcut in the XDG shortcuts format: `CTRL+ALT+F5`, `LOGO+s`.
fn trigger(shortcut: &Shortcut) -> String {
    let mut text = String::new();
    for (on, name) in
        [(shortcut.ctrl, "CTRL+"), (shortcut.alt, "ALT+"), (shortcut.shift, "SHIFT+"), (shortcut.meta, "LOGO+")]
    {
        if on {
            text.push_str(name);
        }
    }
    match shortcut.key {
        Key::Function(number) => text.push_str(&format!("F{number}")),
        Key::Letter(letter) => text.push(letter.to_ascii_lowercase()),
        Key::Digit(digit) => text.push(digit),
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triggers_use_the_xdg_shortcut_format() {
        let [save, load] = Shortcut::defaults();
        assert_eq!(trigger(&save.unwrap()), "CTRL+F5");
        assert_eq!(shortcut_id(&load.unwrap(), false), "load:CTRL+F9");
        let shortcut = Shortcut::parse("Ctrl+Alt+Shift+Meta+S").unwrap();
        assert_eq!(trigger(&shortcut), "CTRL+ALT+SHIFT+LOGO+s");
        assert_eq!(trigger(&Shortcut::parse("Alt+7").unwrap()), "ALT+7");
    }

    #[test]
    fn ids_keep_their_positions_when_one_is_cleared() {
        let [save, load] = Shortcut::defaults();
        assert_eq!(ids([save, load]), ["save:CTRL+F5", "load:CTRL+F9"]);
        assert_eq!(ids([None, load]), ["", "load:CTRL+F9"]);
        assert_eq!(ids([save, None]), ["save:CTRL+F5", ""]);
        assert_eq!(bound_ids([None, load]), ["load:CTRL+F9"]);
    }
}
