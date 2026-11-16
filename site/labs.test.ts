// The Labs pages: what each lab's page links and says, and the index's order.
import { expect, test } from "bun:test"
import { cardLine, cardVersion, inSource, type Lab, newestFirst, renderIndex, renderLab } from "./src/labs"
import catalog from "./src/labs.json"

const origin = "https://dev.anoma.ly/hex"
const labs = catalog.labs as Lab[]
const yolo = labs.find((lab) => lab.id === "yolo-mode")!

test("a lab's page opens it in Hex and offers the Dev build until stable has it", () => {
  const html = renderLab(yolo, { dev: true }, origin)
  expect(html).toContain('href="hex://labs/yolo-mode">Open in Hex</a>')
  expect(html).toContain('href="/hex/download/dev">Get Hex Dev for Mac</a>')
  expect(html).toContain('href="/hex/download/dev/windows-x64-setup">Windows</a>')
  expect(html).toContain("In the Dev build, not yet in a stable release.")
  expect(html).toContain('<meta property="og:url" content="https://dev.anoma.ly/hex/labs/yolo-mode">')
  expect(html).toContain(`<meta property="og:image" content="https://dev.anoma.ly/hex/labs/yolo-mode.png?v=${cardVersion(yolo)}">`)
  expect(html).toContain('<meta name="twitter:card" content="summary_large_image">')
  expect(html).toContain("<title>YOLO mode · Hex Labs</title>")

  const stable = renderLab(yolo, { stable: "3.5.0" }, origin)
  expect(stable).toContain('href="/hex/">Get Hex</a>')
  expect(stable).toContain("In Hex 3.5.0 and later.")
})

test("a lab the Mac doesn't have offers no deep link", () => {
  const windows = labs.find((lab) => !lab.oses.includes("macos"))!
  const html = renderLab(windows, { dev: true }, origin)
  expect(html).not.toContain("hex://")
  expect(html).toContain(`hex labs on ${windows.id}`)
})

test("the stable release has a lab when its source names it", () => {
  expect(inSource(yolo, 'Self::YoloMode => "yolo-mode",')).toBe(true)
  expect(inSource(yolo, 'Self::Yolo => "yolo",')).toBe(false)
})

test("the index lists every lab, newest first", () => {
  const ordered = newestFirst(labs)
  expect(ordered).toHaveLength(labs.length)
  for (let index = 1; index < ordered.length; index++) {
    expect(ordered[index - 1].added >= ordered[index].added).toBe(true)
  }
  const html = renderIndex(labs, origin)
  for (const lab of labs) expect(html).toContain(`href="/hex/labs/${lab.id}"`)
})

test("a card's line keeps the first sentence of a long one", () => {
  expect(cardLine("Send dictations automatically")).toBe("Send dictations automatically.")
  expect(cardLine(`${"a".repeat(140)}. More`)).toBe(`${"a".repeat(140)}.`)
})
