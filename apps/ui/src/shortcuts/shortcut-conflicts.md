# Shortcut conflict list

`shortcut-conflicts.json` is shared by the Settings UI and the Rust host. Each key uses the host's canonical modifier order: `Ctrl`, `Alt`, `Shift`, `Meta`, then one key. `Meta` is Command on macOS and Win on Windows. A `major` entry blocks an override; a `minor` entry warns while allowing the user to continue.

The list is a cleaned copy of the supplied `keys.csv`, with grouped combinations expanded and common capturable shortcuts added. It includes only F1–F12, A–Z, and 0–9 because those are the keys the current shortcut recorder and host support. Entries such as Tab, Escape, arrows, Space, punctuation, Print Screen, media keys, and modifier-only shortcuts were left out rather than silently pretending the app can register them. The Finder description for Command-F was corrected to Find, and Control-F3 was omitted because it focuses the Dock, rather than showing the desktop.

References for additions and descriptions: [Microsoft Windows shortcuts](https://support.microsoft.com/en-us/windows/keyboard-shortcuts-in-windows-dcc61a57-8ff0-cffe-9796-cb9706c75eec) and [Apple Mac shortcuts](https://support.apple.com/en-us/102650). Some shortcuts depend on OS version, settings, keyboard, or foreground app. The host still reports registration failures when another app or the OS owns a key combination.
