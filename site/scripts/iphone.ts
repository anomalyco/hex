// Writes the Hex for iPhone pages into the built site: iphone.html
// (/hex/iphone, the App Store support URL), privacy.html (/hex/privacy, its
// privacy policy URL), and android.html (/hex/android, Hex Preview's download
// and setup), a social card beside each (scripts/card.ts), and all three
// in the sitemap.
//
//   bun scripts/iphone.ts --mode dev
import { getWorkerAssetsDir } from "@cloudflare/build-output-utils"
import { readFileSync, writeFileSync } from "node:fs"
import { join, resolve } from "node:path"
import { parseArgs } from "node:util"
import { canonicalOriginForMode } from "../src/base"
import { ANDROID_PATH, CARDS, IPHONE_PATH, PRIVACY_PATH, renderAndroid, renderIphone, renderPrivacy } from "../src/iphone"
import { card } from "./card"

const { values } = parseArgs({ options: { mode: { type: "string" }, out: { type: "string" } } })
const origin = canonicalOriginForMode(values.mode)
const assets = values.out ?? getWorkerAssetsDir(resolve(import.meta.dirname, ".."))
const address = (path: string) => `${origin.replace("https://", "")}${path.slice("/hex".length)}`
const site = (path: string) => join(assets, path.slice("/hex/".length))

writeFileSync(site(`${IPHONE_PATH}.html`), renderIphone(origin))
writeFileSync(site(`${IPHONE_PATH}.png`), await card("Hex for iPhone", CARDS.iphone.title, CARDS.iphone.line, address(IPHONE_PATH)))
writeFileSync(site(`${PRIVACY_PATH}.html`), renderPrivacy(origin))
writeFileSync(site(`${PRIVACY_PATH}.png`), await card("Hex for iPhone", CARDS.privacy.title, CARDS.privacy.line, address(PRIVACY_PATH)))
writeFileSync(site(`${ANDROID_PATH}.html`), renderAndroid(origin))
writeFileSync(site(`${ANDROID_PATH}.png`), await card("Hex for Android", CARDS.android.title, CARDS.android.line, address(ANDROID_PATH)))

const sitemap = join(assets, "sitemap.xml")
const urls = [IPHONE_PATH, PRIVACY_PATH, ANDROID_PATH].map((path) => `  <url><loc>${origin}${path.slice("/hex".length)}</loc></url>`)
writeFileSync(sitemap, readFileSync(sitemap, "utf8").replace("</urlset>", `${urls.join("\n")}\n</urlset>`))
console.log("iPhone and Android: 3 pages (iphone, privacy, android).")
