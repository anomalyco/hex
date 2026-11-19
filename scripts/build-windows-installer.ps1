# Builds hex-setup-<version>-windows-<x64|arm64>.exe, the per-user Windows
# installer, around an already-built (and signed) release hex.exe and its
# console entry point hex.com. Installs the pinned Inno Setup when ISCC is
# missing, and re-pins another installed version to it. The installer embeds
# both byte for byte. -Console is left out only to wrap a release published
# before hex.com.
#
#   scripts/build-windows-installer.ps1 -Executable dist\hex-2.4.0-windows-x64.exe `
#     -Console dist\hex-2.4.0-windows-x64.com -Version 2.3.0 -Arch x86_64 -OutDir dist
#
# The setup engine (the copy Setup extracts to %TEMP% at launch and the
# installed uninstaller) is signed in two passes on separate runners, because
# the compiler never holds signing credentials (docs/windows-signing.md):
#
#   -ExtractSignedEngine    compiles with SignedUninstaller=yes and keeps the
#                           unsigned uninst-<version>-<hash>.e32 ISCC asks to
#                           have signed in -OutDir. ISCC aborts; that is the
#                           expected result.
#   -SignedEngineDir <dir>  compiles with that signed engine in <dir>.
#
# Neither is the plain unsigned single compile of Test builds.
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string]$Executable,
    [string]$Console,
    [Parameter(Mandatory)] [string]$Version,
    [Parameter(Mandatory)] [ValidateSet('x86_64', 'aarch64')] [string]$Arch,
    [Parameter(Mandatory)] [string]$OutDir,
    [string]$SignedEngineDir,
    [switch]$ExtractSignedEngine
)
$ErrorActionPreference = 'Stop'

# Both passes must use one compiler: the engine's file name carries the Inno
# Setup version and a hash of the engine bytes, so pass 2 cannot use pass 1's
# file from another version. windows-latest images ship this one; another
# installed version is re-pinned to it (Install-Iscc -Repin), and the passes
# fail only if that still leaves another version.
$InnoVersion = '6.7.1'

if ($ExtractSignedEngine -and $SignedEngineDir) { throw 'Pass -ExtractSignedEngine or -SignedEngineDir, not both.' }
$engineSigning = $ExtractSignedEngine -or [bool]$SignedEngineDir

if ($Version -notmatch '^(\d+\.\d+\.\d+(-beta\.\d+)?|0\.0\.0-dev-\d+)$') { throw "Expected a release version, not $Version." }
$Executable = (Resolve-Path $Executable).Path
$consoleDefine = @()
if ($Console) { $consoleDefine = @("/DConsole=$((Resolve-Path $Console).Path)") }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
if ($SignedEngineDir) {
    if (-not (Test-Path -LiteralPath $SignedEngineDir -PathType Container)) { throw "No signed setup engine directory at $SignedEngineDir." }
    $SignedEngineDir = (Resolve-Path $SignedEngineDir).Path
    $signedEngines = @(Get-ChildItem -LiteralPath $SignedEngineDir -File | Where-Object { $_.Name -like 'uninst-*.e32' })
    if ($signedEngines.Count -eq 0) { throw "No signed uninst-*.e32 in $SignedEngineDir." }
}
$label = if ($Arch -eq 'x86_64') { 'x64' } else { 'arm64' }
$issFile = Join-Path (Split-Path -Parent $PSScriptRoot) 'packaging\windows\hex.iss'

function Find-Iscc {
    # The installed compiler first, so a shim on PATH never decides which one
    # signs the setup engine passes.
    $candidates = @(
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
        (Get-Command ISCC.exe -ErrorAction SilentlyContinue | ForEach-Object Source)
    )
    $candidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
}

