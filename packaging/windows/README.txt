SaveScummer {version}
=====================

Save and load backups of game saves with a hotkey, so a bad run can be undone.

Programs (in bin\):

  SaveScummer.exe        SaveScummer itself: runs in the tray, watches games,
                         reacts to hotkeys, makes and restores backups
{ui}  SaveScummer.CLI.exe    the command-line client

Your data lives in %LOCALAPPDATA%\SaveScummer (checkpoints go in its
"checkpoints" folder unless you moved them). Installing, upgrading and
uninstalling never touch it: checkpoints are your saves.

The installer is not code-signed, so Windows SmartScreen may warn about it:
choose "More info", then "Run anyway".

Source code, issues and releases: https://github.com/neochief/savescummer

Third-party licenses are listed in THIRD-PARTY-LICENSES.html.
