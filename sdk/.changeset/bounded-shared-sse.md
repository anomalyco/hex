---
"@kitlangton/hex": patch
---

Decode model progress and dictation levels with shared bounded SSE framing. Large transport chunks containing many small level events no longer fail the event limit, and CR-delimited events dispatch without waiting for more data or EOF. Cancellation, callback failures, and bounded observation buffers remain supported.
