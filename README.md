# SaveScummer

[![Release](https://github.com/neochief/savescummer/actions/workflows/release.yml/badge.svg)](https://github.com/neochief/savescummer/actions/workflows/release.yml)

SaveScummer keeps checkpoints of a game's on-disk saves so you can return to earlier progress. It does not modify the game or its memory.

## How to use it

1. Open SaveScummer and select a detected game. Use **Scan for games** or **Add custom game** if it is missing.
2. Play, then quit the game normally so it writes its progress to disk.
3. Press **Save** to make a checkpoint. Return to the game and keep playing.
4. To return to that point, quit the game, **Load** the checkpoint from history, then launch the game again.

Saving and loading while a game runs is disabled by default. A per-game Expert mode allows it when you know the game can handle it. The built-in catalog lists about 100 games, but most have not yet been checked with a real save-and-restore run; a listed game's save location may still need correction.

[Install and checkpoint locations](docs/install.md) · [Build and automated checks](docs/building.md) · [Manual testing](docs/testing.md)

## CLI

The included CLI talks to the same host as the window. Save and Load require the game to be closed by default. `games` shows the game IDs to use in commands.

| Platform | CLI command |
| --- | --- |
| Windows | `%LOCALAPPDATA%\Programs\SaveScummer\bin\SaveScummer.CLI.exe` |
| macOS | `/Applications/SaveScummer.app/Contents/MacOS/SaveScummer.CLI` |
| Linux | `<AppImage path> cli` |
| Development build | `./target/debug/savescummer-cli --data-dir .runtime/dev` |

For example, on macOS, make a labeled FTL checkpoint after quitting the game. After playing further and quitting again, load that checkpoint:

```sh
cli=/Applications/SaveScummer.app/Contents/MacOS/SaveScummer.CLI
"$cli" games
"$cli" save steam-212680 --label "Before the flagship"
"$cli" history steam-212680
"$cli" load steam-212680
```

To load an older checkpoint, copy its ID from the `history` output (shown in brackets) and pass it with `--checkpoint`:

```sh
"$cli" load steam-212680 --checkpoint "cp-0123456789abcdef0123456789abcdef"
```

The ID above is an example; use one from your own history. On Windows PowerShell, the same commands use the installed executable:

```powershell
$cli = "$env:LOCALAPPDATA\Programs\SaveScummer\bin\SaveScummer.CLI.exe"
& $cli games
& $cli save steam-212680 --label "Before the flagship"
& $cli history steam-212680
```

An agent can inspect a running Linux host without starting another one. Replace the AppImage filename with the version you downloaded:

```sh
appimage=./SaveScummer-linux-x86_64-0.3.1.AppImage
"$appimage" cli --no-start --json games
"$appimage" cli --no-start --json history steam-212680 --all
"$appimage" cli --no-start --json save-set steam-212680
```

`games` returns a JSON array; `history --all` emits one JSON row per line. `--no-start` makes the command fail if the host is not running. Run your platform's CLI command with `--help` for more options.
