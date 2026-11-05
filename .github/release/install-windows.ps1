# Install Hex as a managed, self-updating Windows application for the
# current user, mirroring install-linux.sh: the app lands in
# %APPDATA%\hex\versions\<version>\hex.exe with its console entry point
# hex.com beside it, a Start Menu shortcut points at hex.exe, %APPDATA%\hex\bin
# (a copy of hex.com) goes on PATH, install.json marks the install as direct,
# and the app then keeps itself current from the Anomaly release service
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
$bin = Join-Path $support 'bin'
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

# The user's PATH without (or with, once) bin, so a terminal runs `hex` as
# hex.com, the console program that waits for hex.exe.
function Set-BinOnPath([bool]$present) {
    $environment = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
    $current = [string]$environment.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $entries = @($current -split ';' | Where-Object { $_ -and $_.TrimEnd('\') -ne $bin })
    if ($present) { $entries += $bin }
    $updated = $entries -join ';'
    if ($updated -ne $current) {
        $environment.SetValue('Path', $updated, [Microsoft.Win32.RegistryValueKind]::ExpandString)
        # SetEnvironmentVariable broadcasts the change to running programs.
        [Environment]::SetEnvironmentVariable('HexPathRefresh', '1', 'User')
        [Environment]::SetEnvironmentVariable('HexPathRefresh', $null, 'User')
    }
    $environment.Close()
}

if ($Uninstall) {
    Remove-ManagedShortcut
    Set-BinOnPath $false
    if (Test-Path $bin) { Remove-Item $bin -Recurse -Force }
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
function Resolve-Download($entry, [string]$name) {
    if ($entry.sha256 -notmatch '^[0-9a-f]{64}$' -or $entry.size -le 0) { throw "The manifest's $name entry is invalid." }
    $url = [Uri]::new([Uri]$manifestUrl, $entry.url).AbsoluteUri
    if (-not $url.StartsWith("$origin/")) { throw "The manifest points outside $origin." }
    $url
}

# Moves <source> over <path>. A running program can't be replaced but can be
# renamed, so one in the way moves aside to <path>.old, removed next time.
function Replace-File([string]$source, [string]$path) {
    $aside = "$path.old"
    if (Test-Path $aside) { Remove-Item $aside -Force -ErrorAction SilentlyContinue }
    try {
        Move-Item $source $path -Force
    } catch {
        Move-Item $path $aside -Force
        Move-Item $source $path -Force
    }
}

# Downloads to <path>.partial, checks size and SHA-256 against the manifest,
# then moves it into place.
function Save-Download($entry, [string]$url, [string]$path) {
    $partial = "$path.partial"
    if (Test-Path $partial) { Remove-Item $partial -Force }
    Invoke-WebRequest -Uri $url -OutFile $partial -UseBasicParsing -TimeoutSec 600
    $bytes = (Get-Item $partial).Length
    if ($bytes -ne $entry.size) {
        Remove-Item $partial -Force
        throw "The download has $bytes bytes, expected $($entry.size)."
    }
    $sha = (Get-FileHash -Algorithm SHA256 $partial).Hash.ToLowerInvariant()
    if ($sha -ne $entry.sha256) {
        Remove-Item $partial -Force
        throw 'The download failed checksum verification.'
    }
    Replace-File $partial $path
}

$fileUrl = Resolve-Download $file $target
# hex.com, from releases that have one.
$consoleFile = $manifest.release.targets."$target-cli"
$consoleUrl = if ($consoleFile) { Resolve-Download $consoleFile "$target-cli" }

$versionDir = Join-Path $versions $version
$executable = Join-Path $versionDir 'hex.exe'
New-Item -ItemType Directory -Force $versionDir | Out-Null

$console = Join-Path $versionDir 'hex.com'
Write-Output "Downloading Hex $version..."
Save-Download $file $fileUrl $executable
if ($consoleUrl) { Save-Download $consoleFile $consoleUrl $console }
# The version probe runs the staged files and removes them again on a
# mismatch. hex.com waits for hex.exe, a GUI program PowerShell wouldn't.
if ($consoleUrl) {
    $reported = (& $console --version) -split '\s+' | Select-Object -Last 1
} else {
    $probe = Join-Path $versionDir 'version.txt'
    Start-Process $executable -ArgumentList '--version' -Wait -NoNewWindow -RedirectStandardOutput $probe
    $reported = (Get-Content $probe -Raw).Trim() -split '\s+' | Select-Object -Last 1
    Remove-Item $probe -Force
}
if ($reported -ne $version) {
    Remove-Item $executable -Force
    if (Test-Path $console) { Remove-Item $console -Force }
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

if ($consoleUrl) {
    New-Item -ItemType Directory -Force $bin | Out-Null
    $copy = Join-Path $bin 'hex.com.partial'
    Copy-Item $console $copy -Force
    Replace-File $copy (Join-Path $bin 'hex.com')
    Set-BinOnPath $true
}

Write-Output "Installed Hex $version."
Write-Output "  Executable: $executable"
Write-Output "  Start Menu: Hex"
if ($consoleUrl) { Write-Output "  Terminal: hex (open a new terminal; $bin is on PATH)" }
Write-Output 'Launch at login can be enabled inside Hex Settings.'
