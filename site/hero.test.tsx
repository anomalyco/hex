// The hero's keycap wears the shortcut its copy names, on every OS (#2330).
import { expect, test } from "bun:test"
import { renderToStaticMarkup } from "react-dom/server"
import { OptionKey } from "./src/Shout"
import { OS_SHORTCUTS, type Os } from "./src/shortcuts"

const keycap = (os: Os) => renderToStaticMarkup(<OptionKey os={os} phase="idle" onStart={() => {}} onIntent={() => {}} />)

test("the keycap wears the selected OS's shortcut", () => {
  const mac = keycap("mac")
  expect(mac).toContain('class="keycap__glyph"') // the Mac's ⌥
  expect(mac).toContain(">option<")

  for (const os of ["windows", "linux"] as const) expect(keycap(os)).not.toContain("keycap__glyph")
  expect(keycap("windows")).toContain(">alt<")
  expect(keycap("linux")).toContain('alt<span class="keycap__chord">+ space</span>')
})

test("the keycap and the copy name the same key", () => {
  for (const os of ["mac", "windows", "linux"] as const) {
    const { copy, legend } = OS_SHORTCUTS[os]
    expect(copy.toLowerCase()).toContain(legend.split(" ")[0])
  }
})
