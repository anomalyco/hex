// One Worker serves Hex under /hex on its host: the site, and the release
// files bucket at /hex/files (feeds, appcast, installers, artifacts). Other
// hosts and plain HTTP redirect to the canonical host. `cf dev` with
// ANY_HOST set turns the redirect off for local testing.
import { BASE_PATH, FILES_PATH, stripBase, withBase } from "./src/base"

type Env = {
  ASSETS: { fetch(request: Request): Promise<Response> }
  FILES: R2Bucket
  CANONICAL_HOST: string
  ANY_HOST?: string
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url)
    if (!env.ANY_HOST && (url.hostname !== env.CANONICAL_HOST || url.protocol !== "https:")) {
      url.protocol = "https:"
      url.hostname = env.CANONICAL_HOST
      url.port = ""
      return Response.redirect(url.toString(), 301)
    }
    if (url.pathname === BASE_PATH) {
      url.pathname = `${BASE_PATH}/`
      return Response.redirect(url.toString(), 301)
    }
    if (url.pathname.startsWith(`${FILES_PATH}/`)) return file(request, env, decodeURIComponent(url.pathname.slice(FILES_PATH.length + 1)))

    // The route also matches paths like /hexagon, which are not ours.
    const path = stripBase(url.pathname)
    if (path === undefined) return new Response("Not found", { status: 404 })
    const assetUrl = new URL(url)
    assetUrl.pathname = path
    const assetRequest = new Request(assetUrl, request)
    const response = await env.ASSETS.fetch(assetRequest)
    const headers = new Headers(response.headers)
    // Asset redirects (trailing slashes, /index.html) point at root-relative paths.
    const location = headers.get("Location")
    if (location?.startsWith("/")) headers.set("Location", withBase(location))
    if (path.startsWith("/assets/")) {
      headers.set("Cache-Control", "public, max-age=31536000, immutable")
    }
    secure(headers)
    if (path.endsWith(".mp4")) return ranged(request, request.method === "HEAD" ? await env.ASSETS.fetch(new Request(assetUrl, { headers: request.headers })) : response, headers)
    return new Response(response.body, {
      status: response.status,
      statusText: response.statusText,
      headers,
    })
  },
}

function secure(headers: Headers) {
  headers.set("Strict-Transport-Security", "max-age=31536000")
  headers.set("X-Content-Type-Options", "nosniff")
  headers.set("Referrer-Policy", "strict-origin-when-cross-origin")
}

// Serves one bucket object with its stored Content-Type, Cache-Control, and
// Content-Disposition, honouring Range and conditional requests.
async function file(request: Request, env: Env, key: string): Promise<Response> {
  if (request.method !== "GET" && request.method !== "HEAD") {
    return new Response("Method not allowed", { status: 405, headers: { Allow: "GET, HEAD" } })
  }
  if (!key || key.endsWith("/")) return new Response("Not found", { status: 404 })
  const object =
    request.method === "HEAD"
      ? await env.FILES.head(key)
      : await env.FILES.get(key, { range: request.headers, onlyIf: request.headers })
  if (!object) return new Response("Not found", { status: 404 })
  const headers = new Headers()
  object.writeHttpMetadata(headers)
  headers.set("ETag", object.httpEtag)
  headers.set("Accept-Ranges", "bytes")
  secure(headers)
  if (request.method === "HEAD") {
    headers.set("Content-Length", String(object.size))
    return new Response(null, { headers })
  }
  if (!("body" in object)) return new Response(null, { status: 304, headers })
  const range = object.range as { offset?: number; length?: number; suffix?: number } | undefined
  if (range && request.headers.has("Range")) {
    const offset = range.suffix !== undefined ? object.size - range.suffix : (range.offset ?? 0)
    const length = range.length ?? object.size - offset
    headers.set("Content-Range", `bytes ${offset}-${offset + length - 1}/${object.size}`)
    headers.set("Content-Length", String(length))
    return new Response(object.body, { status: 206, headers })
  }
  headers.set("Content-Length", String(object.size))
  return new Response(object.body, { headers })
}

// Static assets ignore Range, but Safari and AVFoundation need byte ranges to play video.
// A HEAD is answered from the full GET so its headers match.
async function ranged(request: Request, response: Response, headers: Headers): Promise<Response> {
  headers.set("Accept-Ranges", "bytes")
  const match = /^bytes=(\d*)-(\d*)$/.exec(request.headers.get("Range") ?? "")
  if (response.status !== 200 || !match || (!match[1] && !match[2])) {
    return new Response(response.body, { status: response.status, statusText: response.statusText, headers })
  }
  const body = await response.arrayBuffer()
  const size = body.byteLength
  const start = match[1] ? Number(match[1]) : Math.max(0, size - Number(match[2]))
  const end = match[1] && match[2] ? Math.min(Number(match[2]), size - 1) : size - 1
  if (start > end) {
    headers.set("Content-Range", `bytes */${size}`)
    return new Response(null, { status: 416, headers })
  }
  headers.set("Content-Range", `bytes ${start}-${end}/${size}`)
  headers.set("Content-Length", String(end - start + 1))
  return new Response(request.method === "HEAD" ? null : body.slice(start, end + 1), { status: 206, headers })
}
