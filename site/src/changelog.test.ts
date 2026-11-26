// The stable changelog: which files in docs/releases are releases and in what
// order, how their Markdown reads, and what each release's section links.
import { expect, test } from "bun:test"
import { readdirSync, readFileSync } from "node:fs"
import { compareVersions, inline, parseNotes, releasesFrom, releaseUrl, renderChangelog, versions } from "./changelog"

const origin = "https://dev.anoma.ly/hex"

function sections(html: string): string[] {
  return [...html.matchAll(/<section class="release" id="([^"]+)"/g)].map((match) => match[1])
}

test("versions sort by number, newest first, and only <X.Y.Z>.md is a release", () => {
  const files = ["2.1.9.md", "3.4.0.md", "linux-wayland-beta.md", "2.1.10.md", "3.5.0.md", "README.md", "3.5.md", "3.5.0-rc.1.md", "3.5.0.txt"]
  expect(versions(files)).toEqual(["3.5.0", "3.4.0", "2.1.10", "2.1.9"])
  expect(compareVersions("2.1.10", "2.1.9")).toBeGreaterThan(0)
  expect(compareVersions("3.0.0", "2.3.1")).toBeGreaterThan(0)
  expect(compareVersions("3.5.0", "3.5.0")).toBe(0)
})

test("notes parse into a title, sections, wrapped bullets, and a quoted callout", () => {
  const notes = parseNotes(
    [
      "## Hex 9.9.9",
      "",
      "> **windows, coming from 9.8:** run setup",
      "> over your copy.",
      ">",
      "> **hyprland:** reload your config.",
      "",
      "### new",
      "",
      "- a bullet that wraps",
      "  onto a second line",
      "- another bullet",
      "",
      "### fixes",
      "",
      "- one fix",
      "",
      "This macOS release uses",
      "build 99999.",
    ].join("\n"),
  )
  expect(notes.title).toBe("Hex 9.9.9")
  expect(notes.blocks).toEqual([
    {
      type: "quote",
      blocks: [
        { type: "paragraph", text: "**windows, coming from 9.8:** run setup over your copy." },
        { type: "paragraph", text: "**hyprland:** reload your config." },
      ],
    },
    { type: "heading", text: "new" },
    { type: "list", items: ["a bullet that wraps onto a second line", "another bullet"] },
    { type: "heading", text: "fixes" },
    { type: "list", items: ["one fix"] },
    { type: "paragraph", text: "This macOS release uses build 99999." },
  ])
})

test("inline Markdown becomes code, bold, links, and issue links, with the rest escaped", () => {
  expect(inline("run `hex meetings search --meeting <id>` & wait")).toBe(
    "run <code>hex meetings search --meeting &#60;id&#62;</code> &#38; wait",
  )
  expect(inline("**windows:** download ([x64](https://anoma.ly/hex/download/windows-x64-setup))")).toBe(
    '<strong>windows:</strong> download (<a href="https://anoma.ly/hex/download/windows-x64-setup">x64</a>)',
  )
  // A bare address ends before the sentence's stop.
  expect(inline("Download Hex from https://anoma.ly/hex.")).toBe('Download Hex from <a href="https://anoma.ly/hex">https://anoma.ly/hex</a>.')
  expect(inline("re-arm a silenced hook (#1910, #2006)")).toBe(
    're-arm a silenced hook (<a href="https://github.com/anomalyco/hex/issues/1910">#1910</a>, <a href="https://github.com/anomalyco/hex/issues/2006">#2006</a>)',
  )
  // A relative link names the notes' folder on GitHub; a scripted one is only its text.
  expect(inline("see [the guide](../linux.md)")).toBe('see <a href="https://github.com/anomalyco/hex/blob/dev/docs/linux.md">the guide</a>')
  expect(inline("[click](javascript:alert(1))")).not.toContain("<a")
  expect(inline("the #070707 page and <script>")).toBe("the #070707 page and &#60;script&#62;")
})

test("a release links its GitHub release under the tag it was published with", () => {
  expect(releaseUrl("3.5.0")).toBe("https://github.com/anomalyco/hex/releases/tag/v3.5.0")
  expect(releaseUrl("3.0.0")).toBe("https://github.com/anomalyco/hex/releases/tag/v3.0.0")
  expect(releaseUrl("2.3.1")).toBe("https://github.com/anomalyco/hex/releases/tag/app-v2.3.1")
  expect(releaseUrl("2.1.16")).toBe("https://github.com/anomalyco/hex/releases/tag/app-v2.1.16")
  // Versions before 2.1.16 were never published on GitHub.
  expect(releaseUrl("2.1.15")).toBeUndefined()
  expect(releaseUrl("2.0.10")).toBeUndefined()
})

test("the page lists each release once, newest first, with its anchor and sections", () => {
  const releases = releasesFrom([
    { name: "2.0.10.md", text: "## HEX 2.0.10\n\n- Preloads the clipboard." },
    { name: "linux-wayland-beta.md", text: "## Linux Wayland And Nix Beta\n\nUnreleased." },
    { name: "3.5.0.md", text: "## Hex 3.5.0\n\n### new\n\n- a new thing (#1728)\n\n### labs (off by default)\n\n- a lab\n\n### fixes\n\n- a fix" },
    { name: "3.4.0.md", text: "## Hex 3.4.0\n\n> **windows:** run setup.\n\n### New\n\n- an older thing" },
  ])
  const html = renderChangelog(releases, origin)
  expect(sections(html)).toEqual(["3.5.0", "3.4.0", "2.0.10"])
  expect(html).toContain('<h2><a href="#3.5.0">3.5.0</a></h2><a class="release__link" href="https://github.com/anomalyco/hex/releases/tag/v3.5.0">GitHub release</a>')
  expect(html).toContain("<h3>New</h3>")
  expect(html).toContain("<h3>Labs (off by default)</h3>")
  expect(html).toContain("<h3>Fixes</h3>")
  expect(html).toContain("<blockquote>\n<p><strong>windows:</strong> run setup.</p>\n</blockquote>")
  expect(html).toContain('<h2><a href="#2.0.10">2.0.10</a></h2></div>')
  expect(html).not.toContain("Unreleased")
  expect(html).toContain('href="/hex/dev">their own notes</a>')
  expect(html).toContain('<link rel="canonical" href="https://dev.anoma.ly/hex/changelog">')
  expect(html).toContain("<title>Hex changelog</title>")
})

// The real notes: every release file reads as a release, so a new one that the
// page can't show fails here before it fails the site build.
test("every release in docs/releases has a title naming its version and notes under it", () => {
  const folder = new URL("../../docs/releases/", import.meta.url)
  const releases = releasesFrom(readdirSync(folder).map((name) => ({ name, text: readFileSync(new URL(name, folder), "utf8") })))
  expect(releases.length).toBeGreaterThan(50)
  expect(releases.map((release) => release.version)).toContain("3.5.0")
  for (let index = 1; index < releases.length; index++) {
    expect(compareVersions(releases[index - 1].version, releases[index].version)).toBeGreaterThan(0)
  }
  for (const release of releases) {
    expect(release.title.toLowerCase()).toBe(`hex ${release.version}`)
    expect(release.blocks.length).toBeGreaterThan(0)
  }
  expect(sections(renderChangelog(releases, origin))).toEqual(releases.map((release) => release.version))
})
