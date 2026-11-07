# Silently installs, reinstalls, installs over another version with, and
# uninstalls a hex-setup-*.exe on a disposable Windows machine (CI), checking
# the managed layout that src/updater/windows.rs expects at every step. It
# uses the real %APPDATA%, so never run it on a machine with Hex installed.
#
#   scripts/test-windows-installer.ps1 -Installer dist\hex-setup-2.4.0-windows-x64.exe `
#     -Executable dist\hex-2.4.0-windows-x64.exe -Console dist\hex-2.4.0-windows-x64.com -Version 2.4.0
#
# -Console (hex.com) is left out only for a release published before it.
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string]$Installer,
    [Parameter(Mandatory)] [string]$Executable,
    [string]$Console,
    [Parameter(Mandatory)] [string]$Version
)
$ErrorActionPreference = 'Stop'

$support = Join-Path $env:APPDATA 'hex'
$installed = Join-Path $support "versions\$Version\hex.exe"
$installedConsole = Join-Path $support "versions\$Version\hex.com"
$bin = Join-Path $support 'bin'
$pointer = Join-Path $support 'current-version'
$receipt = Join-Path $support 'install.json'
$shortcut = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Hex.lnk'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{6ED48C92-5177-41F4-B8D6-37B7B02CFDF4}_is1'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$logs = Join-Path $env:RUNNER_TEMP 'hex-installer-logs'
New-Item -ItemType Directory -Force $logs | Out-Null

function Check([bool]$condition, [string]$message) {
    if (-not $condition) { throw "FAIL: $message" }
    Write-Output "ok - $message"
}

