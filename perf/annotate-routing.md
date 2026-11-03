# Annotate routing latency

## Run

```sh
scripts/check capture_host routing::look opencode:: --success-output immediate
```

This is a headless benchmark in the routing tests, not a launched Hex/strobe
case. It reads fake sessions, writes disposable bundles, and replaces delivery
with an in-memory acknowledgement. It never records a screen or opens a
microphone. Run Cargo through the build scheduler as usual.

The cases are:

- `capture_host::routing::tests::looked::routing_latency_with_a_slow_session_server`:
  a saved session-ID rule and a repository rule, with 300 ms per fake API read.
  Measures stop/look through the worker's decision and fake delivery.
- `routing::look::read::tests::parallel_reads_keep_the_same_candidates_and_decisions`:
  220 sessions, two global pages, an exact-folder read, and a missing-ID read,
  at 100 ms per request. Runs the old serial reader and the new reader, one
  warmup plus seven measured pairs; prints medians and ranges. Candidate order
  and both routing policies must agree.
- `opencode::remote::tests::a_hung_remote_command_obeys_the_idle_check_deadline`:
  a fake five-second remote command, stopped by the real 600 ms idle-check
  budget. Separate tests cover inherited pipes and oversized responses.

## Findings

The old reader serialized local sensing, global session pages, exact folders,
and missing IDs. The new reader overlaps local sensing and exact folders with
the global pages, keeping at most two requests in flight and preserving merge
order. The missing-ID read still follows the lists so an already-listed ID
doesn't need another request.

The remote transport also ignored the supplied API timeout. The pre-send idle
check could take seconds despite its 600 ms budget. Remote commands now obey
one wall-clock deadline, with bounded output and child cleanup. macOS and
Linux reuse an SSH connection for 60 seconds after its last use; the socket
is in a private, owner-checked directory. Windows retains separate connections.

## Measurements

On a Mac, same 300 ms fake API, one host-path smoke sample per case:

| Stop/look through fake send | Before | After |
| --- | ---: | ---: |
| Session-ID rule | 389 ms | 444 ms |
| Repository rule | 794 ms | 445 ms |

These include scheduling and temporary-file overhead, not just API latency.
The session-ID case still needs one read and shows no speed improvement.

The seven-pair scan benchmark measured a median **866 ms serial → 648 ms
parallel**, with ranges 763–967 ms and 516–685 ms respectively. The fake
five-second command returned after 732 ms under the 600 ms timeout, including
process scheduling and cleanup. The targeted check passed 91 tests.

## Limits and rejected shortcuts

- A saved rule cannot simply skip the session list: a spoken session name has
  higher precedence, and the same list supplies the capsule's recent sessions.
- Saved rules already skip model inference and send immediately on this base
  revision. There is no pre-send countdown left to remove.
- Guesses still wait for the routing model. Its observed multi-second latency
  is not represented by the fake API benchmark, nor reduced by this change.
- SSH's first handshake, remote CLI startup, and the synchronous light-bundle
  transfer remain. Connection reuse removes repeat handshakes, not those costs.
- No live end-to-end subsecond claim: there was no capture, real prompt send,
  installed-app replacement, or visual test in this work. The capsule is not
  changed.
