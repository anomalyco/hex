# Hex website

`https://anoma.ly/hex` is the one Hex site (dev: `https://dev.anoma.ly/hex`).
`bun run dev` starts Vite. `bun run build [--mode dev]` checks TypeScript and
builds Cloudflare Build Output with base `/hex/`; `bun run check:downloads`
checks that every download link serves a file. `cloudflare.config.ts` defines
both Workers. CI deploys them, never a laptop: Site builds without secrets and
`.github/release/deploy-site.sh` deploys the verified Build Output
(`cf deploy --prebuilt --mode <mode>`), refusing any Worker, route, or binding
other than this config's (docs/releasing.md). `worker.ts` serves the
site and, at `/hex/files/`, the release files: the installers and legacy
appcast built with the site, the dev notes from the Release workflow's
`dev-notes` branch, and the frozen 2.3.1 feeds and artifacts read from
`hex-production-files`. It 301s other hosts and plain HTTP to the canonical
host.

The download section (`src/Download.tsx`) is one install switcher and nothing
else: macOS, Windows, Linux, and Mobile tabs of equal width with a sliding pill,
each with every way to install there in at most three rows (Mac: DMG, Homebrew;
Windows: installer, PowerShell script; Linux: script, package, binary; Mobile:
iOS App Store and Android Hex Preview), a small switch inside a row where it has
variants (x64/ARM64 for the Windows installer and the Linux binary, which the
Linux packages follow too; Debian, Fedora, Nix, and AUR for the Linux package),
and the download, open, or copy button. All four stacks share one grid cell, so
switching tabs never moves the page.
An Intel Mac sees that Hex requires Apple silicon instead of a download. It
selects the visitor's OS and architecture on first paint and refines them with
UA client hints. Versions, URLs, and commands live in `src/release.ts`; file
links name `/hex/download/<target>`, which the Worker sends to the stable
release once there is one and to the newest dev build until then. The AUR
package appears from the first build after a stable release, which pushes
`hex-bin`. Add
`?platform=mac|mac-intel|windows-x64|windows-arm64|linux|linux-arm64|mobile`
to see each default. The hero's key and its "Hold …" line follow the selected
tab, so clicking Windows reads Alt and clicking Linux reads Alt+Space; until a
tab is clicked they follow the detected platform.

Scripting off: the tabs and the switches inside a row are buttons, so the page
shows every platform and variant at once instead. `index.html`'s `<noscript>`
stylesheet hides the switcher, the demo key, and the detected-platform line, and
shows `src/Download.tsx`'s `.install__plain` list (each OS, architecture, and
package under its own label, with the commands wrapping instead of truncating)
and `src/App.tsx`'s neutral line naming Mac, Windows, and Linux. `src/styles.css`
keeps both hidden while scripts run, so the page is unchanged there.

The hero is a show (`src/shout/`, plain JS): Kit's notifications pile up around a
floating head that answers them out loud, dictates prompts into an OpenCode window
that fixes the complaints, powers up, and finally sprays the inbox clean. WebGPU
renders it, with a Canvas 2D fallback (`?canvas2d` forces it).

- The prerendered page shows `public/shout-idle.webp`, a still of the idle head, in
  `.show__head`. The engine chunk loads when the browser is idle or the visitor
  reaches for the key (phones wait for the tap), then draws the same head into that box (the canvas reads the
  box, 3.2 head radii wide, so nothing moves). The keycap names the selected
  OS's shortcut: ⌥ option on the Mac, alt on Windows, and alt + space on Linux.
  The key the show shatters keeps that legend too (`start({ key })` reaches
  `cards.makeKeycap`, which draws the ⌥ glyph only for option).
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

## Changelog

`/hex/changelog` is the stable changelog: every `docs/releases/<X.Y.Z>.md`,
newest first, each under its version's anchor (`#3.5.0`) with a link to its
GitHub release (`v<version>` from 3.0.0, `app-v<version>` from 2.1.16 through
2.3.1, none before). `scripts/changelog.ts` reads the folder at build and
`src/changelog.ts` renders it in the Labs pages' look, so a release appears
when the site next deploys and nothing is written by hand. Files that aren't
`<X.Y.Z>.md` (`linux-wayland-beta.md`) are not releases and are left out. The
notes' Markdown is a title, `###` sections, paragraphs, bullets, `>` callouts,
and inline code, bold, links, and `#123` issue references; anything else
reads as a paragraph. `/hex/dev`, the dev builds' notes, links it and is
linked from it, and the landing page's footer links it.

## Meetings

`/hex/meetings/` is the Meetings showcase, a standalone page (plain HTML, ES
modules, CSS, and a voice bank) built outside the site and kept in
`public/meetings/` exactly as it loads. The landing page links it from the
header and the footer. Swap in a new cut with
`bun run import:meetings <bundle folder>`: it follows the page's references from
`index.html` (`scripts/meetings/bundle.ts`) and copies only what the page loads,
leaving build scripts, shader sources, reports, and raw audio behind, and drops
session ids and the bundle's own social tags. The build (`scripts/meetings/build.ts`)
writes the canonical address and social tags for its origin and refuses a
missing reference or a file over Workers' 25 MiB asset limit. The Worker
redirects `/hex/meetings` to `/hex/meetings/` so the page's relative paths
resolve. Its card, `public/meetings/og-meetings.png`, shows the Hex mark,
"Hex Meetings", and the meeting pill and nothing of the show; regenerate it with
`bun run generate:og-meetings` (`CHROMIUM_PATH` names another Chromium when
Playwright's isn't downloaded).

