// PROTOTYPE: local static server for app-shell comparisons.
import { join, normalize } from "node:path"

const root = import.meta.dir
const port = 4173

Bun.serve({
  port,
  async fetch(request) {
    const url = new URL(request.url)
    const pathname = url.pathname === "/" ? "/index.html" : decodeURIComponent(url.pathname)
    const path = normalize(join(root, pathname))
    if (!path.startsWith(root)) return new Response("Not found", { status: 404 })
    const file = Bun.file(path)
    if (!(await file.exists())) return new Response("Not found", { status: 404 })
    return new Response(file)
  },
})

console.log(`Voice Control app-shell prototype: http://localhost:${port}`)
