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

The download section (`src/Download.tsx`) is an OS switcher (macOS, Windows,
Linux) over every way to install on the chosen OS, all at once: download
buttons (Windows and Linux for both x64 and ARM64) and one copyable row per
command (Homebrew; PowerShell; the install script, .deb, .rpm, Nix, and from
the first stable release the AUR). Nothing else: no version or helper text.
The visitor's OS and architecture are selected on first paint and refined with
UA client hints; the .deb and .rpm rows follow the detected architecture.
Versions, URLs, and commands live in `src/release.ts`; file links name
`/hex/download/<target>`, which the Worker sends to the stable release once
there is one and to the newest dev build until then. The AUR tab appears from
the first build after a stable release, which pushes `hex-bin`. Add
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
  30 fps, caps the pixel ratio (1.5 on phones), and thins the messages on small
  screens. Reduced motion keeps the still until the key is pressed, then runs only
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
