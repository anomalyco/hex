---
"@kitlangton/hex": minor
---

`transcribe()` takes `timestamps: "segment" | "word"` and returns the speech model's own `segments` and `words` (`TimedText`: `text`, `startMs`, `endMs`, in milliseconds from the clip's start) with `timestampGranularity`, when Labs › Timestamps in API transcripts is on. The running app reads the lab live; the embedded helper loads no settings, so it reads the saved `labs.api_timestamps` once, on its first request, and a change takes effect after it restarts. `"none"` and absent send no parameter and keep the response as before. A model without that level answers `unsupported-timestamps`; the times are never estimated.
