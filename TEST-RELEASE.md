# Release smoke test

Run this for the Windows or Apple Silicon Mac release asset. Linux AppImages have local build paths but no CI or release job yet; use the [Linux acceptance checks](PLAN-BUILD.md#linux-release-target) before adding them to this release test. Use Steam and a disposable **FTL: Faster Than Light** run. Record the release version and mark each step pass or fail.

Save shortcut: **Ctrl+F5** on Windows; **⌥F5** on Mac.

## Prepare and install

1. Remove the old SaveScummer install:
   - **Windows:** Tray icon → **Exit**; then *Settings → Apps → SaveScummer → Uninstall*.
   - **Mac:** Turn off **Launch on startup**; menu bar icon → **Exit**; move `SaveScummer.app` from Applications to Trash.
2. Confirm no SaveScummer process or installed app remains.
3. Open [GitHub Releases](https://github.com/neochief/savescummer/releases). Download this version's Windows `setup.exe` or Mac `.dmg`. Stop if your platform has no release asset.
4. Install and launch:
   - **Windows:** Run setup with **Launch at sign-in** checked.
   - **Mac:** Open the DMG, drag the app to Applications, and open it.
   - **Check:** The main window and tray/menu-bar icon appear.

## Check launch behavior

5. Close the main window. The icon should remain; **Main window** from its menu should reopen the UI.
6. Run `%LOCALAPPDATA%\Programs\SaveScummer\bin\SaveScummer.exe` on Windows, or open `SaveScummer.app` on Mac. Confirm only one UI window appears.
7. Icon → **Exit**. Launch the app again; the UI should return.
8. Enable **Launch on startup** if needed, then sign out and back in. The icon should appear **without** a window. Launch the app normally; the window should open.

## Check an FTL save and restore

9. In Steam, install FTL if needed and wait for it to finish. Start a disposable run. Confirm FTL appears under **RUNNING** and Save is unavailable while it runs.
10. **Write down the run's sector and beacon (position A).** Save and quit FTL, then exit the game completely.
11. Select FTL in SaveScummer. Press **SAVE**, then press the Save shortcut with the UI focused. Confirm two checkpoints appear in history.
12. Relaunch FTL and advance to a different position (B). Save and quit FTL; exit completely.
13. Close the SaveScummer window and press the Save shortcut with no UI open. Reopen it and confirm a new checkpoint appears.
14. With FTL closed, load the **first** checkpoint from history. Relaunch FTL. **Pass:** the run resumes at position A.
