// The URLs and headers people and old clients depend on: /hex/files serves
// the built installers and appcast with the headers the bucket objects had,
// the frozen feeds from the bucket, and the dev notes from the Release
// workflow's branch.
import { afterEach, expect, test } from "bun:test"
import worker from "./worker"

const NOTES = JSON.stringify({
  schema: 1,
  builds: [
    {
      version: "0.0.0-dev-2",
      commit: "a".repeat(40),
      published_at: "2026-11-09T00:00:00Z",
      items: [{ title: "A new thing", url: "https://github.com/anomalyco/hex/pull/2" }],
    },
  ],
})

const files = new Map([
  ["/install-linux.sh", "#!/bin/sh\necho install\n"],
  ["/install-windows.ps1", "Write-Output install\n"],
  ["/appcast.xml", "<rss></rss>\n"],
])

function object(text: string, contentType: string, cacheControl: string) {
  const bytes = new TextEncoder().encode(text)
  return {
    body: new Response(bytes).body,
    size: bytes.byteLength,
    httpEtag: '"frozen"',
    range: undefined as { offset?: number; length?: number; suffix?: number } | undefined,
    writeHttpMetadata(headers: Headers) {
      headers.set("Content-Type", contentType)
      headers.set("Cache-Control", cacheControl)
    },
    json: () => Promise.resolve(JSON.parse(text) as unknown),
  }
}

function environment(objects: Record<string, ReturnType<typeof object>> = {}) {
  return {
    ASSETS: {
      fetch: (request: Request) => {
        const path = new URL(request.url).pathname
        const text = files.get(path)
        return Promise.resolve(text === undefined ? new Response("Not found", { status: 404 }) : new Response(text))
      },
    },
    FILES: {
      get: (key: string) => Promise.resolve(objects[key] ?? null),
      head: (key: string) => Promise.resolve(objects[key] ?? null),
    },
    CANONICAL_HOST: "anoma.ly",
  } as unknown as Parameters<typeof worker.fetch>[1]
}

const originalFetch = globalThis.fetch

afterEach(() => {
  globalThis.fetch = originalFetch
})

function notesFetch(response: () => Response) {
  globalThis.fetch = (() => Promise.resolve(response())) as unknown as typeof fetch
}

async function get(path: string, env = environment(), method = "GET") {
  return worker.fetch(new Request(`https://anoma.ly${path}`, { method }), env)
}

test("serves the Linux installer with the bucket's type and cache", async () => {
  const response = await get("/hex/files/install-linux.sh")
  expect(response.status).toBe(200)
  expect(response.headers.get("Content-Type")).toBe("text/x-shellscript")
  expect(response.headers.get("Cache-Control")).toBe("public, max-age=300")
  expect(response.headers.get("X-Content-Type-Options")).toBe("nosniff")
  expect(await response.text()).toBe(files.get("/install-linux.sh") as string)
})

test("answers HEAD for an installer without a body", async () => {
  const response = await get("/hex/files/install-linux.sh", environment(), "HEAD")
  expect(response.status).toBe(200)
  expect(response.headers.get("Content-Type")).toBe("text/x-shellscript")
  expect(response.body).toBeNull()
})

test("serves the Windows installer and the legacy appcast with their old headers", async () => {
  const windows = await get("/hex/files/install-windows.ps1")
  expect(windows.headers.get("Content-Type")).toBe("text/plain; charset=utf-8")
  expect(windows.headers.get("Cache-Control")).toBe("public, max-age=300")

  const appcast = await get("/hex/files/appcast.xml")
  expect(appcast.headers.get("Content-Type")).toBe("application/xml")
  expect(appcast.headers.get("Cache-Control")).toBe("no-cache")
  expect(await appcast.text()).toBe(files.get("/appcast.xml") as string)
})

test("serves a frozen 2.3.1 feed from the bucket with its stored headers", async () => {
  const feed = object('{"version":"2.3.1"}', "application/json", "no-cache")
  const response = await get("/hex/files/linux-update.json", environment({ "linux-update.json": feed }))
  expect(response.status).toBe(200)
  expect(response.headers.get("Content-Type")).toBe("application/json")
  expect(response.headers.get("Cache-Control")).toBe("no-cache")
  expect(await response.text()).toBe('{"version":"2.3.1"}')
})

test("serves dev-notes.json from the dev-notes branch", async () => {
  notesFetch(() => new Response(NOTES, { headers: { "Content-Type": "text/plain" } }))
  const response = await get("/hex/files/dev-notes.json")
  expect(response.status).toBe(200)
  expect(response.headers.get("Content-Type")).toBe("application/json; charset=utf-8")
  expect(response.headers.get("Cache-Control")).toBe("public, max-age=60")
  expect(await response.json()).toMatchObject({ builds: [{ version: "0.0.0-dev-2" }] })
})

test("falls back to the bucket's dev notes before the branch exists", async () => {
  notesFetch(() => new Response("Not found", { status: 404 }))
  const response = await get("/hex/files/dev-notes.json", environment({ "dev-notes.json": object(NOTES, "application/json; charset=utf-8", "public, max-age=60") }))
  expect(response.status).toBe(200)
  expect(await response.json()).toMatchObject({ schema: 1 })
})

test("renders /hex/dev from the dev-notes branch", async () => {
  notesFetch(() => new Response(NOTES))
  const response = await get("/hex/dev")
  expect(response.status).toBe(200)
  expect(response.headers.get("Content-Type")).toBe("text/html; charset=utf-8")
  const body = await response.text()
  expect(body).toContain("A new thing")
  expect(body).toContain('href="https://github.com/anomalyco/hex/pull/2"')
})

test("does not answer writes", async () => {
  const response = await get("/hex/files/install-linux.sh", environment(), "POST")
  expect(response.status).toBe(405)
  expect(response.headers.get("Allow")).toBe("GET, HEAD")
})
