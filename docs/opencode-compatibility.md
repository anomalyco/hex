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

## Integration Contract

HEX discovers a separately installed `opencode2`, optionally overridden by
`VOICE_CONTROL_OPENCODE_CLI`. It does not bundle or pin the runtime CLI, own the
managed service, or restart or update it. `opencode2 api` discovers or starts
the service through OpenCode's own lifecycle.

Generation uses `opencode2 api get /api/health` to identify the authenticated
active server, starting it through the CLI when needed. `opencode2 debug paths`
locates its state directory. HEX reads endpoint and password together from the
bounded, owner-only service registration matching that server's PID and version.
It never reads or regenerates the configured password, changes service settings,
or includes credentials in diagnostics.

The system curl sends the authenticated request over loopback HTTP. Credentials
and JSON travel in a curl configuration through stdin, never command arguments
or temporary files. Curl configuration files, proxies, and redirects are not
used. Non-loopback endpoints are rejected. Discovery, request writing, and
response waiting share the generation deadline and cancellation signal.

HEX reads `/api/model` and `/api/model/default` in its Application Support
workspace. Model selections use the catalog's `id`, not the upstream provider's
`modelID`. Available text-output models remain selectable regardless of release
status, including alpha and beta. Selecting a thinking variant for the floating
default saves the resolved catalog model alongside it; existing explicit model
selections are preserved.

`/api/generate` accepts a prompt and optional model reference and returns
`data.text`. It is stateless and uses the server's base configuration, not the
catalog request's directory header. Existing saved selections whose catalog and
upstream IDs match need no migration. Previously broken alias selections can be
fixed by reselecting the model; HEX does not guess an ID migration.

## Updating The Baseline

Pin an exact CLI version and source commit for isolated release-validation runs,
then deliberately update this baseline after testing the contracts above,
deadlines, and cancellation. At this audit, the `v2` source branch published to
npm's `dev` tag; `next` was an older beta, not the newest V2 build. Resolve the
published version rather than assuming a moving tag identifies the desired
source commit.

Check the connected server's `/api/health` version as well as the CLI version:
the CLI's `api` command permits a server version mismatch. Do not replace a
user's running service to perform compatibility validation.

Keep regression coverage for body/credential privacy, curl configuration escaping,
large bodies, loopback-only admission, blocked input pipes, and cancellation.
