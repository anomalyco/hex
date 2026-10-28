# Silently installs, reinstalls, installs over another version with, and
# uninstalls a Hex-Setup-*.exe on a disposable Windows machine (CI), checking
# the managed layout that src/updater/windows.rs expects at every step. It
# uses the real %APPDATA%, so never run it on a machine with Hex installed.
#
#   scripts/test-windows-installer.ps1 -Installer dist\Hex-Setup-2.4.0-windows-x64.exe `
#     -Executable dist\Hex-2.4.0-windows-x64.exe -Version 2.4.0
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string]$Installer,
    [Parameter(Mandatory)] [string]$Executable,
    [Parameter(Mandatory)] [string]$Version
)
$ErrorActionPreference = 'Stop'

$support = Join-Path $env:APPDATA 'voice-control'
$installed = Join-Path $support "versions\$Version\hex.exe"
$pointer = Join-Path $support 'current-version'
$receipt = Join-Path $support 'install.json'
$shortcut = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Hex.lnk'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{8FCF0256-C55C-40C3-865D-5FADB32D13B3}_is1'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$logs = Join-Path $env:RUNNER_TEMP 'hex-installer-logs'
New-Item -ItemType Directory -Force $logs | Out-Null

function Check([bool]$condition, [string]$message) {
    if (-not $condition) { throw "FAIL: $message" }
    Write-Output "ok - $message"
}

function Run-Setup([string]$name) {
    $process = Start-Process $Installer -Wait -PassThru -ArgumentList @(
        '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SKIPVULKANCHECK=1', "/LOG=$logs\$name.log"
    )
    $process.ExitCode
}

if (Test-Path $support) { throw "$support already exists; run this only on a clean machine." }
$expected = (Get-FileHash -Algorithm SHA256 $Executable).Hash

Check ((Run-Setup 'install') -eq 0) 'silent install exits 0'
Check (Test-Path $installed) "hex.exe is at versions\$Version"
Check ((Get-FileHash -Algorithm SHA256 $installed).Hash -eq $expected) 'installed hex.exe matches the release bytes'
Check ((Get-Content $pointer -Raw).Trim() -eq $Version) "current-version names $Version"
$installReceipt = Get-Content $receipt -Raw | ConvertFrom-Json
Check ($installReceipt.app -eq 'hex' -and $installReceipt.artifact -eq 'desktop' -and $installReceipt.channel -eq 'stable' -and $installReceipt.method -eq 'direct') 'install.json marks a direct install of hex desktop stable'
Check ($installReceipt.root -eq $support) 'install.json names the install root'
$link = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut)
Check ($link.TargetPath -eq $installed) 'Start Menu shortcut targets the managed executable'
Check ($link.Arguments -eq 'app') 'Start Menu shortcut launches the app'
Check ($link.Description -eq 'HEX managed install') 'Start Menu shortcut carries the managed mark the updater repoints'
$entry = Get-ItemProperty $uninstallKey
Check ($entry.DisplayName -eq 'Hex') 'Apps & Features lists Hex'
Check ($entry.DisplayVersion -eq $Version) "Apps & Features shows $Version"
$uninstaller = ($entry.UninstallString -replace '"', '')
Check ($uninstaller -like "$support\uninstall\*") 'the uninstaller lives outside versions\, which updates prune'
Check (Test-Path $uninstaller) 'the uninstaller exists'

if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64' -or (Test-Path "$env:SystemRoot\System32\vulkan-1.dll")) {
    $reported = (& $installed --version) -split '\s+' | Select-Object -Last 1
    Check ($reported -eq $Version) "installed hex.exe reports $Version"
} else {
    Write-Output 'skip - hex.exe --version needs vulkan-1.dll, which this runner lacks'
}

Check ((Run-Setup 'reinstall') -eq 0) 'reinstalling the same version exits 0'
Check ((Get-FileHash -Algorithm SHA256 $installed).Hash -eq $expected) 'reinstall keeps the release bytes'

# Versions are never compared: the installer installs its own over any other.
Set-Content -Path $pointer -Value '99.0.0' -NoNewline
Check ((Run-Setup 'over-another') -eq 0) 'installing over another version exits 0'
Check ((Get-Content $pointer -Raw).Trim() -eq $Version) 'current-version names this installer''s version'

# Simulate what a self-update and Launch at login leave behind.
$updated = Join-Path $support 'versions\99.0.0\hex.exe'
New-Item -ItemType Directory -Force (Split-Path $updated) | Out-Null
Copy-Item $installed $updated
New-Item -ItemType Directory -Force (Join-Path $support 'updates') | Out-Null
if (-not (Test-Path $runKey)) { New-Item -Path $runKey | Out-Null }
Set-ItemProperty -Path $runKey -Name 'HEX' -Value "`"$updated`" app --hidden"
Set-Content -Path (Join-Path $support 'settings.json') -Value '{}'

$process = Start-Process $uninstaller -Wait -PassThru -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=$logs\uninstall.log"
Check ($process.ExitCode -eq 0) 'silent uninstall exits 0'
# The uninstaller finishes from a temporary copy of itself.
$deadline = (Get-Date).AddSeconds(60)
while ((Test-Path $uninstallKey) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
Check (-not (Test-Path $uninstallKey)) 'Apps & Features entry is removed'
Check (-not (Test-Path (Join-Path $support 'versions'))) 'every version, including self-updated ones, is removed'
Check (-not (Test-Path (Join-Path $support 'updates'))) 'staged updates are removed'
Check (-not (Test-Path $pointer)) 'current-version is removed'
Check (-not (Test-Path $receipt)) 'install.json is removed'
Check (-not (Test-Path $shortcut)) 'Start Menu shortcut is removed'
Check ($null -eq (Get-ItemProperty -Path $runKey -Name 'HEX' -ErrorAction SilentlyContinue)) 'Launch at login is removed'
Check (Test-Path (Join-Path $support 'settings.json')) 'settings are kept'
Remove-Item $support -Recurse -Force
Write-Output "Installer $([IO.Path]::GetFileName($Installer)) passed."
