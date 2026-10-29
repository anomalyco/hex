# Checks the Authenticode signatures CI applied through Azure Artifact
# Signing to every executable and installer in the given folders. Run after
# both the executables and the installers are signed.
#
#   .github/release/verify-authenticode.ps1 -Folders work
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string[]]$Folders
)
$ErrorActionPreference = 'Stop'

$files = @($Folders | ForEach-Object { Get-ChildItem -LiteralPath $_ -Filter '*.exe' })
if ($files.Count -eq 0) { throw "No executables in $($Folders -join ', ')." }
foreach ($file in $files) {
    $signature = Get-AuthenticodeSignature -LiteralPath $file.FullName
    if ($signature.Status -ne 'Valid') {
        throw "$($file.Name) is not validly signed: $($signature.Status) $($signature.StatusMessage)"
    }
    # Artifact Signing certificates last days; only the timestamp keeps
    # the signature valid afterwards.
    if (-not $signature.TimeStamperCertificate) { throw "$($file.Name) has no RFC 3161 timestamp." }
    Write-Output "ok - $($file.Name) signed by $($signature.SignerCertificate.Subject)"
}
