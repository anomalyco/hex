# Builds hex-setup-<version>-windows-<x64|arm64>.exe, the per-user Windows
# installer, around an already-built (and signed) release hex.exe. Installs
# Inno Setup 6 when ISCC is missing. The installer embeds the executable byte
# for byte.
#
#   scripts/build-windows-installer.ps1 -Executable dist\Hex-2.4.0-windows-x64.exe `
#     -Version 2.3.0 -Arch x86_64 -OutDir dist
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string]$Executable,
    [Parameter(Mandatory)] [string]$Version,
    [Parameter(Mandatory)] [ValidateSet('x86_64', 'aarch64')] [string]$Arch,
    [Parameter(Mandatory)] [string]$OutDir
)
$ErrorActionPreference = 'Stop'

if ($Version -notmatch '^(\d+\.\d+\.\d+(-beta\.\d+)?|0\.0\.0-dev-\d+)$') { throw "Expected a release version, not $Version." }
$Executable = (Resolve-Path $Executable).Path
New-Item -ItemType Directory -Force $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
$label = if ($Arch -eq 'x86_64') { 'x64' } else { 'arm64' }
$script = Join-Path (Split-Path -Parent $PSScriptRoot) 'packaging\windows\hex.iss'

function Find-Iscc {
    $candidates = @(
        (Get-Command ISCC.exe -ErrorAction SilentlyContinue | ForEach-Object Source),
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    )
    $candidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
}

$iscc = Find-Iscc
if (-not $iscc) {
    if (Get-Command choco.exe -ErrorAction SilentlyContinue) {
        choco install innosetup --yes --no-progress | Out-Host
    } else {
        $setup = Join-Path $env:TEMP 'innosetup.exe'
        Invoke-WebRequest -Uri 'https://jrsoftware.org/download.php/is.exe' -OutFile $setup -UseBasicParsing
        Start-Process $setup -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CURRENTUSER' -Wait
    }
    $iscc = Find-Iscc
    if (-not $iscc) { throw 'Inno Setup could not be installed.' }
}

& $iscc /Qp "/DAppVersion=$Version" "/DArch=$label" "/DExecutable=$Executable" "/DOutputDir=$OutDir" $script
if ($LASTEXITCODE -ne 0) { throw "ISCC failed with exit code $LASTEXITCODE." }
$installer = Join-Path $OutDir "hex-setup-$Version-windows-$label.exe"
if (-not (Test-Path $installer)) { throw "ISCC did not produce $installer." }
Write-Output $installer
