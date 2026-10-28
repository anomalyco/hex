# Windows Authenticode signing

Windows releases are Authenticode-signed with Anomaly's Azure Artifact Signing
(formerly Trusted Signing) account from GitHub Actions. Signing is separate
from the release service: Authenticode is what Windows and SmartScreen check,
and Hex's updater checks it again (with the manifest's SHA-256) before
installing an update.

## Pipeline

`.github/workflows/release.yml`, split like OpenCode's release:

```ts
build (build-desktop.yml; x64 and ARM64 runners)   // compiles; no environment or secrets
└── build-windows-<arch>: Hex-<v>-windows-<x64|arm64>.exe (+ SHA256SUMS, digest output)
sign-windows (release env, .github/release only)
├── verify the artifacts' digests
├── azure/login (OIDC, no client secret)
├── artifact-signing-action: both hex.exe     // SHA-256, RFC 3161 timestamp
└── verify-authenticode.ps1                   // every exe Valid + timestamped
installers (windows-installers.yml)            // no secrets
├── build-windows-installer.ps1 x64 + arm64   // Inno embeds the signed bytes
└── test-windows-installer.ps1 on native x64 and ARM64
sign-windows-installers (release env)          // signs and checks both Hex-Setup-*.exe
publish                                       // stable requires the signatures
```

- The compile jobs never hold signing credentials. Only the two signing
  jobs enter the `release` environment, and they run no repository code.
  Test builds never sign.
- Signing is skipped while any Azure value is missing. The run then warns
  and writes "not Authenticode-signed" to its summary; everything else still
  runs, including the installer tests.
- The Inno Setup uninstaller (`voice-control\uninstall\unins000.exe`) is
  written at install time and is not signed. It is created locally with no
  Mark of the Web, so SmartScreen does not check it.
- Self-updates download `hex.exe` through Hex itself, so SmartScreen is not
  involved there either. The manifest's SHA-256 and the Authenticode check
  protect it.

## Configuration

The workflow uses OpenCode's names. In anomalyco/hex, these are `release`
environment secrets:

- `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`, `AZURE_SUBSCRIPTION_ID`: the Entra app
  or managed identity that GitHub OIDC signs in as.
- `AZURE_TRUSTED_SIGNING_ENDPOINT`: the regional endpoint, for example
  `https://eus.codesigning.azure.net/`.
- `AZURE_TRUSTED_SIGNING_ACCOUNT_NAME`, `AZURE_TRUSTED_SIGNING_CERTIFICATE_PROFILE`:
  the signing account and its Public Trust certificate profile.

The identity needs a federated credential with issuer
`https://token.actions.githubusercontent.com`, subject
`repo:anomalyco/hex:environment:release`, and audience
`api://AzureADTokenExchange`. It also needs the *Artifact Signing Certificate
Profile Signer* role on the profile. Luke (Hona) owns Anomaly's Azure
self-service setup.

## SmartScreen reputation

- Artifact Signing issues a short-lived certificate (about three days) for
  each signing session. Without the timestamp, a signature would expire with
  its certificate, so `verify-authenticode.ps1` rejects any signature that
  lacks one.
- SmartScreen builds reputation per publisher identity, not per certificate.
  The daily certificate rotation does not reset it. A new publisher can still
  show "Windows protected your PC" on early downloads until enough installs
  build reputation. The warning then names Anomaly Innovations, Inc. instead
  of "Unknown publisher".
- Keep the signed bytes the same everywhere: the R2 release, the GitHub
  mirror, and the installer all serve the same files. Re-signing would create
  new files with no reputation.
