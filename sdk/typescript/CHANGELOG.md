# @kitlangton/hex

## 0.2.2

### Patch Changes

- e163753: Support Effect 4.0.0-rc.112 and newer in the Effect client, using the current tagged-error API.

## 0.2.1

### Patch Changes

- c5ba7d4: Stop dictation heartbeat timers after terminal capture errors while preserving retries for transient failures.

## 0.2.0

### Minor Changes

- Require local API v2 for owner-scoped dictation capture, rejecting legacy HEX
  apps before a recording can start and become orphaned.
