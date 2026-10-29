// Renders the Open Graph / Twitter card (public/og.png, 1200 × 630): the app icon, the name,
// and one line. scripts/og/hex-icon.png is the 1024 px icon exported from the built Hex.app
// (iconutil -c iconset Hex.app/Contents/Resources/AppIcon.icns), so it carries Icon
// Composer's glass and shadow.
import { chromium } from "playwright"
import { readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"

const icon = readFileSync(new URL("./og/hex-icon.png", import.meta.url)).toString("base64")
const font = readFileSync(new URL("../node_modules/@fontsource-variable/inter/files/inter-latin-wght-normal.woff2", import.meta.url)).toString("base64")

const html = `<!doctype html>
<style>
  @font-face { font-family: Inter; src: url(data:font/woff2;base64,${font}) format("woff2"); font-weight: 100 900; }
  html, body { margin: 0; width: 1200px; height: 630px; background: #070707; }
  body {
    display: flex; align-items: center; justify-content: center; gap: 48px; padding: 0 0 6px; box-sizing: border-box;
    background: radial-gradient(circle at 390px 315px, rgba(255, 255, 255, 0.055), transparent 340px), #070707;
    color: #f3f3f0; font-family: Inter, sans-serif; font-feature-settings: "cv02", "cv03", "cv04", "cv11", "ss01";
    -webkit-font-smoothing: antialiased;
  }
  img { width: 340px; height: 340px; margin-left: -24px; }
  h1 { margin: 0; font-size: 112px; font-weight: 560; letter-spacing: -0.05em; line-height: 1; }
  p { margin: 22px 0 0; color: #9b9da3; font-size: 32px; font-weight: 450; letter-spacing: -0.015em; line-height: 1.3; max-width: 15em; }
</style>
<img src="data:image/png;base64,${icon}" alt="">
<div><h1>Hex</h1><p>Local voice dictation for Mac, Windows, and Linux.</p></div>`

const browser = await chromium.launch({ headless: true })
try {
  const page = await browser.newPage({ viewport: { width: 1200, height: 630 }, deviceScaleFactor: 1 })
  await page.setContent(html)
  await page.evaluate(() => document.fonts.ready)
  await page.screenshot({ path: fileURLToPath(new URL("../public/og.png", import.meta.url)) })
} finally {
  await browser.close()
}