function Install-Iscc([switch]$Repin) {
    if (Get-Command choco.exe -ErrorAction SilentlyContinue) {
        # `choco install --version` treats a preinstalled package as "already
        # installed" whatever its version, so a re-pin upgrades (and may
        # downgrade) to the pinned version instead.
        if ($Repin) {
            choco upgrade innosetup --version=$InnoVersion --allow-downgrade --yes --no-progress | Out-Host
            if ($LASTEXITCODE -ne 0) { throw "choco could not change Inno Setup to $InnoVersion (exit $LASTEXITCODE)." }
        } else {
            choco install innosetup --version=$InnoVersion --yes --no-progress | Out-Host
            if ($LASTEXITCODE -ne 0) { throw "choco could not install Inno Setup $InnoVersion (exit $LASTEXITCODE)." }
        }
    } elseif ($engineSigning) {
        throw "Signing the setup engine needs Inno Setup $InnoVersion from Chocolatey, and choco.exe is not on PATH."
    } else {
        $setup = Join-Path $env:TEMP 'innosetup.exe'
        Invoke-WebRequest -Uri 'https://jrsoftware.org/download.php/is.exe' -OutFile $setup -UseBasicParsing
        Start-Process $setup -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CURRENTUSER' -Wait
    }
}

$iscc = Find-Iscc
if (-not $iscc) {
    Install-Iscc
    $iscc = Find-Iscc
    if (-not $iscc) { throw 'Inno Setup could not be installed.' }
}

function Invoke-Iscc([string[]]$Arguments, [string[]]$InputLines) {
    # The compiler's lines are data, even on stderr: keep them from becoming
    # terminating errors in Windows PowerShell or with
    # $PSNativeCommandUseErrorActionPreference on, and echo them so a
    # successful compile keeps its log.
    $ErrorActionPreference = 'Continue'
    $PSNativeCommandUseErrorActionPreference = $false
    $lines = if ($InputLines) {
        @($InputLines | & $iscc @Arguments 2>&1 | ForEach-Object { "$_" })
    } else {
        @(& $iscc @Arguments 2>&1 | ForEach-Object { "$_" })
    }
    $exitCode = $LASTEXITCODE
    $text = $lines -join "`n"
    if ($text) { Write-Host $text }
    [pscustomobject]@{ ExitCode = $exitCode; Text = $text }
}