## Search and agents

The landing page keeps its TV hero and puts the crawlable copy below it, on
the install stack's width, hairlines, and text spine. `src/Below.tsx` is one
spec sheet (four facts over a 2×2 of features, each an `h2`, one sentence, and
a line of mono tags), the questions as closed native `<details>` (no script,
every answer in the HTML), and the footer's links. `index.html` carries the
matching `FAQPage`, `SoftwareApplication`, `Organization`, `WebSite`, and
`VideoObject` graph. `seo.test.ts` checks the headings, that no paragraph
there passes 22 words (260 in all), and that every visible question and answer
is its structured copy word for word.

`/hex/mcp` (`src/mcp.ts`, `scripts/mcp.ts`) is the MCP page: where `hex` is on
macOS, Windows, and Linux, one tab per client (OpenCode, Claude Code, Claude
Desktop, Cursor; radio buttons and CSS, with every card in one grid cell so a
tab never moves the page), and the five tools `src/meeting/mcp.rs` serves. Its
one script only reveals the copy buttons and makes them work.
`/hex/vs-wispr-flow` and `/hex/vs-superwhisper` (`src/compare.ts`) compare Hex
with each app's own public pages, sourced and dated on the page (`CHECKED`),
with a "may fit better" list; `/hex/linux` (`src/linux.ts`) has this build's
install commands and each desktop's shortcut and paste path. `scripts/pages.ts`
writes all three, their cards, and their sitemap entries. The Meetings build
adds `src/meetings-page.ts`'s crawlable section, title, and FAQPage under the
showcase. `intent.test.ts` checks every visible question against its
structured copy. Update a comparison's facts and its date together.
`public/llms.txt` is the agent index, `public/llms-full.txt` the single-file
reference, and `public/skill.md` the skill an agent loads.
`public/sitemap.xml` lists the static routes and each build script adds its
own pages.

Canonicals, social tags, and sitemap URLs always name `anoma.ly` even in a dev
build (`canonicalOriginForMode`). `worker.ts` puts `X-Robots-Tag: noindex,
nofollow` on every response from another host, so `dev.anoma.ly` is never the
indexed copy, and it sets the charset on `.md` and `.txt` routes.

## Performance

`perf/hex.bench.ts` is a [strobe](https://github.com/anomalyco/strobe) bench: load to
the live head, the idle head for 8 s, and the first 20 s of the show after a click
on the key. `strobe.json` builds the site and serves the Build Output under `/hex/`
with `perf/serve.ts`. strobe builds and serves both variants with this checkout's
`strobe.json` and `perf/`, so any commit works as the base. From the repository root:

```sh
strobe bench site/perf/hex.bench.ts --base origin/dev --head .
strobe bench site/perf/hex.bench.ts --head . --check            # each case once, pass or fail
strobe report experiment <exp_…> --format pr --update-pr <n>     # the results block in PR n
```

In a PR, write your own text (what strobe showed, what changed) and let
`--update-pr` place strobe's generated results block: it replaces only its own
block after a rerun. Don't edit inside it. The bench's `expect` refuses a block
whose page isn't the landing page (`WrongScreen`).

The show draws at its own frame rate, not the display's: `canvas.frames` and
`canvas.interval.p95` count the show's WebGPU frames, where presented frames count
the display's refreshes. `gpu.compile` is the GPU process's shader and pipeline work
during a load.

## Social image

The Open Graph and Twitter image is `public/og.png` (1200 × 630): the app icon,
the name, and one line. `scripts/og/hex-icon.png` is the 1024 px icon exported
from the built app (`iconutil -c iconset Hex.app/Contents/Resources/AppIcon.icns`).
Regenerate with `bun run generate:og`, bump the `?v=` on both image tags in
`index.html`, rebuild, deploy, and fetch the published PNG.

The Labs pages (`/hex/labs`, `/hex/labs/<id>`) each have their own card,
drawn at build by `scripts/labs.ts` with satori and resvg (no browser): the
icon, "Hex Labs", the lab's title and line, and its OSes. Their `?v=` is a
hash of what the card shows. `src/labs.json` comes from the app's Labs
registry; never edit it by hand (`HEX_UPDATE_LABS_JSON=1 scripts/check
labs::web` from the repository root rewrites it).

Messages also plays `public/og.mp4` (`og:video`) inline on a loop in place of the
image; other apps keep the PNG. It's a 4.5 s, 1200 × 630, 60 fps silent H.264
loop: the icon speaks and the words pile up under it
(`scripts/og-video/hex-pour.js`, matter.js physics, deterministic per frame so
the last frame meets the first). `bun run generate:og-video [hex-line|hex-key]`
renders it through Chromium and ffmpeg; bump the `?v=` on both `og:video` tags
after changing it. The Worker answers Range requests for `.mp4`, which Safari
and AVFoundation need.
