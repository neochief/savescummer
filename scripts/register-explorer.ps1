param([string]$Dll, [switch]$Uninstall, [switch]$Dev)
$ErrorActionPreference = 'Stop'
$classId = '{3F8F42CE-463F-41B6-98D1-8C8D16B88931}'
$classKey = "HKCU:\Software\Classes\CLSID\$classId"
$handlerKey = 'HKCU:\Software\Classes\Directory\shellex\ContextMenuHandlers\SaveScummer'
if ($Dev) {
    $classId = '{43BFBA41-D0AB-44D3-A5D6-600EB5C74D18}'
    $classKey = "HKCU:\Software\Classes\CLSID\$classId"
    $handlerKey = 'HKCU:\Software\Classes\Directory\shellex\ContextMenuHandlers\SaveScummerDev'
}
if ($Uninstall) {
    foreach ($key in @($handlerKey, $classKey)) {
        if (Test-Path -LiteralPath $key) { Remove-Item -LiteralPath $key -Recurse }
    }
} else {
    if (-not $Dll) { throw 'Supply -Dll with the built savescummer-explorer.dll path.' }
    $resolvedDll = (Resolve-Path -LiteralPath $Dll).Path
    if ([IO.Path]::GetExtension($resolvedDll) -ne '.dll') { throw 'Expected a DLL file.' }
    New-Item -Path "$classKey\InprocServer32" -Force | Out-Null
    Set-Item -LiteralPath "$classKey\InprocServer32" -Value $resolvedDll
    New-ItemProperty -LiteralPath "$classKey\InprocServer32" -Name ThreadingModel -Value Apartment -PropertyType String -Force | Out-Null
    New-Item -Path $handlerKey -Force | Out-Null
    Set-Item -LiteralPath $handlerKey -Value $classId
}
Write-Output 'Explorer integration registration updated for the current user. New Explorer processes load the change.'
