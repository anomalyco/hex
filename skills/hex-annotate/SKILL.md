---
name: hex-annotate
description: Read Hex Annotate screen captures, where someone recorded their screen while talking, clicking, and drawing red marks on it. Use whenever a message names or attaches a .scrawl bundle, a capture’s summary.md, contact-sheet.png, or frames/, when the user says "look at my capture" or refers to something they just recorded or drew, or when you need `hex annotate` to inspect a capture, list its marks, zoom into a moment, or pull frames. It also applies when a message starts with a `[hex-annotate …]` header, or the older `[scrawl …]` header from earlier versions.
license: MIT
---

# Hex captures

Hex's Annotate records someone's screen, their voice, clicks, and red ink, and
packs the result into a `.scrawl` folder (a bundle). They sent it to show you a
problem instead of describing it, so the capture is the bug report. Read it
closely before you answer, and don't ask them to explain again what the capture
already shows. If something is unclear, look closer with `hex annotate zoom` first.

## The Labs switch

`hex annotate inspect`, `zoom`, `frames`, and `skill` are behind **Labs › Agent
tools for captures** (`labs.annotate_agent_tools`), off by default. While it is
off they refuse and say how to turn it on. Turn it on with:

```sh
hex labs on annotate-agent-tools
```

or in Settings › Labs, under Annotate ("Agent tools for captures"). It writes
`settings.json`, so a running Hex takes it at once.

## Finding hex

`command -v hex` finds it when it is on PATH. On macOS, if PATH has none, run
`/Applications/Hex.app/Contents/MacOS/hex` (or the same path under
`~/Applications`), or symlink it into `~/.local/bin`. These four commands
work with Hex closed: they read the bundle, decode its video, and write only
into its `zoom/` folder.

## Read the bundle first

1. `contact-sheet.png` shows every keyframe in time order, left to right and
   top to bottom, each with its number `N` in a small chip in a corner (the
   `#N` in `summary.md`). Open it, or the individual images in `frames/`
   (`frame-03.png` is #3), before changing anything: the marks and words only
   make sense on the images, and an image often shows the problem itself.
2. `summary.md` names what was recorded, its length, and one line per
   keyframe: `#3 (0:10) [clicked "New tab"] "…"`. `⚑` marks a moment the
   person flagged on purpose, and `⟨Name⟩` a new screen an app reported.
3. `scrawl.json` is the full manifest: keyframes, `marks` with shape and box,
   `events`, `transcript.words` with times, the app `context`, and `autocrop`.
   Speech recognition mishears names and code terms, so trust the screen when
   the two disagree.
4. `recording.mp4` or `recording.mov`, and `audio.wav`, are the source media.
   A `.media-pending` file means the video is still being copied from another
   Mac.

Markers at a glance: red strokes, circles, underlines, and arrows are the
person's marks and point at the problem; a ripple is a click, a finger trail a
swipe, and a tapering red trail the laser pointer. Coordinates in `summary.md`
are fractions of the captured area from the top left (x 0.51 is just right of
centre). When several marks sit on one screen the text numbers them in drawing
order (`✎① 0:06 …`, `✎② 0:09 …`); the image shows only the ink, so use
`zoom --mark` to see which ink is which.

Answer with the moments named the way the summary names them, as in
"#3 (0:10)", so the person can find them.

## hex annotate

`inspect`, `zoom`, and `frames` take the bundle as their first argument: a
bundle name (`1792933491953.scrawl`, extension optional), a path, or `latest`
(the default) for the newest capture in Hex's captures folder. `skill` takes
no argument.

```sh
hex annotate inspect <capture>             # summary.md
hex annotate inspect <capture> --marks     # one line per mark
hex annotate inspect <capture> --json      # scrawl.json, pretty
hex annotate zoom <capture> --at 0:21      # a sheet of frames around a moment
hex annotate zoom <capture> --at "the width changes"   # …or a phrase from it
hex annotate zoom <capture> --mark 2       # …or one mark, drawn whole
hex annotate zoom <capture> --mark 1 --frame 4         # ✎① of keyframe #4
hex annotate frames <capture> --from 0:04 --to 0:06 --fps 4
hex annotate skill                         # this guide, from the installed Hex
```

### inspect --marks

Lists every mark as `#2 ✎② 0:09 marked (2 strokes) · m2`: the keyframe, the
number the summary shows, the time, the shape and what it was drawn on, and the
mark's id. `--marks --json` gives the same as JSON, for picking one out
yourself. Use it before `zoom --mark` when the summary's numbering is
ambiguous.

### zoom

Use `zoom` when the problem is motion (an animation, a jump, a flicker) or
happens between keyframes, or when you can't tell what a mark points at. It
writes a dense contact sheet of frames around the moment into
`<bundle>/zoom/zoom-<from>-<to>.png` and prints:

```text
…/captures/1792933491953.scrawl/zoom/zoom-9500-11500.png
0:09.500 → 0:11.500, 12 frames: #1 0:09.500, #2 0:09.681, #3 0:09.863, …
Said: “but the width changes when the panel opens”
```

`--mark` prints the mark's line first, then the same:

```text
#1 ✎② 0:01 marked “the width” · m2
…/captures/1792933491953.scrawl/zoom/zoom-300-2300.png
0:00.300 → 0:02.300, 12 frames: #1 0:00.300, #2 0:00.481, #3 0:00.663, …
Said: “it blue”
```

Open that PNG and look at it before you act on the feedback. The cells are
numbered from #1 within the sheet, so they are not keyframe numbers.
`--span` is the seconds the sheet covers (default 2, a quarter of it before
the moment and the rest after; for a mark, at least the whole mark, centred),
`--frames` the number of cells (default 12, at most 48), and `--full` shows
the whole screen instead of the bundle's auto-crop. `--at` takes a time
(`0:21`, `21.5`, `21`) or a phrase from the narration; `--mark` takes a
mark's number from the summary or its id (`m2`), and `--frame` says which
keyframe's `✎①` the number counts when several have one.

### frames

Extracts one plain PNG per frame across a time range, at `--fps` frames a
second (default 4, 1 to 240), into
`<bundle>/zoom/range-<from>-<to>/frame-001.png` and so on, and prints each
path. Use it when you need stills at exact times, or when a zoom sheet's cells
are too small to read. The range must not end before it starts, `--to` is
clamped to the recording's length, and one call writes at most 240 frames;
narrow the range or lower `--fps`. Without a video, or over an empty range, it
prints the bundle's existing keyframe PNGs in `frames/` that fall in the range
instead.

### Leaving the bundle as it is

`zoom` and `frames` write only into `<bundle>/zoom/`; repeating a command
there replaces the frames it generated. Leave every other file and folder in
the bundle alone: Hex and Scrawl manage them, and retention removes the video
and audio after a while.

## When the CLI isn't there

If `hex` is missing, is an older build without these commands, or the Labs
switch can't be turned on, read the bundle directly: `contact-sheet.png`,
`frames/frame-NN.png`, `summary.md`, and `scrawl.json`. That covers every
keyframe at full size; only `zoom` and `frames` add anything, and they need
the video. After retention has removed `recording.mp4` and `audio.wav`, those
two commands can't decode anything, but the sheet, keyframes, and manifest stay.

Install this skill with
`npx skills add anomalyco/hex --skill hex-annotate`.
