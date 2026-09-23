# Windows packaging

Everything Windows-specific for building distributions lives here.

- `licenses/` — license notices (Qt LGPL/GPL) bundled into every portable
  package and the installer.
- `portable/` — helper scripts copied into the portable ZIP so that copy can
  register or unregister the Explorer context menu without the installer.
- `installer/` — the Inno Setup definition (`savescummer.iss`) for the per-user
  1.0 installer: installs `SaveScummer`, `SaveScummer.Host`, `SaveScummer.CLI`
  and `savescummer-explorer.dll` to `%LOCALAPPDATA%\Programs\SaveScummer`,
  registers the Explorer extension (on by default; the classic handler appears
  under **Show more options** on Windows 11), enables the sign-in entry on a
  first install (task `checkedonce`, so upgrades keep a user's opt-out), and
  ships an uninstaller that preserves `%LOCALAPPDATA%\SaveScummer`.
  The extension DLL uses `restartreplace`, because `explorer.exe` keeps a loaded
  DLL mapped and Windows cannot overwrite it in place.

The portable ZIP is assembled by `scripts/package-windows.ps1`, which uses the
platform-neutral core in `scripts/package-common.ps1`; the installer is compiled
by `scripts/build-installer.ps1` from that staged payload. macOS and Linux will
add their own `packaging/<os>/` folders when those platforms qualify.

The Explorer CLSID and registry layout are defined in
`integrations/windows-explorer/identity.h`, `scripts/register-explorer.ps1` and
the `ExplorerClsid` define in `installer/savescummer.iss`. Keep all three in sync.