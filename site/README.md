# Hex website

`https://anoma.ly/hex` is the one Hex site (dev: `https://dev.anoma.ly/hex`).
`bun run dev` starts Vite. `bun run build [--mode dev]` checks TypeScript and
builds Cloudflare Build Output with base `/hex/`; `bun run check:downloads`
checks that every download link serves a file. `cloudflare.config.ts` defines
both Workers. CI deploys them, never a laptop: Site builds without secrets and
`.github/release/deploy-site.sh` deploys the verified Build Output
(`cf deploy --prebuilt --mode <mode>`), refusing any Worker, route, or binding
other than this config's (docs/releasing.md). `worker.ts` serves the
site and, at `/hex/files/`, the environment's R2 bucket of release files, and
301s other hosts and plain HTTP to the canonical host.

The download section (`src/Download.tsx`) is one install switcher and nothing
else: macOS, Windows, and Linux tabs of equal width with a sliding pill, each
with every way to install there in at most three rows (Mac: DMG, Homebrew;
Windows: installer, PowerShell script; Linux: script, package, binary), a small
switch inside a row where it has variants (x64/ARM64 for the Windows installer
and the Linux binary, which the Linux packages follow too; Debian, Fedora, Nix,
and AUR for the Linux package), and the download button or copyable command.
All three stacks share one grid cell, so switching tabs never moves the page.
An Intel Mac sees that Hex requires Apple silicon instead of a download. It
selects the visitor's OS and architecture on first paint and refines them with
UA client hints. Versions, URLs, and commands live in `src/release.ts`; file
links name `/hex/download/<target>`, which the Worker sends to the stable
release once there is one and to the newest dev build until then. The AUR
package appears from the first build after a stable release, which pushes
`hex-bin`. Add
`?platform=mac|mac-intel|windows-x64|windows-arm64|linux|linux-arm64|mobile`
to see each default.

The hero is a show (`src/shout/`, plain JS): Kit's notifications pile up around a
floating head that answers them out loud, dictates prompts into an OpenCode window
that fixes the complaints, powers up, and finally sprays the inbox clean. WebGPU
renders it, with a Canvas 2D fallback (`?canvas2d` forces it).

- The prerendered page shows `public/shout-idle.webp`, a still of the idle head, in
  `.show__head`. The engine chunk loads when the browser is idle or the visitor
  reaches for the ⌥ key (phones wait for the tap), then draws the same head into that box (the canvas reads the
  box, 3.2 head radii wide, so nothing moves).
- Nothing audible or heavy loads before the key press: the voice bank and its
  manifest are fetched on start, the opening clips first. The sound toggle in the
  header remembers its choice in `localStorage`.
- It pauses when the tab is hidden or the hero is off screen, runs the idle head at
  30 fps on a timer (no frame callback between its frames), caps the pixel ratio
  (1.5 on phones), and thins the messages on small screens. The option key's glow
  pulses at 30 fps from `src/glow.ts`, set by the key until the engine runs and then
  by the engine in the head's frame: as a CSS animation it restyled the page on every
  vsync. Reduced motion keeps the still until the key is pressed, then runs only
  the calm stage with no shake or flashes.
- Dax, Jay and Frank (teammates) appear with their real X avatars; everyone else is
  invented with a generated portrait (`src/shout/assets/people.webp`). Brand glyphs
  are Simple Icons (CC0) and Lucide (ISC); OpenCode's mark is from opencode.ai.

`VITE_SHOUT_CAPTURE=1` builds a capture variant with a deterministic frame stepper
(`?capture`, `window.__shout`) and an audio play log; production builds strip it.
Regenerate the idle still after changing the head:

```sh
VITE_SHOUT_CAPTURE=1 bunx vite --port 5531
HEX_SITE_URL=http://127.0.0.1:5531 bun run generate:poster
```

## Performance

`perf/hex.bench.ts` is a [strobe](https://github.com/Hona/strobe) bench: load to
the live head, the idle head for 8 s, and the first 20 s of the show after a click
on the key. `strobe.json` builds the site and serves the Build Output under `/hex/`
with `perf/serve.ts`. From the repository root, against a checkout of `dev` that
has the same two files:

```sh
strobe bench site/perf/hex.bench.ts --base <dev checkout> --head .
```

## Social image

The Open Graph and Twitter image is `public/og.png` (1200 × 630): the app icon,
the name, and one line. `scripts/og/hex-icon.png` is the 1024 px icon exported
from the built app (`iconutil -c iconset Hex.app/Contents/Resources/AppIcon.icns`).
Regenerate with `bun run generate:og`, bump the `?v=` on both image tags in
`index.html`, rebuild, deploy, and fetch the published PNG.

Messages also plays `public/og.mp4` (`og:video`) inline on a loop in place of the
image; other apps keep the PNG. It's a 4.5 s, 1200 × 630, 60 fps silent H.264
loop: the icon speaks and the words pile up under it
(`scripts/og-video/hex-pour.js`, matter.js physics, deterministic per frame so
the last frame meets the first). `bun run generate:og-video [hex-line|hex-key]`
renders it through Chromium and ffmpeg; bump the `?v=` on both `og:video` tags
after changing it. The Worker answers Range requests for `.mp4`, which Safari
and AVFoundation need.
