# Save Scummer

I want to create a windows app that would let me help to save and load backups of the game save files or data, mainly for roguelike games.

The app data model should have a library of known games and what directory (we'll call it DIR) for this game to backup (basically, either the save game dir, or the entire game data dir if the game is clever enough to prevent tampering with the saves).

Ideally, this app should be cross platform and work on both Windows, Linux and macOS.


## SCAN, GAME LIBRARY, KNOWN GAMES

When the app is launched, it should perfom a quick scan for gaming platforms and games that are present on the user's computer. Found platforms and games are displayed in the main app windows list. The scan is performaed each time the app starts and periodically (once in 15 minutes).

## Game platforms

### Steam

%STEAMAPPS% (for example: "C:\Program Files (x86)\Steam\steamapps\common")

This should be detected first, so that the patterns like %STEAMAPPS% could be expanded when the scan for games is being run.

## Game library

Our library should have known data about the game that would let the app to detect whether it's installed or running.

- Game name (e.g. "Void War")
- Game icon (path?)
- Patterns to look for the install location (e.g. "%STEAMAPPS%\Void War")
- Patterns to look for the save/data DIR (e.g. "%APPDATA%\Void War")

Any game that was successfully found should have a new real entry in the KNOWN GAMES data structure that is filled with:

- Game name
- Game icon
- Path to the game executable
- Path to the game's data DIR

## MONITOR and ACTIVE STACK

After the app finished the initial scan, it should track launching, activation and finishing of the KNOWN GAMES executables in the central data structure called ACTIVE STACK. It would help with displaing the relevant information in the app's UI in the game's widget and also communicate which game to operate on when the global shortcuts are called.

There should be the focus stack of the launched games, and the shortcuts should only work with the latest game in the stack. There should be just one element per launched game in the stack.Two game processes couns as one in the stack. If all game processes are terminated, the entry is removed from the stack.  If the game is focusd, it's getting to the top of the stack.

For example, I have FTL launched. The stack contains just that game. Now I launch Void War and it's getting focsed. The app detects that and adds Void War to the stack. Now if I alt-tab to FTL, FTL is added on top of the stack. If I exit FTL, it's removed from the stack, Void War is on top of the stack, even though it may be currently in foreground.

When the game is terminated, it's temp data (such as UNDO PATH) is reset, all undo dirs are removed.


## SAVE 

When save is triggered, I want the app to:

1. Check if the game DIR exists, if not, finish the SAVE operation.

2. Create a copy of that dir as a new folder following the OS conventions for duplicate dirs (for example, for WIndows, it's "Void_War - Copy", "Void_War - Copy (2)" and so on). There can be multiple copies of the DIR, this is expected.

3. Reset UNDO PATH (so that any undo for load in ui dissapears)


## LOAD

When load is triggered, I want the app to:

1. Check if there's no copies of the DIR and do nothing and finish LOAD if there's no copies.

2. Create immedite copy of the current DIR (with ".undo_YYYY-MM-DD_H:i:s" datetime suffix). Save the path to that undo copy as the latest UNDO PATH for that given game.

3. After backup is created, replace the original DIR with the copy of it's latest copy, unless the exact backup is passed to the load as argument. Keep the copy, I might be loading from it again and again.


## Global shortcuts

Ctrl+F5 (SAVE operation)
Ctrl+F9 (LOAD operation)


## Explorer extension

I also want to register two explorer extensions that would show up in the explorer context menu:

Save - it should only appear if I clicked over DIR, it should do the save operation.
Load - it should only appear if I clicked over one of the DIR copies, it should do the load operation over that backup.


## UI

### Main windows

It should show a scrollable list of widgets, one per games, for games that are in our library and that are detected on this computer. If there's no installed games, the list is empty and the text "No known games installed on this computer.". The list shows launched games in the ACTIVE STACK first in the proper order, and then all installed known games. The idea is to have the launched active games on top. If the game is closed (stops being in ACTIVE STACK, ) If after a scan the game is no longer installed, the entry should disappear.

Each game widget should show the following:

-----------------------------------------------
ICON + NAME (Running)

|----| |---------------|---| |---------|
|Save| |      Load     | ↓ | | Explore |
|----| [ 4 seconds ago ]---| |---------| [...]
-----------------------------------------------


The bottom row is buttons.


### Save

Save is a button with does the SAVE operation.


### Load

Load is a button with a dropdown section.

Dropdown that lists all the available backups, latest on first as:

- 4 secons ago
- 2 minutes ago
- 1 hour and 12 minutes ago
- yesterday, 23:20:12
- yesterday, 12:22:12
- Wednesday, 12:23:22
- 2012-12-12, 12:12:21

The idea here is to list in the most convenient way, so that one could distinguish between recent saves. The cut offs:

- Seconds ago
- Minutes ago
- Hours and minutes ago
- yesterday, time
- last 7 days as days, time
- full date time

The most recent option should be shown under the main Load caption in smaller subtler type.

The whole load control is disabled if there's no backups.

If there's latest UNDO PATH available: a small UNDO button should appear under the Load button (centered under Load button) that would let you remove the current DIR and replace it with the contents of the most recent UNDO PATH (but only if it exists), the UNDO PATH dir is removed and the UNDO PATH is reset.


### Explore

Should open the dir containing the DIR in the OS file explorer.


### ...

Should open dropdown with misc actions:

- Configure

    Should open a small popup where you would see:

Game executable: [...prefilled path...][open icon] [Reset]
Game backup dir: [...prefilled path...][open icon] [Reset]

[Save] [Cancel]

Changing the path and saving, would update the entry in the KNOWN GAMES.


- Flush backups

    Should be disabled if there's no backups.

    Should display a confirmation that says: "This will remove all X backups, the most recent is **@**.", where @ is the date in the same format as in dropdown. If confirmed, all the game backups and any stray undos should be removed.


## Autolaunch

Above the main window list, there should be a checkbox:  [X] Start with the system

The app can be launched as minimized with "--minimized" flag, in which case it starts but is hidden to tray.

By default, if you launch the app, the main window is shown and focused. When you close the app, it's minimized to tray.

Clicking on tray icon, shows the main window.

The tray icon has two context menus:
- Main window
- Exit


# OS Specifics

## Dir copies file names convention

- Windows: "Void_War", "Void_War - Copy", "Void_War - Copy (2)"
- macOS: unknown
- Linux: unknown

## Global shortcuts

- Windows: possible
- macOS: unknown
- Linux: unknown


## File explorer, Finder extension or variants

- Windows: possible to extend the Explorer context menus
- macOS: unknown
- Linux: unknown