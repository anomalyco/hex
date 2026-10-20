# Hex website

`hex.kitlangton.dev` is the one Hex site. `bun run dev` starts Vite. `bun run
build` checks TypeScript and builds it; `bun run check:downloads` checks that
every download link serves a file; `bunx wrangler deploy` publishes the Worker,
whose `worker.ts` 301s `hex.kitlangton.com`, plain HTTP, and workers.dev to the
same path on `hex.kitlangton.dev`.

The download section (`src/Download.tsx`) names the visitor's platform on first
paint and refines it with UA client hints: a DMG for Apple silicon Macs, the
x64 or ARM64 `Setup.exe` for Windows, the copyable install command for Linux,
and "Send yourself the link" on phones. Versions and URLs live in
`src/release.ts`. Add `?platform=mac|mac-intel|windows-x64|windows-arm64|linux|mobile`
to see each state.

The wind-up teeth in `src/ChatteringTeeth.tsx` use separate body and foot groups:
the soles remain planted during anticipation, the whole toy travels during the
hop, and the body settles after contact. Wide screens use the perimeter; narrow
screens use the space above the copy. A warm spotlight fades in from 1.4–2.4
seconds, foreshadowing the landing. The toy stays hidden for three seconds, then
hops in from the left to the center above the headline. The light fades away
as roaming begins. Reduced motion reveals the light and toy as still poses at
their respective delays. The canvas is click-through and decorative;
the site has no microphone control or microphone access.

Rounded teeth, teal irises, and cherry-red lacquer use a three-softbox procedural
studio environment for enamel and metal reflections, plus soft directional shadows.
The environment is generated
locally, with no remote model or lighting asset request.

## Social image

The Open Graph and Twitter image is `public/og-teeth.png` (1200 × 630). It is
rendered from the same Three.js model, using `?og` for a deterministic poster pose.
With the dev server running, regenerate after changing the model:

```sh
bunx playwright install chromium
bun run generate:og
# If Vite chose another port:
HEX_SITE_URL=http://127.0.0.1:5174 bun run generate:og
```
Inspect the resulting image, rebuild, then deploy. Verify the production HTML's
image metadata and fetch the published PNG.
