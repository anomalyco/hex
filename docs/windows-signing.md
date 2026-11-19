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
└── build-windows-<arch>: hex-<v>-windows-<x64|arm64>.exe (+ SHA256SUMS, digest output)
sign-windows (release env, .github/release only)
├── verify the artifacts' digests
├── azure/login (OIDC, no client secret)
├── artifact-signing-action: both hex.exe     // SHA-256, RFC 3161 timestamp
└── verify-authenticode.ps1                   // every exe Valid + timestamped
setup-engine (windows-setup-engine.yml)        // signed runs only; no secrets
└── build-windows-installer.ps1 -ExtractSignedEngine: uninst-<inno>-<hash>.e32
sign-windows-engine (release env)              // signs that setup engine
installers (windows-installers.yml)            // no secrets
├── build-windows-installer.ps1 -SignedEngineDir // Inno embeds the signed bytes
└── test-windows-installer.ps1 on native x64 and ARM64, -SignedEngine
sign-windows-installers (release env)          // signs and checks both hex-setup-*.exe
publish                                       // stable requires the signatures
```

- The compile jobs never hold signing credentials. Only the signing jobs enter
  the `release` environment, and they run no repository code. Test builds never
  sign.
- Signing is skipped while any Azure value is missing. The run then warns
  and writes "not Authenticode-signed" to its summary; everything else still
  runs, including the installer tests.
- Setup runs from a copy of its engine that it extracts to `%TEMP%` at launch,
  and the uninstaller it writes (`hex\uninstall\unins000.exe`) is another copy
  of the same engine. Neither is the outer Setup.exe, so signing that exe
  afterwards does not sign them, and Windows 11 Smart App Control
  (VerifiedAndReputableDesktop) blocks the unsigned copy: setup aborts with
  "Unable to execute file in the temporary directory" (error 4551).
- `SignedUninstaller=yes` fixes both: Inno Setup embeds a signature in the
  engine, so the extracted copy and the installed uninstaller verify. The
  signature cannot be applied inside the compile that uses it, because that
  job has no credentials, so the engine is signed in its own passes, and only
  when Windows is signed (`setup-engine` waits for `sign-windows`):

1. `setup-engine` compiles `packaging/windows/hex.iss` with
   `SignedUninstaller=yes` and a `SignedUninstallerDir`. With no SignTool
   configured, ISCC writes the unsigned `uninst-<inno version>-<hash>.e32`
   there and aborts as a compile failure (exit 2), asking for that file to be
   signed. `build-windows-installer.ps1 -ExtractSignedEngine` treats exactly
   that abort as success, keeps the engine, and fails if the abort or the file
   is missing.
2. `sign-windows-engine` Authenticode-signs the `.e32` and verifies it
   (`verify-authenticode.ps1 -Extensions .e32`). Apart from the shared
   credentials check, its steps are unguarded: with a missing credential the
   sign step fails the job rather than uploading nothing.
3. `installers` compiles again with `-SignedEngineDir` pointing at the signed
   engine. ISCC copies the signature into the engine it embeds, so no sign
   tool runs there. If ISCC instead asks for a signature again, the signed
   engine does not match the compiler's engine (a different Inno Setup
   version, or changed `[Setup]` directives) and the build fails loudly.

```ts
Setup.exe (signed after compile)
└── engine + attached signature (SignedUninstaller)   // signed by sign-windows-engine
    ├── %TEMP% self-copy at launch                    // the one Smart App Control runs
    └── uninstall\unins000.exe                        // written at install time
```

- Both passes must use the same Inno Setup, because the engine's file name
  carries the compiler version and a hash of the engine bytes. The Inno Setup
  engine is always 32-bit (`.e32` on both architectures). The script pins
  6.7.1, read from the compiler's banner since ISCC.exe's version resource is
  0.0.0; when ISCC is missing it installs that version, and when another
  version is installed it re-pins with `choco upgrade --version=6.7.1
  --allow-downgrade` (a plain `choco install --version` treats a preinstalled
  package as already installed). Either pass then fails rather than compile
  with another version. A runner image update surfaces here, on every signed
  run.
- Test builds and any run without the Azure values compile the installer once
  with no `SignedUninstaller` define: unchanged, unsigned, and no engine pass
  at all.
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
- Smart App Control checks the setup engine's signature every time setup runs
  rather than a reputation, so the engine's signature only has to be valid and
  timestamped; it is the same public-trust certificate the executables use.
- Keep the signed bytes the same everywhere: the R2 release, the GitHub
  mirror, and the installer all serve the same files. Re-signing would create
  new files with no reputation.
