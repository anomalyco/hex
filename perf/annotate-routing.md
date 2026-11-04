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

## First-pass limits (superseded below)

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

## Second pass: publish before remote work

The comparison keeps the first-pass worker as a test-only reference. Both
workers see the same disposable bundles and sessions, at 60 ms per fake API
read, with a 2.5 s model answer. The new path prepares sessions during recording.
One warmup and seven measured runs per case, on a Mac:

| Written → first usable route | Before median (range) | After median (range) |
| --- | ---: | ---: |
| Saved session | 203 ms (148–218) | 9 ms (6–9) |
| Saved repository | 171 ms (144–212) | 6 ms (2–9) |
| Model-assisted guess | 2,760 ms (2,665–2,864) | 15 ms (9–18) |

The first guess is deterministic and usable immediately; the model can refine
it later. Picking or undoing cancels outstanding refinements. Saved rules take
precedence over spoken names, start a three-second pre-send cancellation window
at Written, and resolve a repository's session during that window. An older
accepted rule send survives a newer capture.

The host benchmark models delivery as three serialized 60 ms operations before
(light copy, status, post) and two afterward (status, post). It measured route
shown → fake send completion at 315 → 3,256 ms for session rules and
261 → 3,253 ms for repository rules. **The new numbers include the intentional
3,000 ms countdown**, which this base revision lacked. These delivery numbers
are a scheduling model, not a measurement of SSH or HTTP; the separate transport
fixture below exercises the real send code.

Run the paired host benchmark (its wall-clock samples need the longer CI profile):

```sh
scripts/check paired_written_to_route_and_send --run-ignored only --profile ci --success-output immediate
```

### Remote delivery

The default remote path sends request JSON on SSH stdin to a Python 3
standard-library HTTP client. It discovers the running service, checks a
registered PID through `/api/info`, and posts the contact sheet inline.
The folder copy follows in the background. Explicit `agentOpencode` settings
retain the CLI path; a missing Python interpreter falls back before any request
is made, never after an ambiguous POST failure.

`opencode::remote_fixture::paired_remote_delivery` runs the real bundle loader,
send, status check, subprocess transport and prompt parsing against a disposable
HTTP server. A fake SSH hop adds 60 ms per invocation and drains tar transfers.
The before case uses the retained CLI/copy path; the after case uses the new
HTTP/stdin path. The fake CLI has no added startup penalty, and the bundle has
no heavy media. A separate test checks inline image bytes and HTTP 401/409
responses through the Python client.

| Real send pipeline, fake 60 ms SSH | Before median (range) | After median (range) |
| --- | ---: | ---: |
| Delivery after choosing / countdown | 592 ms (444–643) | 409 ms (288–430) |

This is a 31% median reduction without modeling the production CLI's startup
cost. It excludes the intentional countdown and includes subprocess and file
overhead. It does not establish an eight-second improvement on a live host.

```sh
scripts/check paired_remote_delivery --run-ignored only --profile ci --success-output immediate
```

### Budgets and proof limits

Credential lookup previously ran before the model request's three-second timer.
It now shares the deadline with inference and retries. Request stdin, stdout,
stderr and descendant cleanup are bounded too; a child that never reads stdin
cannot extend the wait. The deterministic guess does not wait for this budget.

The reported multi-second live send has not been reproduced by these fixtures.
The old path serialized light-file transfer, remote CLI startup for status, and
remote CLI startup for the POST. The default path removes both CLI startups and
the foreground transfer, but its live benefit is unmeasured. The 600 ms optional
idle check is bounded; an unavailable service or slow POST can still delay or
fail delivery. No live Hex launch, screen capture, real prompt send or installed
app replacement was performed.
