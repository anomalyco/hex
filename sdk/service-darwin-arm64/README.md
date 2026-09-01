# HEX Service for Apple Silicon

> **Status:** Private, unpublished platform artifact for Apple silicon macOS.
> This checkout does not contain `bin/hex-service`.

`@hex-ai/service-darwin-arm64` is a placeholder for the planned native dependency
of `@kitlangton/hex`. The public client does not declare this package as a
dependency. Its platform resolver recognizes this name, but consumers currently
must supply an explicit helper command to `create()`.

The TypeScript release workflow does not build or insert `bin/hex-service`.
The separate service-app scripts package the full desktop executable as
`hex-service`; they do not produce a transcription-only npm payload. A validated
signed artifact, consumer packaging, and helper publication remain planned in
[`docs/plans/typescript-sdk.md`](../../docs/plans/typescript-sdk.md).
