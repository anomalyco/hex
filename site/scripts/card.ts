// The 1200 × 630 social card the build draws for its static pages (Labs, the
// iPhone pages): satori lays it out and resvg draws it, so the build needs no
// browser.
import { Resvg } from "@resvg/resvg-js"
import { readFileSync } from "node:fs"
import { join, resolve } from "node:path"
import satori from "satori"

const root = resolve(import.meta.dirname, "..")
const font = (weight: number) => readFileSync(join(root, `node_modules/@fontsource/inter/files/inter-latin-${weight}-normal.woff`))
const fonts = [
  { name: "Inter", data: font(500), weight: 500 as const, style: "normal" as const },
  { name: "Inter", data: font(600), weight: 600 as const, style: "normal" as const },
]
const icon = `data:image/png;base64,${readFileSync(join(root, "scripts/og/hex-icon.png")).toString("base64")}`

type Node = { type: string; props: Record<string, unknown> }
const el = (type: string, style: Record<string, unknown>, children?: unknown, props: Record<string, unknown> = {}): Node => ({
  type,
  props: { style, children, ...props },
})

function lineSize(line: string): number {
  return line.length <= 70 ? 36 : 30
}

function titleSize(title: string): number {
  if (title.length <= 16) return 108
  if (title.length <= 26) return 88
  return 72
}

// The card: the icon and the kicker ("Hex Labs") above, the title big, the
// line, and the footer, on the home page card's black with its soft light.
export async function card(kicker: string, title: string, line: string, footer: string): Promise<Buffer> {
  const tree = el(
    "div",
    {
      width: 1200,
      height: 630,
      display: "flex",
      color: "#f3f3f0",
      fontFamily: "Inter",
      backgroundColor: "#070707",
      backgroundImage: "radial-gradient(circle at 160px 90px, rgba(255,255,255,0.075), rgba(255,255,255,0) 520px)",
    },
    el("div", { position: "absolute", top: 72, right: 84, bottom: 68, left: 84, display: "flex", flexDirection: "column", justifyContent: "space-between" }, [
      el("div", { display: "flex", alignItems: "center", gap: 18 }, [
        el("img", { width: 76, height: 76, margin: "-8px" }, undefined, { src: icon, width: 76, height: 76 }),
        el("div", { display: "flex", fontSize: 34, fontWeight: 600, letterSpacing: "-0.02em" }, kicker),
      ]),
      el("div", { display: "flex", flexDirection: "column" }, [
        el(
          "div",
          { display: "flex", fontSize: titleSize(title), fontWeight: 600, letterSpacing: "-0.045em", lineHeight: 1.02, maxWidth: 1032 },
          title,
        ),
        el("div", { display: "flex", marginTop: 26, color: "#a3a5ab", fontSize: lineSize(line), fontWeight: 500, letterSpacing: "-0.015em", lineHeight: 1.3, maxWidth: 980 }, line),
      ]),
      el("div", { display: "flex", color: "#6f7177", fontSize: 26, fontWeight: 500 }, footer),
    ]),
  )
  const svg = await satori(tree as Parameters<typeof satori>[0], { width: 1200, height: 630, fonts })
  return new Resvg(svg, { fitTo: { mode: "width", value: 1200 } }).render().asPng()
}
