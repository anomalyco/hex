# OpenCode V2 Compatibility

## Validation Baseline

The 2026-08-26 compatibility baseline is OpenCode V2 commit
[`6c392355dd6c4f15889d28273c6e3dbf69c4548c`](https://github.com/anomalyco/opencode/tree/6c392355dd6c4f15889d28273c6e3dbf69c4548c)
and the [official V2 API reference](https://opencode.ai/v2/docs/api/).
The newest published V2-branch CLI verified in the npm registry was
[`@opencode-ai/cli@0.0.0-dev-18334`](https://www.npmjs.com/package/@opencode-ai/cli/v/0.0.0-dev-18334),
published from that same commit by
[release run 18334](https://github.com/anomalyco/opencode/actions/runs/33009417492).
Its catalog, generation, and service-discovery contracts are unchanged from the
initial audit at `009eca8b5baa01b690753992725b450d045dd623`.
This records source review and local regression tests, including authenticated
loopback generation with a 1 MB JSON body, cancellation, and deadline expiry.
A provider-backed generation test also passed using CLI `0.0.0-dev-18334` and
the already-running server `0.0.0-dev-18329`, without creating a session or
restarting the service.

This is a dated validation snapshot, not a claim about the newest CLI available
today. The integration contract below describes Hex's current implementation;
new provider-backed validation must record its own CLI and server versions.

## Integration Contract

Hex never runs an OpenCode executable, never starts or restarts the service,
and never searches for or probes an installed CLI. It only talks to an already
running service (`src/desktop/opencode_service.rs`):

- It reads the bounded registration `service.json` (the stable, beta and dev
  channels), then other channels' `service-<channel>.json`, in
  `$XDG_STATE_HOME/opencode`, else `~/.local/state/opencode`; OpenCode uses the
  same path on Windows. On macOS and Linux the file must be a regular file the
  user owns with no group or other permissions.
- The registration's `url` must be loopback `http://` (`localhost`, `0.0.0.0`
  and `::` are reached through loopback); anything else is refused.
- `GET /api/info` with the registration's password (HTTP Basic, user
  `opencode`) must answer with the registered `pid` (and `version`, when
  registered). Otherwise the registration is stale and OpenCode isn't running.
- Requests are HTTP/1.1 over `TcpStream`; content-length and chunked answers are
  decoded, bounded to 16 MiB, and every wait observes the caller's deadline and
  cancellation. Credentials never reach diagnostics.

When no service answers, features report "OpenCode isn't running" and link to
the OpenCode docs; Hex starts nothing.

Hex reads `/api/model` and `/api/model/default` through authenticated HTTP, avoiding
large piped CLI responses. It supplies `x-opencode-directory` for its Application
Support workspace. Model selections use the catalog's `id`, not the upstream
provider's `modelID`. Available text-output models remain selectable regardless
of release status, including alpha and beta. Selecting a thinking variant for the
floating default saves the resolved catalog model alongside it; existing explicit
model selections are preserved.

One-shot generation accepts a prompt and optional model reference and returns
`data.text`. Current OpenCode serves it at `/api/experimental/generate`; Hex
falls back to the older `/api/generate` only when the server answers the new
route with HTTP 404, within the same generation deadline. It is stateless and uses the server's base configuration, not the
catalog request's directory header. Existing saved selections whose catalog and
upstream IDs match need no migration. Previously broken alias selections can be
fixed by reselecting the model; Hex does not guess an ID migration.

## Updating The Baseline

Pin an exact CLI version and source commit for isolated release-validation runs,
then deliberately update this baseline after testing the contracts above,
deadlines, and cancellation. At the 2026-08-26 audit, the `v2` source branch
published to npm's `dev` tag; `next` was an older beta, not the newest V2 build.
Resolve the published version rather than assuming a moving tag identifies the
desired source commit.

Record the connected server's `/api/info` version. Do not replace a user's
running service to perform compatibility validation.

Keep regression coverage for registration privacy, stale registrations,
loopback-only admission, chunked answers, deadlines, and cancellation.
