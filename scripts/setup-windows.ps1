[CmdletBinding()]
param(
    [switch]$SkipCheck
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Test-Tool {
    param([Parameter(Mandatory)][string]$Name)

    return $null -ne (Get-Command $Name -ErrorAction SilentlyContinue)
}

function Invoke-WingetInstall {
    param(
        [Parameter(Mandatory)][string]$Id,
        [string[]]$ExtraArguments = @()
    )

    & winget install --id $Id -e --silent --accept-package-agreements `
        --accept-source-agreements @ExtraArguments
    if ($LASTEXITCODE -ne 0) {
        throw "winget could not install $Id (exit code $LASTEXITCODE)"
    }
}

function Invoke-Rustup {
    param([Parameter(Mandatory)][string[]]$Arguments)

    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        if (Test-Path -LiteralPath $script:RustupPath) {
            & $script:RustupPath @Arguments
            if ($LASTEXITCODE -ne 0) {
                throw "rustup $($Arguments -join ' ') failed (exit code $LASTEXITCODE)"
            }
            return
        }
        Start-Sleep -Milliseconds 250
    }
    throw "rustup did not finish replacing $script:RustupPath"
}

function Find-FxcCompiler {
    $configured = [Environment]::GetEnvironmentVariable("GPUI_FXC_PATH", "User")
    if (-not [string]::IsNullOrWhiteSpace($configured) -and
        (Test-Path -LiteralPath $configured -PathType Leaf)) {
        return $configured
    }

    $onPath = Get-Command "fxc.exe" -ErrorAction SilentlyContinue
    if ($null -ne $onPath) {
        return $onPath.Source
    }

    $kitBin = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
    if (-not (Test-Path -LiteralPath $kitBin -PathType Container)) {
        return $null
    }
    return Get-ChildItem -LiteralPath $kitBin -Directory |
        Sort-Object Name -Descending |
        ForEach-Object { Join-Path $_.FullName "x64\fxc.exe" } |
        Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } |
        Select-Object -First 1
}

if (-not (Test-Tool "winget")) {
    throw "winget is required. Install or update App Installer from Microsoft Store."
}

$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
$wingetLinks = Join-Path $env:LOCALAPPDATA "Microsoft\WinGet\Links"
$env:Path = "$cargoBin;$wingetLinks;$env:Path"

if (-not (Test-Tool "rustup")) {
    Write-Host "Installing rustup..."
    Invoke-WingetInstall -Id "Rustlang.Rustup"
}
$script:RustupPath = (Get-Command "rustup" -ErrorAction Stop).Source

if (-not (Test-Tool "cmake")) {
    Write-Host "Installing CMake for the current user..."
    Invoke-WingetInstall -Id "Kitware.CMake" -ExtraArguments @("--scope", "user")
}

$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
$hasCppTools = $false
if (Test-Path -LiteralPath $vswhere) {
    $cppInstallation = & $vswhere -latest -products * `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
        -property installationPath
    $hasCppTools = -not [string]::IsNullOrWhiteSpace(($cppInstallation -join ""))
}

if (-not $hasCppTools) {
    Write-Host "Installing Visual Studio Build Tools with the C++ workload..."
    Invoke-WingetInstall -Id "Microsoft.VisualStudio.2022.BuildTools" -ExtraArguments @(
        "--override",
        "--wait --passive --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
    )
}

$fxcPath = Find-FxcCompiler
if ([string]::IsNullOrWhiteSpace($fxcPath)) {
    throw "fxc.exe is required by GPUI. Install the Windows 10/11 SDK through Visual Studio Installer."
}
$env:GPUI_FXC_PATH = $fxcPath
[Environment]::SetEnvironmentVariable("GPUI_FXC_PATH", $fxcPath, "User")
Write-Host "Using GPUI shader compiler: $fxcPath"

Invoke-Rustup -Arguments @("toolchain", "install", "stable", "--profile", "minimal")
Invoke-Rustup -Arguments @("default", "stable")
Invoke-Rustup -Arguments @("target", "add", "x86_64-pc-windows-msvc")
Invoke-Rustup -Arguments @("component", "add", "rustfmt", "clippy", "rust-analyzer")

$repository = Split-Path -Parent $PSScriptRoot
Push-Location $repository
try {
    cargo fetch --target x86_64-pc-windows-msvc
    if (-not $SkipCheck) {
        cargo check --all-targets
    }
}
finally {
    Pop-Location
}

Write-Host "Windows Rust environment is ready."
Write-Host "Next: cargo run -- devices"
