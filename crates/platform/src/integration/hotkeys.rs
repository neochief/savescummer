//! What macOS and Linux share about `global-hotkey`: the shortcut's native
//! form, registering a set with rollback, and mapping a press back to its
//! action. The managers, their threads and listeners stay in each OS file.

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use super::{HotkeyAction, Key, Shortcut, Shortcuts};

pub fn native_shortcut(shortcut: Shortcut) -> HotKey {
    let mut modifiers = Modifiers::empty();
    if shortcut.ctrl {
        modifiers |= Modifiers::CONTROL;
    }
    if shortcut.alt {
        modifiers |= Modifiers::ALT;
    }
    if shortcut.shift {
        modifiers |= Modifiers::SHIFT;
    }
    if shortcut.meta {
        modifiers |= Modifiers::SUPER;
    }
    let name = match shortcut.key {
        Key::Function(number) => format!("F{number}"),
        Key::Letter(letter) => format!("Key{letter}"),
        Key::Digit(digit) => format!("Digit{digit}"),
    };
    let code: Code = name.parse().expect("validated shortcut key");
    HotKey::new((!modifiers.is_empty()).then_some(modifiers), code)
}

/// Registers each set shortcut; one another app holds is reported and
/// skipped.
pub fn register(manager: &GlobalHotKeyManager, shortcuts: Shortcuts) -> Vec<String> {
    let mut errors = Vec::new();
    for shortcut in shortcuts.into_iter().flatten() {
        if let Err(e) = manager.register(native_shortcut(shortcut)) {
            errors.push(format!("{} is unavailable: another app already uses it ({e})", shortcut.canonical()));
        }
    }
    errors
}

/// Replaces `old` with `new`. If one of the new ones can't be registered,
/// `old` is registered again and nothing changed.
pub fn rebind(manager: &GlobalHotKeyManager, old: Shortcuts, new: Shortcuts) -> Result<(), String> {
    for shortcut in old.into_iter().flatten() {
        let _ = manager.unregister(native_shortcut(shortcut));
    }
    let mut registered = Vec::new();
    for shortcut in new.into_iter().flatten() {
        let hotkey = native_shortcut(shortcut);
        if let Err(error) = manager.register(hotkey) {
            for hotkey in registered {
                let _ = manager.unregister(hotkey);
            }
            for shortcut in old.into_iter().flatten() {
                let _ = manager.register(native_shortcut(shortcut));
            }
            return Err(format!("{} is unavailable: {error}", shortcut.canonical()));
        }
        registered.push(hotkey);
    }
    Ok(())
}

/// The action a press of one of `shortcuts` runs; releases run nothing.
pub fn action(event: GlobalHotKeyEvent, shortcuts: Shortcuts) -> Option<HotkeyAction> {
    if event.state != HotKeyState::Pressed {
        return None;
    }
    shortcuts
        .iter()
        .position(|shortcut| shortcut.is_some_and(|shortcut| native_shortcut(shortcut).id() == event.id))
        .map(|index| if index == 0 { HotkeyAction::Save } else { HotkeyAction::Load })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presses_map_to_their_actions_and_releases_to_none() {
        let shortcuts = Shortcut::defaults();
        let save = native_shortcut(shortcuts[0].unwrap()).id();
        let load = native_shortcut(shortcuts[1].unwrap()).id();
        let press = |id| GlobalHotKeyEvent { id, state: HotKeyState::Pressed };
        assert_eq!(action(press(save), shortcuts), Some(HotkeyAction::Save));
        assert_eq!(action(press(load), shortcuts), Some(HotkeyAction::Load));
        assert_eq!(action(GlobalHotKeyEvent { id: save, state: HotKeyState::Released }, shortcuts), None);
        assert_eq!(action(press(save), [None, shortcuts[1]]), None, "a cleared shortcut runs nothing");
    }
}
