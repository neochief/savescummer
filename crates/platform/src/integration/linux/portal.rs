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
    Bind(Shortcuts, Option<mpsc::Sender<Result<(), String>>>),
}

pub struct Portal {
    commands: async_mpsc::UnboundedSender<Command>,
}

/// Whether to try the portal: in a Wayland session. Under X11 the key
/// grabs work everywhere and need no dialog.
pub fn wanted() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some_and(|d| !d.is_empty())
}

/// Starts the portal thread and binds `shortcuts`. The receiver gets the
/// outcome of that first bind, which waits for the user if the desktop asks
/// (the first time). `on_press` runs on the portal thread for each press.
pub fn start(
    shortcuts: Shortcuts,
    on_press: fn(HotkeyAction),
) -> Result<(Portal, mpsc::Receiver<Result<(), String>>), String> {
    let (commands, commands_rx) = async_mpsc::unbounded();
    let (first_tx, first_rx) = mpsc::channel();
    let _ = commands.unbounded_send(Command::Bind(shortcuts, Some(first_tx)));
    std::thread::Builder::new()
        .name("shortcuts-portal".into())
        .spawn(move || async_io::block_on(run(commands_rx, on_press)))
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

    /// Binds `shortcuts` once the portal is free, without waiting.
    pub fn bind_later(&self, shortcuts: Shortcuts) {
        let _ = self.commands.unbounded_send(Command::Bind(shortcuts, None));
    }
}

/// The portal thread. It ends when [`Portal`] is dropped (the channel
/// closes), closing the session, which releases the shortcuts.
async fn run(mut commands: async_mpsc::UnboundedReceiver<Command>, on_press: fn(HotkeyAction)) {
    let _ = ashpd::register_host_app(APP_ID.parse().expect("a valid app id")).await;
    let setup = async {
        let portal = GlobalShortcuts::new().await?;
        let activated = portal.receive_activated().await?;
        Ok::<_, ashpd::Error>((portal, activated))
    };
    let (portal, mut activated) = match setup.await {
        Ok(setup) => setup,
        Err(e) => {
            // Answer every waiting bind, then stop.
            let error = format!("the desktop has no shortcuts portal ({e})");
            commands.close();
            while let Some(Command::Bind(_, reply)) = commands.next().await {
                reply.map(|reply| reply.send(Err(error.clone())));
            }
            return;
        }
    };
    // The bound session and its shortcut ids, in action order.
    let mut bound: Option<(Session<GlobalShortcuts>, Vec<String>)> = None;
    loop {
        match select(commands.next(), activated.next()).await {
            Either::Left((Some(Command::Bind(shortcuts, reply)), _)) => {
                let first = bound.is_none();
                let result = match bind(&portal, shortcuts, !first).await {
                    Ok(new) => {
                        if let Some((old, _)) = bound.replace(new) {
                            let _ = old.close().await;
                        }
                        Ok(())
                    }
                    Err(e) => Err(e),
                };
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

/// Binds `shortcuts` in a new session. With `timeout`, gives up if the user
/// doesn't answer the desktop's dialog in time.
async fn bind(
    portal: &GlobalShortcuts,
    shortcuts: Shortcuts,
    timeout: bool,
) -> Result<(Session<GlobalShortcuts>, Vec<String>), String> {
    let session =
        portal.create_session(Default::default()).await.map_err(|e| format!("the shortcuts portal failed: {e}"))?;
    let mut ids = Vec::new();
    let mut new = Vec::new();
    for (shortcut, description) in shortcuts.into_iter().zip(["Save a checkpoint", "Load the latest checkpoint"]) {
        // An id no key can match keeps the positions when one is unset.
        let Some(shortcut) = shortcut else {
            ids.push(String::new());
            continue;
        };
        let id = shortcut_id(&shortcut, ids.is_empty());
        new.push(NewShortcut::new(&id, description).preferred_trigger(trigger(&shortcut).as_str()));
        ids.push(id);
    }
    if new.is_empty() {
        return Ok((session, ids));
    }
    let request = async {
        portal.bind_shortcuts(&session, &new, None, Default::default()).await.and_then(|request| request.response())
    };
    let answer = if timeout {
        let timer = async {
            async_io::Timer::after(ANSWER_TIMEOUT).await;
            None
        };
        match select(Box::pin(request.map(Some)), Box::pin(timer)).await {
            Either::Left((answer, _)) | Either::Right((answer, _)) => answer,
        }
    } else {
        Some(request.await)
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
}