function Run-Setup([string]$name) {
    $process = Start-Process $Installer -Wait -PassThru -ArgumentList @(
        '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=$logs\$name.log"
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
# Hex reads the receipt as UTF-8; Setup once wrote the ANSI code page (#1352).
$receiptBytes = [IO.File]::ReadAllBytes($receipt)
Check (-not ($receiptBytes.Length -ge 3 -and $receiptBytes[0] -eq 0xEF -and $receiptBytes[1] -eq 0xBB -and $receiptBytes[2] -eq 0xBF)) 'install.json has no byte order mark'
$strict = New-Object System.Text.UTF8Encoding($false, $true)
Check ($null -ne $strict.GetString($receiptBytes)) 'install.json is valid UTF-8'
$link = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut)
Check ($link.TargetPath -eq $installed) 'Start Menu shortcut targets the managed executable'
Check ($link.Arguments -eq 'app') 'Start Menu shortcut launches the app'
Check ($link.Description -eq 'Hex managed install') 'Start Menu shortcut carries the managed mark the updater repoints'
$entry = Get-ItemProperty $uninstallKey
Check ($entry.DisplayName -eq 'Hex') 'Apps & Features lists Hex'
Check ($entry.DisplayVersion -eq $Version) "Apps & Features shows $Version"
$uninstaller = ($entry.UninstallString -replace '"', '')
Check ($uninstaller -like "$support\uninstall\*") 'the uninstaller lives outside versions\, which updates prune'
Check (Test-Path $uninstaller) 'the uninstaller exists'

function User-Path { [Environment]::GetEnvironmentVariable('Path', 'User') }
function Path-Has([string]$folder) {
    @((User-Path) -split ';' | Where-Object { $_.TrimEnd('\') -eq $folder }).Count -gt 0
}

# Starts without a Vulkan loader too (#1240), which some runners lack.
if ($Console) {
    $expectedConsole = (Get-FileHash -Algorithm SHA256 $Console).Hash
    Check (Test-Path $installedConsole) "hex.com is beside hex.exe at versions\$Version"
    Check ((Get-FileHash -Algorithm SHA256 $installedConsole).Hash -eq $expectedConsole) 'installed hex.com matches the release bytes'
    Check ((Get-FileHash -Algorithm SHA256 (Join-Path $bin 'hex.com')).Hash -eq $expectedConsole) 'bin\hex.com matches the release bytes'
    Check (Path-Has $bin) 'bin is on the user''s PATH'
    # hex.com is a console program, so PowerShell waits for it, prints or
    # captures its output, and sets $LASTEXITCODE, as for any other command.
    $reported = (& $installedConsole --version) -split '\s+' | Select-Object -Last 1
    Check ($reported -eq $Version) "installed hex.com runs hex.exe, which reports $Version"
    # A new terminal: the user's PATH, where `hex` resolves to bin\hex.com.
    $env:Path = "$(User-Path);$([Environment]::GetEnvironmentVariable('Path', 'Machine'))"
    $command = Get-Command hex -CommandType Application | Select-Object -First 1
    Check ($command.Source -eq (Join-Path $bin 'hex.com')) "``hex`` resolves to bin\hex.com ($($command.Source))"
    $started = Get-Date
    $status = hex status --json
    $code = $LASTEXITCODE
    Check ($code -eq 3) "``hex status --json`` waits and exits 3, Hex isn't running ($code)"
    Check (($status | Out-String) -match 'not-running') "``hex status --json`` printed its reply: $status"
    Check ($null -ne (($status | Out-String) | ConvertFrom-Json)) '``hex status --json`` printed JSON'
    Write-Output "  (hex status --json took $([int]((Get-Date) - $started).TotalMilliseconds) ms)"
} else {
    # A release before hex.com: its hex.exe was a console program.
    $out = Join-Path $logs 'version.txt'
    Start-Process $installed -ArgumentList '--version' -Wait -NoNewWindow -RedirectStandardOutput $out
    $reported = (Get-Content $out -Raw).Trim() -split '\s+' | Select-Object -Last 1
    Check ($reported -eq $Version) "installed hex.exe reports $Version"
}

Check ((Run-Setup 'reinstall') -eq 0) 'reinstalling the same version exits 0'
Check ((Get-FileHash -Algorithm SHA256 $installed).Hash -eq $expected) 'reinstall keeps the release bytes'
if ($Console) {
    Check (@((User-Path) -split ';' | Where-Object { $_.TrimEnd('\') -eq $bin }).Count -eq 1) 'reinstalling adds bin to PATH once'
}

# Versions are never compared: the installer installs its own over any other.
Set-Content -Path $pointer -Value '99.0.0' -NoNewline
Check ((Run-Setup 'over-another') -eq 0) 'installing over another version exits 0'
Check ((Get-Content $pointer -Raw).Trim() -eq $Version) 'current-version names this installer''s version'

# Simulate what a self-update and Launch at login leave behind.
$updated = Join-Path $support 'versions\99.0.0\hex.exe'
New-Item -ItemType Directory -Force (Split-Path $updated) | Out-Null
Copy-Item $installed $updated
if ($Console) {
    Copy-Item $installedConsole (Join-Path (Split-Path $updated) 'hex.com')
    Copy-Item (Join-Path $bin 'hex.com') (Join-Path $bin 'hex.com.old')
}
New-Item -ItemType Directory -Force (Join-Path $support 'updates') | Out-Null
if (-not (Test-Path $runKey)) { New-Item -Path $runKey | Out-Null }
Set-ItemProperty -Path $runKey -Name 'Hex' -Value "`"$updated`" app --hidden"
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
Check ($null -eq (Get-ItemProperty -Path $runKey -Name 'Hex' -ErrorAction SilentlyContinue)) 'Launch at login is removed'
Check (Test-Path (Join-Path $support 'settings.json')) 'settings are kept'
if ($Console) {
    Check (-not (Test-Path $bin)) 'bin is removed'
    Check (-not (Path-Has $bin)) 'bin is off the user''s PATH'
}
Remove-Item $support -Recurse -Force
Write-Output "Installer $([IO.Path]::GetFileName($Installer)) passed."
# `hex status` left $LASTEXITCODE at 3, which would become this script's code.
exit 0
