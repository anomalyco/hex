// One Worker serves Hex under /hex on its host: the site, the release files
// bucket at /hex/files (feeds, appcast, installers, artifacts), and
// /hex/download/dev, the latest dev channel Mac download, and
// /hex/download/<target>, the release every download button names. Other hosts and
// plain HTTP redirect to the canonical host. `cf dev` with ANY_HOST set turns
// the redirect off for local testing.
import { BASE_PATH, DEV_DOWNLOAD_PATH, DOWNLOAD_PATH, FILES_PATH, RELEASE_SERVICE, stripBase, withBase } from "./src/base"

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
    if (url.pathname === DEV_DOWNLOAD_PATH) return devDownload(request)
    if (url.pathname.startsWith(`${DOWNLOAD_PATH}/`)) return download(request, url.pathname.slice(DOWNLOAD_PATH.length + 1))

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

const DOWNLOAD_TARGETS = new Set([
  "macos-arm64-dmg",
  "windows-x64-setup",
  "windows-arm64-setup",
  "linux-x64",
  "linux-arm64",
  "linux-x64-deb",
  "linux-arm64-deb",
  "linux-x64-rpm",
  "linux-arm64-rpm",
  // The bare updater files.
  "macos-arm64",
  "windows-x64",
  "windows-arm64",
])

// Redirects /hex/download/<target> to the release service's alias for
// <target> on the stable channel (the dev channel while there is no stable
// release, decided per request), and /hex/download/dev/<target> to the dev
// channel's.
async function download(request: Request, path: string): Promise<Response> {
  if (request.method !== "GET" && request.method !== "HEAD") {
    return new Response("Method not allowed", { status: 405, headers: { Allow: "GET, HEAD" } })
  }
  const dev = path.startsWith("dev/")
  const target = dev ? path.slice("dev/".length) : path
  if (!DOWNLOAD_TARGETS.has(target)) return new Response("Not found", { status: 404 })
  let channel = "dev"
  if (!dev) {
    try {
      const stable = await fetch(`${RELEASE_SERVICE}/hex/desktop/stable.json`, { headers: { "User-Agent": "hex-site" }, cache: "no-store" })
      if (stable.ok) channel = "stable"
    } catch {
      // The service is unreachable: the dev alias fails the same way, visibly.
    }
  }
  const headers = new Headers({ Location: `${RELEASE_SERVICE}/hex/desktop/${channel}/${target}`, "Cache-Control": "no-store" })
  secure(headers)
  return new Response(null, { status: 302, headers })
}

// Redirects to the DMG of the release service's latest dev channel build, so
// one link always names the newest Mac dev build on both site environments. The service
// is another Worker on this zone; the compatibility flag
// global_fetch_strictly_public lets this fetch reach it.
async function devDownload(request: Request): Promise<Response> {
  if (request.method !== "GET" && request.method !== "HEAD") {
    return new Response("Method not allowed", { status: 405, headers: { Allow: "GET, HEAD" } })
  }
  const manifestUrl = `${RELEASE_SERVICE}/hex/desktop/dev.json`
  const text = (body: string, status: number) => {
    const headers = new Headers({ "Content-Type": "text/plain; charset=utf-8", "Cache-Control": "no-store" })
    secure(headers)
    return new Response(`${body}\n`, { status, headers })
  }
  let response: Response
  try {
    response = await fetch(manifestUrl, { headers: { "User-Agent": "hex-site" }, cache: "no-store" })
  } catch {
    return text("Hex's release service did not answer. Try again in a minute.", 502)
  }
  if (response.status === 404) return text("No dev build of Hex has been published yet.", 404)
  if (!response.ok) return text(`Hex's release service answered ${response.status}. Try again in a minute.`, 502)
  type Manifest = { app?: string; artifact?: string; channel?: string; release?: { version?: string; targets?: Record<string, { url?: string }> } }
  let manifest: Manifest
  try {
    manifest = (await response.json()) as Manifest
  } catch {
    return text("Hex's dev release manifest is not valid JSON.", 502)
  }
  if (manifest.app !== "hex" || manifest.artifact !== "desktop" || manifest.channel !== "dev") {
    return text("Hex's dev release manifest names another release.", 502)
  }
  const version = manifest.release?.version ?? "unknown"
  const dmg = manifest.release?.targets?.["macos-arm64-dmg"]?.url
  if (!dmg) return text(`The latest dev build of Hex (${version}) has no Mac download.`, 404)
  // Target URLs resolve against the manifest's URL (anomalyco/release docs/protocol.md).
  // Only redirect to the service's own files, never to another origin.
  let location: URL
  try {
    location = new URL(dmg, manifestUrl)
  } catch {
    return text("Hex's dev release manifest has an invalid Mac download URL.", 502)
  }
  if (location.origin !== new URL(manifestUrl).origin || !location.pathname.startsWith("/release/hex/files/")) {
    return text("Hex's dev release manifest points the Mac download outside the release service.", 502)
  }
  const headers = new Headers({ Location: location.toString(), "Cache-Control": "no-store" })
  secure(headers)
  return new Response(null, { status: 302, headers })
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
