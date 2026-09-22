# Windows packaging

Everything Windows-specific for building distributions lives here.

- `licenses/` — license notices (Qt LGPL/GPL) bundled into every portable
  package and the installer.
- `installer/` — (Phase B) the Inno Setup definition for the per-user
  1.0 installer: installs `SaveScummer`, `SaveScummer.Host`, `SaveScummer.CLI`,
  registers the Explorer extension (on by default), offers Launch on startup,
  and ships an uninstaller that preserves `%LOCALAPPDATA%\SaveScummer`.

The portable ZIP is assembled by `scripts/package-windows.ps1`, which uses the
platform-neutral core in `scripts/package-common.ps1`; macOS and Linux will add
their own `packaging/<os>/` folders when those platforms qualify.
