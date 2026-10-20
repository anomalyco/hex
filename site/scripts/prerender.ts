// Puts the server-rendered Mac page into dist/index.html, so the copy and the
// download button are in the HTML before JavaScript runs; main.tsx hydrates it.
// Also stamps the release version into the JSON-LD.
import { readFileSync, rmSync, writeFileSync } from "node:fs"
import { render } from "../dist-server/prerender.js"
import { MAC_VERSION } from "../src/release"

const path = new URL("../dist/index.html", import.meta.url)
const html = readFileSync(path, "utf8")
const marker = '<div id="root"></div>'
if (!html.includes(marker)) throw new Error(`dist/index.html has no ${marker}`)
writeFileSync(
  path,
  html.replace(marker, `<div id="root">${render()}</div>`).replaceAll("__HEX_VERSION__", MAC_VERSION),
)
rmSync(new URL("../dist-server", import.meta.url), { recursive: true, force: true })
