# Hex words file: shared cases

The cases every Hex reads, merges and writes `.hexwords` files against: the desktop app
(Rust), Hex for iPhone and Hex for Android. The format is defined in
[`docs/specs/hex-words.md`](../../../docs/specs/hex-words.md). This folder only proves it.

- **`read.json`**: one case per file in `files/`. Read its bytes (or, for `generate`, the
  base file followed by spaces up to `padWithSpacesTo` bytes) and compare the whole result to
  `expect`. Compare `skipped` as a set of `{path, reason}`. Where a case has `skippedCounts`
  instead, compare the number of skips per reason.
- **`merge.json`**: `merge(receiver, before, incoming, limits, parts, choices)` against
  `expect`. `receiver` is `desktop` or `phone`, and `limits` overrides the receiver's own
  for that case. Undo after each case must give back `before`.
- **`write.json`**: give the writer `input`, parse what it wrote, drop `madeBy`, and compare
  to `expect.file`. Compare `expect.notExported` as a set. The written bytes must also be
  UTF-8 without a byte order mark, pass `hex-words.schema.json`, and read back with nothing
  skipped.
- **`hex-words.schema.json`**: JSON Schema 2020-12 for what a writer writes.

Some files here are broken on purpose: not UTF-8, UTF-16, 10,000 levels deep. Keep them byte for
byte. Don't reformat them, and don't let an editor fix their encoding.

A change to the definition that changes an outcome changes these cases in the same pull request.
