// Renders the idle-head poster (public/shout-idle.webp) that the page shows before the show's
// renderer loads. Needs a capture build, which exposes the engine's frame stepper:
//
//   VITE_SHOUT_CAPTURE=1 bunx vite --port 5531
//   HEX_SITE_URL=http://127.0.0.1:5531 bun run generate:poster
//
// Requires cwebp (brew install webp).
import { chromium } from "playwright"
import { fileURLToPath } from "node:url"

const site = process.env.HEX_SITE_URL ?? "http://127.0.0.1:5531"
const png = fileURLToPath(new URL("../node_modules/.cache/shout-idle.png", import.meta.url))
const webp = fileURLToPath(new URL("../public/shout-idle.webp", import.meta.url))

const browser = await chromium.launch({ headless: true, args: ["--enable-unsafe-webgpu", "--use-angle=metal", "--mute-audio"] })
try {
  // The largest head box (20rem), at 2×.
  const page = await browser.newPage({ viewport: { width: 1440, height: 1100 }, deviceScaleFactor: 2 })
  await page.goto(`${site}/?capture`)
  await page.waitForFunction(() => document.querySelector("main")?.dataset.phase === "live" && "__shout" in window)
  await page.addStyleTag({ content: ".header, .show, .hero { visibility: hidden !important }" })
  const box = await page.locator(".show__head").boundingBox()
  if (!box) throw new Error("no .show__head")
  await page.screenshot({ path: png, clip: box })
} finally {
  await browser.close()
}
const webpResult = Bun.spawnSync(["cwebp", "-quiet", "-q", "86", png, "-o", webp], { stdio: ["ignore", "inherit", "inherit"] })
if (webpResult.exitCode !== 0) throw new Error("cwebp failed")
