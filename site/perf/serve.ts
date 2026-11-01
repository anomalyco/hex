// Serves the built site (Cloudflare Build Output) under /hex/ as worker.ts does for its assets, for strobe and other
// local measurement: `bun perf/serve.ts <port>` after `bun run build`. Downloads, files and dev notes are not served.
import { join, normalize } from "node:path"

const port = Number(process.argv[2] ?? 4801)
const root = join(import.meta.dir, "../.cloudflare/output/v0/workers/default/assets")

Bun.serve({
  port,
  hostname: "127.0.0.1",
  async fetch(request) {
    const url = new URL(request.url)
    if (url.pathname === "/hex") return Response.redirect(`${url.origin}/hex/`, 301)
    if (!url.pathname.startsWith("/hex/")) return new Response("Not found", { status: 404 })
    let path = normalize(decodeURIComponent(url.pathname.slice("/hex".length)))
    if (path.endsWith("/") || path.endsWith("\\")) path += "index.html"
    const file = Bun.file(join(root, path))
    if (!(await file.exists())) return new Response("Not found", { status: 404 })
    const headers = new Headers({ "Content-Type": file.type })
    if (url.pathname.startsWith("/hex/assets/")) headers.set("Cache-Control", "public, max-age=31536000, immutable")
    return new Response(file, { headers })
  },
})
console.log(`http://127.0.0.1:${port}/hex/`)
