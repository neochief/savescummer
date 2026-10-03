# Install SaveScummer

Download the package for your platform from [GitHub Releases](https://github.com/neochief/savescummer/releases).

| Platform | File | Install |
| --- | --- | --- |
| Windows 10/11 x64 | `SaveScummer-windows-x64-<version>-setup.exe` | Run the per-user installer; no administrator access is needed. |
| macOS 13+, Apple Silicon | `SaveScummer-macos-arm64-<version>.dmg` | Drag SaveScummer to Applications. It runs in the menu bar and may ask for access to a game's save folder. |
| Linux x86_64, glibc 2.35+ | `SaveScummer-linux-x86_64-<version>.AppImage` | Make the AppImage executable (`chmod +x <file>.AppImage`) and run it. |
| Linux ARM64, glibc 2.35+ | `SaveScummer-linux-aarch64-<version>.AppImage` | Make the AppImage executable and run it. |

The Linux AppImage includes the host, UI and CLI; it needs no installation or root access.

## Upgrade or remove

- **Windows:** run the new installer to upgrade, or uninstall SaveScummer in Settings → Apps.
- **macOS:** quit from the menu bar, then drag the new app over the old one to upgrade. To remove it, turn off launch at login, quit, then move the app to Trash.
- **Linux:** quit from the tray and run the new AppImage. To remove it, turn off launch at login, quit, then delete the AppImage.

## Checkpoint storage

Checkpoints are stored in the `checkpoints` folder under SaveScummer's app data directory unless you move the checkpoint store in Settings:

| Platform | App data directory |
| --- | --- |
| Windows | `%LOCALAPPDATA%\SaveScummer` |
| macOS | `~/Library/Application Support/SaveScummer` |
| Linux | `~/.local/share/SaveScummer` |

Installing, upgrading, or uninstalling SaveScummer does not delete your checkpoints.
