# build-installer.ps1 — compiles packaging/windows/installer/savescummer.iss
# into dist/SaveScummer-windows-x64-<version>-setup.exe.
#
# The payload is the release portable folder produced by package-windows.ps1
# (dist/SaveScummer-windows-x64). Build it first with:
#     ./build.ps1 release
# or point -PayloadDirectory at another staged package.
param(
    [string]$PayloadDirectory,
    [string]$OutputDirectory,
    [string]$IsccPath
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path "$PSScriptRoot/..").Path
. (Join-Path $PSScriptRoot 'package-common.ps1')

$version = Get-AppVersion -CargoManifest (Join-Path $root 'Cargo.toml')
if (-not $PayloadDirectory) { $PayloadDirectory = Join-Path $root 'dist/SaveScummer-windows-x64' }
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $root 'dist' }
$payload = [IO.Path]::GetFullPath($PayloadDirectory)
if (-not (Test-Path -LiteralPath (Join-Path $payload 'bin/SaveScummer.exe'))) {
    throw "Release payload not found: $payload. Run ./build.ps1 release first."
}
if (-not (Test-Path -LiteralPath (Join-Path $payload 'bin/savescummer-explorer.dll'))) {
    throw "The payload does not include the Explorer extension: $payload. Rebuild with ./build.ps1 release."
}

if (-not $IsccPath) {
    $command = Get-Command iscc -ErrorAction SilentlyContinue
    if ($command) { $IsccPath = $command.Source }
}
if (-not $IsccPath) {
    $candidates = @()
    if ($env:ProgramFiles) { $candidates += Join-Path $env:ProgramFiles 'Inno Setup 6/ISCC.exe' }
    if (${env:ProgramFiles(x86)}) { $candidates += Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6/ISCC.exe' }
    if ($env:LOCALAPPDATA) { $candidates += Join-Path $env:LOCALAPPDATA 'Programs/Inno Setup 6/ISCC.exe' }
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate) { $IsccPath = $candidate; break }
    }
}
if (-not $IsccPath) {
    throw 'Inno Setup (ISCC.exe) was not found. Run ./scripts/setup-innosetup.ps1 or install Inno Setup 6.'
}

$installerName = Get-InstallerArtifactName -Os 'windows' -Arch 'x64' -Version $version
# Inno Setup appends .exe to OutputBaseFilename, so pass the stem.
$installerStem = [IO.Path]::GetFileNameWithoutExtension($installerName)
$output = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $output | Out-Null
$iss = Join-Path $root 'packaging/windows/installer/savescummer.iss'

Write-Host "Compiling the SaveScummer $version installer."
& $IsccPath "/DSaveScummerVersion=$version" "/DInstallerName=$installerStem" "/DPayloadDir=$payload" "/DOutputDir=$output" $iss
if ($LASTEXITCODE -ne 0) { throw 'Inno Setup compilation failed.' }

$installer = Join-Path $output $installerName
if (-not (Test-Path -LiteralPath $installer)) {
    throw "Inno Setup reported success but $installer was not produced."
}
Write-Output "Installer: $installer"
Write-Output ('Installer size: {0:N1} MiB' -f ((Get-Item -LiteralPath $installer).Length / 1MB))