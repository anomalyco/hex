// hex.kitlangton.dev is the one Hex site. The old hex.kitlangton.com, plain
// HTTP, and workers.dev all 301 to the same path and query there. Hashed build
// assets get a long cache. `wrangler dev --var ANY_HOST:1` turns the redirect
// off for local testing.
type Env = { ASSETS: { fetch(request: Request): Promise<Response> }; ANY_HOST?: string }

const HOST = "hex.kitlangton.dev"

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url)
    if (!env.ANY_HOST && (url.hostname !== HOST || url.protocol !== "https:")) {
      url.protocol = "https:"
      url.hostname = HOST
      url.port = ""
      return Response.redirect(url.toString(), 301)
    }

    const response = await env.ASSETS.fetch(request)
    const headers = new Headers(response.headers)
    if (url.pathname.startsWith("/assets/")) {
      headers.set("Cache-Control", "public, max-age=31536000, immutable")
    }
    headers.set("Strict-Transport-Security", "max-age=31536000")
    headers.set("X-Content-Type-Options", "nosniff")
    headers.set("Referrer-Policy", "strict-origin-when-cross-origin")
    return new Response(response.body, {
      status: response.status,
      statusText: response.statusText,
      headers,
    })
  },
}
