---
"@kitlangton/hex": patch
---

`connect()` reads the macOS discovery file from `~/Library/Application Support/hex`, where Hex now keeps its data, falling back to the previous `voice-control` folder.
