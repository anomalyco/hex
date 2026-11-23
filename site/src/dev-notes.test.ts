// The dev changelog's ?since= range: what an install sees between the build
// it runs and the newest published one.
import { expect, test } from "bun:test"
import { type DevNotes, renderDevNotes } from "./dev-notes"

const download = "/hex/download/dev"

const notes: DevNotes = {
  schema: 1,
  builds: [
    {
      version: "0.0.0-dev-4",
      commit: "d".repeat(40),
      published_at: "2026-11-12T00:00:00Z",
      items: [{ title: "Newest", url: "https://github.com/anomalyco/hex/pull/4" }],
    },
    {
      version: "0.0.0-dev-3",
      commit: "c".repeat(40),
      published_at: "2026-11-11T00:00:00Z",
      items: [{ title: "Middle", url: "https://github.com/anomalyco/hex/pull/3" }],
    },
    {
      version: "0.0.0-dev-2",
      commit: "b".repeat(40),
      published_at: "2026-11-10T00:00:00Z",
      items: [{ title: "Oldest", url: "https://github.com/anomalyco/hex/pull/2" }],
    },
  ],
}

function sections(html: string): string[] {
  return [...html.matchAll(/<section id="([^"]+)"/g)].map((match) => match[1])
}

test("a version older than the newest lists only the builds after it", () => {
  const html = renderDevNotes(notes, download, "0.0.0-dev-2")
  expect(sections(html)).toEqual(["0.0.0-dev-4", "0.0.0-dev-3"])
  expect(html).toContain("Changes since 0.0.0-dev-2, the build you run.")
  expect(html).not.toContain('id="0.0.0-dev-2"')
})

test("the newest version says there is nothing newer", () => {
  const html = renderDevNotes(notes, download, "0.0.0-dev-4")
  expect(sections(html)).toEqual([])
  expect(html).toContain("No changes since 0.0.0-dev-4, the newest dev build.")
  expect(html).toContain('href="/hex/dev">Every dev build</a>')
  expect(html).not.toContain("No dev build notes yet.")
})

test("a version older than the notes go lists every build", () => {
  const html = renderDevNotes(notes, download, "0.0.0-dev-1")
  expect(sections(html)).toEqual(["0.0.0-dev-4", "0.0.0-dev-3", "0.0.0-dev-2"])
})

test("no version lists every build", () => {
  const html = renderDevNotes(notes, download, null)
  expect(sections(html)).toEqual(["0.0.0-dev-4", "0.0.0-dev-3", "0.0.0-dev-2"])
  expect(html).not.toContain('class="since"')
})

test("notes with no builds still say there are no notes", () => {
  const html = renderDevNotes({ schema: 1, builds: [] }, download, null)
  expect(html).toContain("No dev build notes yet.")
})