function Get-IsccVersion {
    # ISCC.exe's version resource is 0.0.0, so ask the compiler itself: a
    # compile without /Q prints "Compiler engine version: Inno Setup 6.7.1"
    # before it does anything else. ISCC reads a script from standard input
    # ("-") and /O- disables output, so nothing is built; the throwaway
    # OutputDir is only there because ISCC uses it for scratch files, and
    # never the caller's.
    $dir = Join-Path ([IO.Path]::GetTempPath()) ("hex-inno-version-" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force $dir | Out-Null
    try {
        $lines = @(
            '[Setup]',
            'AppName=Hex Inno Setup version probe',
            'AppVersion=0.0.1',
            'DefaultDirName={autopf}\HexInnoSetupVersionProbe',
            "OutputDir=$dir",
            'OutputBaseFilename=probe',
            'Uninstallable=no'
        )
        $result = Invoke-Iscc -Arguments @('/O-', '-') -InputLines $lines
    } finally {
        Remove-Item -Recurse -Force $dir -ErrorAction SilentlyContinue
    }
    $match = [regex]::Match($result.Text, 'Compiler engine version:\s*Inno Setup\s+(\d+(?:\.\d+)+)')
    if (-not $match.Success) {
        throw "Could not read the Inno Setup version from ISCC (exit $($result.ExitCode)):`n$($result.Text)"
    }
    $match.Groups[1].Value
}

$defines = @('/Qp', "/DAppVersion=$Version", "/DArch=$label", "/DExecutable=$Executable") + $consoleDefine

if ($engineSigning) {
    $found = Get-IsccVersion
    if ($found -ne $InnoVersion) {
        Write-Host "Inno Setup $found is installed; the setup engine passes need $InnoVersion. Re-pinning it."
        Install-Iscc -Repin
        $iscc = Find-Iscc
        if (-not $iscc) { throw 'Inno Setup could not be installed.' }
        $found = Get-IsccVersion
        if ($found -ne $InnoVersion) {
            throw "Inno Setup is $found, but both setup engine passes must use $InnoVersion, because the engine's file name carries that version. Install Inno Setup $InnoVersion or change the pin in this script."
        }
    }
}

if ($ExtractSignedEngine) {
    $temp = Join-Path ([IO.Path]::GetTempPath()) "hex-setup-engine-$Arch"
    $compileOut = Join-Path $temp 'out'
    $engineDir = Join-Path $temp 'engine'
    foreach ($dir in $compileOut, $engineDir) {
        if (Test-Path -LiteralPath $dir) { Remove-Item -Recurse -Force $dir }
        New-Item -ItemType Directory -Force $dir | Out-Null
    }
    $result = Invoke-Iscc -Arguments ($defines + @("/DOutputDir=$compileOut", "/DSignedEngineDir=$engineDir", $issFile))
    # Without a signature, ISCC writes the engine it just built into
    # SignedUninstallerDir and aborts asking for that exact file to be signed
    # ("Signed uninstaller mode is enabled. Using an external code-signing
    # tool, please attach your digital signature to the following executable
    # file: ... and compile again", Compiler.SetupCompiler.pas). A compile
    # failure is exit code 2 (ISCC.dpr).
    $engines = @(Get-ChildItem -LiteralPath $engineDir -File | Where-Object { $_.Name -like 'uninst-*.e32' })
    if ($result.Text -notmatch 'Signed uninstaller mode is enabled') {
        throw "ISCC did not ask for a signed setup engine (exit $($result.ExitCode)):`n$($result.Text)"
    }
    if ($result.ExitCode -ne 2) {
        throw "ISCC asked for a signed setup engine but exited $($result.ExitCode), not 2:`n$($result.Text)"
    }
    if ($engines.Count -ne 1) { throw "Expected one unsigned setup engine in $engineDir, found $($engines.Count)." }
    $engine = $engines[0]
    $target = Join-Path $OutDir $engine.Name
    if (Test-Path -LiteralPath $target) {
        # The other architecture's compile built the same engine; it must be
        # the same bytes, or pass 2 would get one of them unpredictably.
        if ((Get-FileHash -Algorithm SHA256 -LiteralPath $target).Hash -ne (Get-FileHash -Algorithm SHA256 -LiteralPath $engine.FullName).Hash) {
            throw "$target differs from the engine this compile built. Delete it and compile again."
        }
    } else {
        Copy-Item -LiteralPath $engine.FullName -Destination $target
    }
    # ISCC's abort is the expected result here, but it leaves $LASTEXITCODE
    # set for whoever called this script, and a GitHub step exits with it.
    $global:LASTEXITCODE = 0
    Write-Output $target
    return
}

if ($SignedEngineDir) { $defines += "/DSignedEngineDir=$SignedEngineDir" }
$result = Invoke-Iscc -Arguments ($defines + @("/DOutputDir=$OutDir", $issFile))
if ($result.ExitCode -ne 0) {
    if ($result.Text -match 'Signed uninstaller mode is enabled|differ unexpectedly') {
        throw "ISCC could not use the signed setup engine in $SignedEngineDir. It needs the engine built by Inno Setup $InnoVersion from this same script; compile the engine again with -ExtractSignedEngine.`n$($result.Text)"
    }
    throw "ISCC failed with exit code $($result.ExitCode).`n$($result.Text)"
}
$installer = Join-Path $OutDir "hex-setup-$Version-windows-$label.exe"
if (-not (Test-Path $installer)) { throw "ISCC did not produce $installer." }
# Leave the caller a successful native exit code, as scripts/test-windows-installer.ps1 does.
$global:LASTEXITCODE = 0
Write-Output $installer
