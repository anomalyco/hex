# HEX Service for Apple Silicon

> **Status:** Private, unpublished platform artifact for Apple silicon macOS.
> This checkout does not contain `bin/hex-service`.

Platform artifact consumed automatically by `@hex-ai/client`. Applications do
not import this package or locate its executable directly.

The release workflow inserts the Developer ID signed and notarized
`bin/hex-service` artifact before packing this package.
