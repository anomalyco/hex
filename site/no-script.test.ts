// The landing page without scripts. index.html's <noscript> stylesheet hides
// the controls that need them — the OS tabs, the architecture and package
// switches inside a row, and the demo key — and shows src/Download.tsx's plain
// list of every platform and variant; src/styles.css keeps that list and the
// neutral hero line hidden while scripts run. The halves only work while the
// class names match, and nothing else exercises the scripting-off page.
import { expect, test } from "bun:test"
import { readFileSync } from "node:fs"

const read = (path: string) => readFileSync(new URL(path, import.meta.url), "utf8")

const html = read("./index.html")
const css = read("./src/styles.css")
const app = read("./src/App.tsx")
const download = read("./src/Download.tsx")
const shout = read("./src/Shout.tsx")
const noScript = html.match(/<noscript>([\s\S]*?)<\/noscript>/)?.[1] ?? ""

test("scripting off hides the inert controls", () => {
  expect(noScript).toMatch(/\.keycap[^}]*display:\s*none\s*!important/)
  expect(noScript).toMatch(/\.install__os[^}]*display:\s*none\s*!important/)
  expect(noScript).toMatch(/\.install__panels[^}]*display:\s*none\s*!important/)
  expect(noScript).toMatch(/\.hero__device[^}]*display:\s*none\s*!important/)
})

test("scripting off shows the plain list and the neutral hero line", () => {
  expect(noScript).toMatch(/\.install__plain\s*{[^}]*display:\s*flex/)
  expect(noScript).toMatch(/\.hero__any\s*{[^}]*display:\s*block/)
})

test("scripts on keep the plain list and the neutral hero line hidden", () => {
  expect(css).toMatch(/\.install__plain\s*{[^}]*display:\s*none/)
  expect(css).toMatch(/\.hero__any\s*{[^}]*display:\s*none/)
})

test("every class the swap names is on the page", () => {
  expect(shout).toContain("keycap")
  for (const name of ["install__os", "install__panels", "install__plain"]) {
    expect(download).toContain(name)
  }
  for (const name of ["hero__device", "hero__any"]) expect(app).toContain(name)
})

test("mobile platform lands on the mobile tab with iOS and Android links", async () => {
  const { osOf } = await import("./src/Download")
  expect(osOf("mobile")).toBe("mobile")
  const { render } = await import("./src/prerender")
  const rendered = render()
  expect(rendered).toContain('id="install-tab-mobile"')
  expect(rendered).toContain('id="install-panel-mobile"')
  expect(rendered).toContain('id="install-plain-mobile"')
  expect(rendered).toContain('href="/hex/ios"')
  expect(rendered).toContain('href="/hex/android"')
})
