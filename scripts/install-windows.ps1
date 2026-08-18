# Install HEX as a managed, self-updating Windows application for the
# current user, mirroring install-linux-release.sh: the executable lands
# in %APPDATA%\voice-control\versions\<version>\hex.exe, a Start Menu
# shortcut points at it, and the app then keeps itself current through
# the ed25519-signed update feed.
#
# Bootstrap trust: PowerShell cannot verify ed25519, so this installer
# trusts TLS to the release origin plus the feed's content address
# (size and SHA-256). Every later update is signature-verified by the
# app itself before activation.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File install-windows.ps1
#   powershell -ExecutionPolicy Bypass -File install-windows.ps1 -Uninstall

[CmdletBinding()]
param(
    [switch]$Uninstall
)

$ErrorActionPreference = 'Stop'

$baseUrl = if ($env:HEX_RELEASE_BASE_URL) { $env:HEX_RELEASE_BASE_URL } else { 'https://pub-089d681d41754031a4aefa7017d8c2fb.r2.dev' }
$support = Join-Path $env:APPDATA 'voice-control'
$versions = Join-Path $support 'versions'
$pointer = Join-Path $support 'current-version'
$shortcut = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\HEX.lnk'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$managedMark = 'HEX managed install'

function Remove-ManagedShortcut {
    if (Test-Path $shortcut) {
        $shell = New-Object -ComObject WScript.Shell
        $link = $shell.CreateShortcut($shortcut)
        if ($link.Description -eq $managedMark) {
            Remove-Item $shortcut -Force
        }
    }
}

if ($Uninstall) {
    Remove-ManagedShortcut
    try { Remove-ItemProperty -Path $runKey -Name 'HEX' -ErrorAction Stop } catch {}
    if (Test-Path $versions) { Remove-Item $versions -Recurse -Force }
    if (Test-Path $pointer) { Remove-Item $pointer -Force }
    Write-Output "Removed HEX. Logs and settings remain in $support."
    exit 0
}

Write-Output "Fetching the HEX Windows release feed..."
$feedRaw = Invoke-WebRequest -Uri "$baseUrl/windows-update.json" -UseBasicParsing -TimeoutSec 30
$feed = $feedRaw.Content | ConvertFrom-Json
$payloadBytes = [Convert]::FromBase64String($feed.payload)
$manifest = [Text.Encoding]::UTF8.GetString($payloadBytes) | ConvertFrom-Json

if ($manifest.schema_version -ne 1) { throw "Unsupported update schema $($manifest.schema_version)." }
if ($manifest.channel -ne 'stable') { throw 'Unsupported update channel.' }
if ($manifest.target -ne 'x86_64-pc-windows-msvc') { throw 'The published release targets a different platform.' }
$version = $manifest.version
$artifact = $manifest.artifact
if ($artifact -ne "HEX-$version-$($manifest.sha256)-x86_64-windows.exe") {
    throw 'The release artifact is not content-addressed.'
}

$versionDir = Join-Path $versions $version
$executable = Join-Path $versionDir 'hex.exe'
New-Item -ItemType Directory -Force $versionDir | Out-Null

Write-Output "Downloading HEX $version..."
$partial = Join-Path $versionDir 'hex.exe.partial'
if (Test-Path $partial) { Remove-Item $partial -Force }
Invoke-WebRequest -Uri "$baseUrl/releases/$artifact" -OutFile $partial -UseBasicParsing -TimeoutSec 600

$bytes = (Get-Item $partial).Length
if ($bytes -ne $manifest.bytes) {
    Remove-Item $partial -Force
    throw "The download has $bytes bytes, expected $($manifest.bytes)."
}
$sha = (Get-FileHash -Algorithm SHA256 $partial).Hash.ToLowerInvariant()
if ($sha -ne $manifest.sha256) {
    Remove-Item $partial -Force
    throw 'The download failed checksum verification.'
}
Move-Item $partial $executable -Force
# PowerShell only executes .exe paths, so the version probe runs on the
# staged executable and removes it again on a mismatch.
$reported = (& $executable --version) -split '\s+' | Select-Object -Last 1
if ($reported -ne $version) {
    Remove-Item $executable -Force
    throw "The downloaded HEX reports version $reported, expected $version."
}

$pointerPartial = Join-Path $support 'current-version.partial'
Set-Content -Path $pointerPartial -Value $version -NoNewline
Move-Item $pointerPartial $pointer -Force

$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut($shortcut)
$link.TargetPath = $executable
$link.Arguments = 'app'
$link.Description = $managedMark
$link.WorkingDirectory = $versionDir
$link.Save()

Write-Output "Installed HEX $version."
Write-Output "  Executable: $executable"
Write-Output "  Start Menu: HEX"
Write-Output 'Launch at login can be enabled inside HEX Settings.'
