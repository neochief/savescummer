# setup-innosetup.ps1 — installs Inno Setup 6 through winget when ISCC.exe is
# not already available. Scripts never silently download SDKs; this command is
# explicit and only runs when invoked.
$ErrorActionPreference = 'Stop'

$existing = Get-Command iscc -ErrorAction SilentlyContinue
if ($existing) {
    Write-Output "Inno Setup already available: $($existing.Source)"
    exit 0
}
$candidates = @()
if ($env:ProgramFiles) { $candidates += Join-Path $env:ProgramFiles 'Inno Setup 6/ISCC.exe' }
if (${env:ProgramFiles(x86)}) { $candidates += Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6/ISCC.exe' }
if ($env:LOCALAPPDATA) { $candidates += Join-Path $env:LOCALAPPDATA 'Programs/Inno Setup 6/ISCC.exe' }
foreach ($candidate in $candidates) {
    if (Test-Path -LiteralPath $candidate) {
        Write-Output "Inno Setup already installed: $candidate"
        Write-Output 'The build scripts detect this location; no PATH change is needed.'
        exit 0
    }
}

if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
    throw 'winget is not available. Install Inno Setup 6 from https://jrsoftware.org/isdl.php, then retry.'
}
Write-Host 'Installing Inno Setup 6 with winget.'
winget install --id JRSoftware.InnoSetup -e --accept-package-agreements --accept-source-agreements
if ($LASTEXITCODE -ne 0) { throw 'winget could not install Inno Setup 6.' }
Write-Output 'Inno Setup installed. The build scripts detect the per-user install location; run scripts/build-installer.ps1.'