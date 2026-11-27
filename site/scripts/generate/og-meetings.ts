// Renders the Meetings showcase's Open Graph / Twitter card
// (public/meetings/og-meetings.png, 1200 × 630): the Hex mark, "Hex Meetings",
// one line, and the meeting pill Hex shows when a call starts. It gives away
// nothing of the show itself. Regenerate with `bun run generate:og-meetings`.
import { chromium } from "playwright"
import { readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"

const font = readFileSync(new URL("../../node_modules/@fontsource-variable/inter/files/inter-latin-wght-normal.woff2", import.meta.url)).toString("base64")

const mark = (size: number, stroke: string) => `<svg width="${size}" height="${size}" viewBox="0 0 36 36" fill="none" stroke="${stroke}" stroke-linejoin="round">
  <path stroke-width="2.4" d="M18 3.1 30.9 10.55v14.9L18 32.9 5.1 25.45v-14.9Z"/>
  <path stroke-width="2" d="M18 9.1 25.71 13.55v8.9L18 26.9l-7.71-4.45v-8.9Z"/>
  <path fill="${stroke}" stroke="none" d="m18 13.3 4.07 2.35v4.7L18 22.7l-4.07-2.35v-4.7Z"/>
</svg>`

const html = `<!doctype html>
<style>
  @font-face { font-family: Inter; src: url(data:font/woff2;base64,${font}) format("woff2"); font-weight: 100 900; }
  html, body { margin: 0; width: 1200px; height: 630px; background: #070707; }
  body {
    display: flex; flex-direction: column; align-items: center; justify-content: center; box-sizing: border-box; padding-bottom: 4px;
    background: radial-gradient(ellipse 620px 360px at 600px 250px, rgba(255, 255, 255, 0.05), transparent 70%), #070707;
    color: #f3f3f0; font-family: Inter, sans-serif; font-feature-settings: "cv02", "cv03", "cv04", "cv11", "ss01";
    -webkit-font-smoothing: antialiased; text-align: center;
  }
  .mark { display: block; margin-bottom: 30px; }
  h1 { margin: 0; font-size: 96px; font-weight: 560; letter-spacing: -0.045em; line-height: 1; }
  p { margin: 22px 0 0; color: #9b9da3; font-size: 32px; font-weight: 450; letter-spacing: -0.015em; line-height: 1.3; }
  .pill {
    display: flex; align-items: center; gap: 14px; margin-top: 54px; padding: 10px 10px 10px 18px; border-radius: 999px;
    background: linear-gradient(#1b1b1d, #141416); border: 1px solid rgba(255, 255, 255, 0.09);
    box-shadow: 0 1px 0 rgba(255, 255, 255, 0.06) inset, 0 18px 40px rgba(0, 0, 0, 0.55), 0 2px 6px rgba(0, 0, 0, 0.4);
    font-size: 23px; font-weight: 500; letter-spacing: -0.01em;
  }
  .pill svg { display: block; }
  .pill__title { color: #ecece8; }
  .pill__dot { color: #5d5f64; margin: 0 -2px; }
  .pill__state { color: #8d8f95; }
  .record {
    display: flex; align-items: center; gap: 9px; margin-left: 6px; padding: 9px 18px 9px 15px; border-radius: 999px;
    background: #f3f3f0; color: #0b0b0c; font-weight: 560;
  }
  .record i { width: 11px; height: 11px; border-radius: 50%; background: #ec4e40; }
</style>
<div class="mark">${mark(68, "#f3f3f0")}</div>
<h1>Hex Meetings</h1>
<p>Local meeting transcripts &amp; notes on your Mac.</p>
<div class="pill">
  ${mark(26, "#ecece8")}
  <span class="pill__title">Q4 sync</span><span class="pill__dot">·</span><span class="pill__state">detected</span>
  <span class="record"><i></i>Record</span>
</div>`

// CHROMIUM_PATH points at another Chromium when Playwright's own isn't downloaded.
const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM_PATH })
try {
  const page = await browser.newPage({ viewport: { width: 1200, height: 630 }, deviceScaleFactor: 1 })
  await page.setContent(html)
  await page.evaluate(() => document.fonts.ready)
  await page.screenshot({ path: fileURLToPath(new URL("../../public/meetings/og-meetings.png", import.meta.url)) })
} finally {
  await browser.close()
}
