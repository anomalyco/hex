# Checks the Authenticode signatures CI applied through Azure Artifact
# Signing to every executable (.exe and hex.com) and installer in the given
# folders. Run after both the executables and the installers are signed.
# -Extensions names which files to check: the setup engine (.e32) is signed in
# its own job, before an installer embeds it.
#
#   .github/release/verify-authenticode.ps1 -Folders work
#   .github/release/verify-authenticode.ps1 -Folders engine -Extensions .e32
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string[]]$Folders,
    [string[]]$Extensions = @('.exe', '.com')
)
$ErrorActionPreference = 'Stop'

# Accept an extension with or without its dot.
$Extensions = @($Extensions | ForEach-Object { if ($_ -like '.*') { $_ } else { ".$_" } })
$files = @($Folders | ForEach-Object { Get-ChildItem -LiteralPath $_ -File | Where-Object Extension -in $Extensions })
if ($files.Count -eq 0) { throw "No $(($Extensions -join ', ')) files in $($Folders -join ', ')." }
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
