# Install Hex as a managed, self-updating Windows application for the
# current user, mirroring install-linux.sh: the executable lands
# in %APPDATA%\hex\versions\<version>\hex.exe, a Start Menu
# shortcut points at it, install.json marks the install as direct, and the
# app then keeps itself current from the Anomaly release service
# (docs/plans/release-service.md).
#
# Trust: HTTPS to the release service, the manifest's size and SHA-256 for
# the download, and the executable's Authenticode signature, which the app
# checks again for every update.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File install-windows.ps1
#   powershell -ExecutionPolicy Bypass -File install-windows.ps1 -Uninstall

[CmdletBinding()]
param(
    [switch]$Uninstall
)

$ErrorActionPreference = 'Stop'

$origin = if ($env:HEX_RELEASE_BASE_URL) { $env:HEX_RELEASE_BASE_URL.TrimEnd('/') } else { 'https://anoma.ly/release' }
if (-not $origin.StartsWith('https://')) { throw 'HEX_RELEASE_BASE_URL must be an https:// URL.' }
# HEX_INSTALL_CHANNEL=dev installs the newest dev build and keeps the
# install on the Dev update channel; =stable requires the stable release.
# Unset, it installs stable, or the newest dev build while the release
# service has no stable Hex release yet.
$channel = $env:HEX_INSTALL_CHANNEL
if ($channel -and $channel -notin @('stable', 'dev')) { throw 'HEX_INSTALL_CHANNEL must be stable or dev.' }
$support = Join-Path $env:APPDATA 'hex'
$versions = Join-Path $support 'versions'
$pointer = Join-Path $support 'current-version'
$receipt = Join-Path $support 'install.json'
$shortcut = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Hex.lnk'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$managedMark = 'Hex managed install'

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
    try { Remove-ItemProperty -Path $runKey -Name 'Hex' -ErrorAction Stop } catch {}
    if (Test-Path $versions) { Remove-Item $versions -Recurse -Force }
    if (Test-Path $pointer) { Remove-Item $pointer -Force }
    if (Test-Path $receipt) { Remove-Item $receipt -Force }
    Write-Output "Removed Hex. Logs and settings remain in $support."
    exit 0
}

# A 32-bit PowerShell on 64-bit Windows reports the real machine here.
$machine = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
$target = switch ($machine) {
    'AMD64' { 'windows-x64' }
    'ARM64' { 'windows-arm64' }
    default { throw "Hex supports 64-bit x64 and ARM64 Windows, not $machine." }
}
# The x64 build decodes on the GPU through Vulkan, whose loader ships with
# NVIDIA, AMD, and Intel graphics drivers rather than with Windows itself.
if ($machine -eq 'AMD64' -and -not (Test-Path (Join-Path $env:SystemRoot 'System32\vulkan-1.dll'))) {
    throw 'Hex for x64 Windows needs a graphics driver with Vulkan support (vulkan-1.dll was not found). Update the GPU driver, then run this installer again.'
}

if (-not $channel) {
    try {
        $null = Invoke-WebRequest -Uri "$origin/hex/desktop/stable.json" -UseBasicParsing -TimeoutSec 30
        $channel = 'stable'
    } catch {
        if ($_.Exception.Response -and [int]$_.Exception.Response.StatusCode -eq 404) {
            Write-Output 'Hex has no stable release yet; installing the newest dev build, which keeps updating from the Dev channel. Set HEX_INSTALL_CHANNEL=stable to require the stable release.'
            $channel = 'dev'
        } else {
            throw
        }
    }
}
$manifestUrl = "$origin/hex/desktop/$channel.json"
Write-Output "Fetching $manifestUrl..."
$manifest = (Invoke-WebRequest -Uri $manifestUrl -UseBasicParsing -TimeoutSec 30).Content | ConvertFrom-Json
if ($manifest.schema -ne 1) { throw "Unsupported manifest schema $($manifest.schema)." }
if ($manifest.app -ne 'hex' -or $manifest.artifact -ne 'desktop' -or $manifest.channel -ne $channel) {
    throw "The manifest is not Hex desktop on the $channel channel."
}
$version = $manifest.release.version
# Display text that names a directory; versions are never compared.
if ($version -notmatch '^[0-9A-Za-z][0-9A-Za-z.+_-]{0,127}$') { throw "Unexpected version $version." }
$file = $manifest.release.targets.$target
if (-not $file) { throw "Hex $version has no $target build." }
if ($file.sha256 -notmatch '^[0-9a-f]{64}$' -or $file.size -le 0) { throw "The manifest's $target entry is invalid." }
$fileUrl = [Uri]::new([Uri]$manifestUrl, $file.url).AbsoluteUri
if (-not $fileUrl.StartsWith("$origin/")) { throw "The manifest points outside $origin." }

$versionDir = Join-Path $versions $version
$executable = Join-Path $versionDir 'hex.exe'
New-Item -ItemType Directory -Force $versionDir | Out-Null

Write-Output "Downloading Hex $version..."
$partial = Join-Path $versionDir 'hex.exe.partial'
if (Test-Path $partial) { Remove-Item $partial -Force }
Invoke-WebRequest -Uri $fileUrl -OutFile $partial -UseBasicParsing -TimeoutSec 600

$bytes = (Get-Item $partial).Length
if ($bytes -ne $file.size) {
    Remove-Item $partial -Force
    throw "The download has $bytes bytes, expected $($file.size)."
}
$sha = (Get-FileHash -Algorithm SHA256 $partial).Hash.ToLowerInvariant()
if ($sha -ne $file.sha256) {
    Remove-Item $partial -Force
    throw 'The download failed checksum verification.'
}
Move-Item $partial $executable -Force
# PowerShell only executes .exe paths, so the version probe runs on the
# staged executable and removes it again on a mismatch.
$reported = (& $executable --version) -split '\s+' | Select-Object -Last 1
if ($reported -ne $version) {
    Remove-Item $executable -Force
    throw "The downloaded Hex reports version $reported, expected $version."
}

$pointerPartial = Join-Path $support 'current-version.partial'
Set-Content -Path $pointerPartial -Value $version -NoNewline
Move-Item $pointerPartial $pointer -Force
$receiptPartial = "$receipt.partial"
# WriteAllText writes UTF-8 without a byte order mark on every PowerShell.
[IO.File]::WriteAllText($receiptPartial, ([ordered]@{ app = 'hex'; artifact = 'desktop'; channel = $channel; method = 'direct'; root = $support } | ConvertTo-Json -Compress))
Move-Item $receiptPartial $receipt -Force

$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut($shortcut)
$link.TargetPath = $executable
$link.Arguments = 'app'
$link.Description = $managedMark
$link.WorkingDirectory = $versionDir
$link.Save()

Write-Output "Installed Hex $version."
Write-Output "  Executable: $executable"
Write-Output "  Start Menu: Hex"
Write-Output 'Launch at login can be enabled inside Hex Settings.'
