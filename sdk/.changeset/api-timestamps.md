---
"@kitlangton/hex": minor
---

`transcribe()` takes `timestamps: "segment" | "word"` on the running app and returns the speech model's own `segments` and `words` (`TimedText`: `text`, `startMs`, `endMs`, in milliseconds from the clip's start) with `timestampGranularity`, when Labs › Timestamps in API transcripts is on. `"none"` and absent send no parameter and keep the response as before. A model without that level answers `unsupported-timestamps`; the times are never estimated.
