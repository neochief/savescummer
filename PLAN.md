# Save Scummer

I want to create a Windows app that would help me save and load backups of game save files or data, mainly for roguelike games.

The app's data model should have a library of known games and the directory (we'll call it DIR) to back up for each game (basically, either the save game dir or the entire game data dir if the game is clever enough to prevent tampering with the saves).

Ideally, this app should be cross-platform and work on Windows, Linux and macOS.


## SCAN, GAME LIBRARY, KNOWN GAMES

When the app is launched, it should perform a quick scan for gaming platforms and games that are present on the user's computer. Found platforms and games are displayed in the main app window's list. The scan is performed each time the app starts and periodically (once every 15 minutes).

## Game platforms

### Steam

%STEAMAPPS% (for example: "C:\Program Files (x86)\Steam\steamapps\common")

This should be detected first, so that patterns like %STEAMAPPS% can be expanded when the scan for games is being run.

## Game library

Our library should have known data about each game that would let the app detect whether it's installed or running.

- Game name (e.g. "Void War")
- Game icon (path?)
- Patterns to look for the install location (e.g. "%STEAMAPPS%\Void War")
- Patterns to look for the save/data DIR (e.g. "%APPDATA%\Void War")

Any game that was successfully found should have a new entry in the KNOWN GAMES data structure that is filled with:

- Game name
- Game icon
- Path to the game executable
- Path to the game's data DIR

## MONITOR and ACTIVE STACK

After the app finishes the initial scan, it should track the launch, activation and termination of the KNOWN GAMES executables in the central data structure called ACTIVE STACK. It would help with displaying the relevant information in the game's widget in the app's UI and also communicate which game to operate on when the global shortcuts are used.

There should be a focus stack of the launched games, and the shortcuts should only work with the latest game in the stack. There should be just one element per launched game in the stack. Two processes for the same game count as one in the stack. If all game processes are terminated, the entry is removed from the stack. If the game is focused, it moves to the top of the stack.

For example, I have FTL launched. The stack contains just that game. Now I launch Void War and it gets focused. The app detects that and adds Void War to the stack. Now if I alt-tab to FTL, FTL moves to the top of the stack. If I exit FTL, it's removed from the stack. Void War is on top of the stack, even though it may currently be in the foreground.

When the game is terminated, its temp data (such as UNDO PATH) is reset, and all undo dirs are removed.


## SAVE

When save is triggered, I want the app to:

1. Check if the game DIR exists. If not, finish the SAVE operation.

2. Create a copy of that dir as a new folder following the OS conventions for duplicate dirs (for example, for Windows, it's "Void_War - Copy", "Void_War - Copy (2)" and so on). There can be multiple copies of the DIR; this is expected.

3. Reset UNDO PATH (so that any undo for load in the UI disappears).


## LOAD

When load is triggered, I want the app to:

1. Check if there are any copies of the DIR. If there are none, do nothing and finish LOAD.

2. Create an immediate copy of the current DIR (with a ".undo_YYYY-MM-DD_H:i:s" datetime suffix). Save the path to that undo copy as the latest UNDO PATH for that given game.

3. After the backup is created, replace the original DIR with a copy of its latest backup, unless a specific backup is passed to load as an argument. Keep the backup; I might be loading from it again and again.


## Global shortcuts

Ctrl+F5 (SAVE operation)
Ctrl+F9 (LOAD operation)


## Explorer extension

I also want to register two Explorer extensions that would show up in the Explorer context menu:

Save - it should only appear if I right-clicked on DIR; it should do the save operation.
Load - it should only appear if I right-clicked on one of the DIR copies; it should do the load operation using that backup.


## UI

### Main window

It should show a scrollable list of widgets, one per game, for games that are in our library and that are detected on this computer. If there are no installed games, the list is empty and shows the text "No known games installed on this computer." The list shows launched games in the ACTIVE STACK first in the proper order, and then all installed known games. The idea is to have the launched active games on top. If the game is closed, it leaves ACTIVE STACK. If a scan finds that the game is no longer installed, the entry should disappear.

Each game widget should show the following:

-----------------------------------------------
ICON + NAME (Running)

|----| |---------------|---| |---------|
|Save| |      Load     | ↓ | | Explore |
|----| [ 4 seconds ago ]---| |---------| [...]
-----------------------------------------------


The bottom row contains buttons.


### Save

Save is a button that does the SAVE operation.


### Load

Load is a button with a dropdown section.

The dropdown lists all the available backups, latest first, as:

- 4 seconds ago
- 2 minutes ago
- 1 hour and 12 minutes ago
- yesterday, 23:20:12
- yesterday, 12:22:12
- Wednesday, 12:23:22
- 2012-12-12, 12:12:21

The idea here is to list backups in the most convenient way, so that one could distinguish between recent saves. The cutoffs:

- Seconds ago
- Minutes ago
- Hours and minutes ago
- yesterday, time
- last 7 days as days of the week, time
- full date and time

The most recent option should be shown under the main Load caption in smaller, subtler type.

The whole Load control is disabled if there are no backups.

If an UNDO PATH is available, a small UNDO button should appear centered under the Load button. It would let you remove the current DIR and replace it with the contents of the most recent UNDO PATH (but only if it exists). The UNDO PATH dir is then removed and UNDO PATH is reset.


### Explore

Should open the dir containing the DIR in the OS file explorer.


### ...

Should open a dropdown with miscellaneous actions:

- Configure

    Should open a small popup where you would see:

Game executable: [...prefilled path...][open icon] [Reset]
Game backup dir: [...prefilled path...][open icon] [Reset]

[Save] [Cancel]

Changing the path and saving would update the entry in KNOWN GAMES.


- Flush backups

    Should be disabled if there are no backups.

    Should display a confirmation that says: "This will remove all X backups. The most recent is **@**." Here, @ is the date in the same format as in the dropdown. If confirmed, all the game backups and any stray undos should be removed.


## Autolaunch

Above the main window list, there should be a checkbox:  [X] Start with the system

The app can be launched minimized with the "--minimized" flag, in which case it starts minimized to the tray.

By default, if you launch the app, the main window is shown and focused. When you close the app, it's minimized to the tray.

Clicking on the tray icon shows the main window.

The tray icon's context menu has two items:

- Main window
- Exit


# OS specifics

## Naming conventions for directory copies

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
