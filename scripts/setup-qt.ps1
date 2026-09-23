# setup-qt.ps1 — installs the pinned Qt SDK the desktop build uses, on
# Windows, macOS and Linux, with aqtinstall (the command-line installer behind
# the Qt online installer).
#
#   ./scripts/setup-qt.ps1                  # pinned default version and kit for this OS
#   ./scripts/setup-qt.ps1 -Version 6.5.3
#   ./scripts/setup-qt.ps1 -Arch win64_msvc2019_64
#   ./scripts/setup-qt.ps1 -Force           # reinstall over an existing kit
#
# The SDK lands in .runtime/Qt/<version>/<kit> — the location build.ps1 and
# build-desktop.ps1 already prefer, and the location the CI workflow caches.
# The script is idempotent: when that kit is already present it does nothing.
#
# Python 3.8+ is required; the script creates its own virtualenv under
# .runtime/qt-tools/venv and never installs into the system Python. CMake 3.21+
# is still required for the desktop build (PATH or .runtime/qt-tools/cmake).
param(
    [string]$Version = '6.5.3',
    [string]$Arch,
    [string]$Destination,
    [switch]$Force
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path "$PSScriptRoot/..").Path

if ($IsWindows) {
    $hostOs = 'windows'
    $defaultArch = 'win64_msvc2019_64'
    $qmake = 'bin/qmake.exe'
} elseif ($IsMacOS) {
    $hostOs = 'mac'
    $defaultArch = 'clang_64'
    $qmake = 'bin/qmake'
} elseif ($IsLinux) {
    $hostOs = 'linux'
    $defaultArch = 'gcc_64'
    $qmake = 'bin/qmake'
} else {
    throw 'Unsupported operating system.'
}
if (-not $Arch) { $Arch = $defaultArch }

# aqtinstall names Windows kits win64_msvc2019_64; the Qt online installer uses
# msvc2019_64, and build.ps1's default -QtPrefix expects that name. Install
# with the aqt name and normalize the directory afterwards.
$kit = $Arch
if ($IsWindows -and $Arch -match '^(?:win32|win64)_(.+)$') { $kit = $Matches[1] }

if (-not $Destination) { $Destination = Join-Path $root '.runtime/Qt' }
New-Item -ItemType Directory -Force -Path $Destination | Out-Null
$Destination = (Resolve-Path -LiteralPath $Destination).Path
$qtDirectory = Join-Path $Destination "$Version/$kit"
$aqtDirectory = Join-Path $Destination "$Version/$Arch"
$qmakePath = Join-Path $qtDirectory $qmake

if (-not $Force -and (Test-Path -LiteralPath $qmakePath)) {
    Write-Output "Qt $Version ($Arch) is already installed at $qtDirectory"
    Write-Output 'Pass -Force to reinstall it.'
    exit 0
}

# --- Python ---------------------------------------------------------------
# Prefer the py launcher and versioned interpreters. Skip the Microsoft Store
# execution-alias stubs under WindowsApps: they do not run Python.
$candidates = @()
foreach ($name in @('py', 'python3', 'python')) {
    $command = Get-Command $name -ErrorAction SilentlyContinue
    if (-not $command -or $command.Source -like '*\WindowsApps\*') { continue }
    $prefix = if ($name -eq 'py') { @('-3') } else { @() }
    $candidates += [pscustomobject]@{ Path = $command.Source; Prefix = $prefix }
}
foreach ($command in @(Get-Command 'python3.*' -CommandType Application -ErrorAction SilentlyContinue)) {
    if ($command.Source -like '*\WindowsApps\*' -or $command.Name -notmatch '^python3\.\d+$') { continue }
    if ($candidates.Path -notcontains $command.Source) {
        $candidates += [pscustomobject]@{ Path = $command.Source; Prefix = @() }
    }
}
$python = $null
$pythonPrefix = @()
foreach ($candidate in $candidates) {
    $probe = & $candidate.Path @($candidate.Prefix) -c 'import sys; print("%d.%d" % sys.version_info[:2])' 2>$null
    if ($LASTEXITCODE -eq 0 -and $probe -match '^(\d+)\.(\d+)$' -and
        ([int]$Matches[1] -gt 3 -or ([int]$Matches[1] -eq 3 -and [int]$Matches[2] -ge 8))) {
        $python = $candidate.Path
        $pythonPrefix = @($candidate.Prefix)
        break
    }
}
if (-not $python) {
    throw 'Python 3.8 or newer was not found. Install it from https://www.python.org/downloads/ and retry.'
}

# --- aqtinstall virtual environment --------------------------------------
$venvDirectory = Join-Path $root '.runtime/qt-tools/venv'
$venvPython = Join-Path $venvDirectory $(if ($IsWindows) { 'Scripts/python.exe' } else { 'bin/python' })
if (-not (Test-Path -LiteralPath $venvPython)) {
    Write-Host 'Creating the aqtinstall virtual environment under .runtime/qt-tools/venv.'
    New-Item -ItemType Directory -Force -Path (Join-Path $root '.runtime/qt-tools') | Out-Null
    & $python @pythonPrefix -m venv $venvDirectory
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $venvPython)) {
        throw 'Could not create the Python virtual environment for aqtinstall.'
    }
}
& $venvPython -m pip install --disable-pip-version-check --quiet aqtinstall
if ($LASTEXITCODE -ne 0) { throw 'Could not install aqtinstall with pip.' }

# --- Qt -------------------------------------------------------------------
Write-Host "Installing Qt $Version ($Arch) with aqtinstall into $qtDirectory."
Write-Host 'This downloads the official Qt archives from download.qt.io.'
& $venvPython -m aqt install-qt $hostOs desktop $Version $Arch --outputdir $Destination
if ($LASTEXITCODE -ne 0) { throw 'aqtinstall failed to install Qt.' }
if ($aqtDirectory -ne $qtDirectory -and (Test-Path -LiteralPath (Join-Path $aqtDirectory $qmake))) {
    if (Test-Path -LiteralPath $qtDirectory) { Remove-Item -LiteralPath $qtDirectory -Recurse -Force }
    Move-Item -LiteralPath $aqtDirectory -Destination $qtDirectory
}
if (-not (Test-Path -LiteralPath $qmakePath)) {
    throw "aqtinstall finished but $qmakePath was not found."
}
$installed = (& $qmakePath -query QT_VERSION).Trim()
Write-Output "Qt $installed ($Arch) installed at $qtDirectory"
Write-Output "Desktop builds find it automatically; otherwise pass -QtPrefix '$qtDirectory'."

$cmake = Get-Command cmake -ErrorAction SilentlyContinue
if (-not $cmake -and -not (Test-Path -LiteralPath (Join-Path $root '.runtime/qt-tools/cmake/data/bin/cmake.exe'))) {
    Write-Warning 'CMake 3.21 or newer was not found on PATH; install it before building the desktop.'
}