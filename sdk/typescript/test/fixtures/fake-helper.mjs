import http from "node:http"

if (process.env.HEX_FAKE_MODE === "exit-before-ready") process.exit(23)
if (process.env.HEX_FAKE_MODE === "bad-handshake") {
  process.stdout.write('{"type":"ready","url":"https://example.com"}\n')
  process.stdin.resume()
  process.stdin.on("end", () => process.exit(0))
} else {
  const token = "hex_0000000000000000000000000000000000000000000000000000000000000000"
  const models = [{
    id: "parakeet_v2",
    name: "Parakeet v2",
    installed: true,
    verified: true,
    managed: false,
    downloadBytes: 42,
    languages: ["en"],
    supportsLanguageDetection: false,
  }]
  const server = http.createServer((request, response) => {
    if (request.headers.authorization !== `Bearer ${token}`) {
      response.writeHead(401).end()
      return
    }
    const url = new URL(request.url ?? "/", "http://127.0.0.1")
    if (request.method === "GET" && url.pathname === "/health") {
      response.setHeader("content-type", "application/json")
      response.end(JSON.stringify({ version: "test", apiVersion: "1" }))
      return
    }
    if (request.method === "GET" && url.pathname === "/capabilities") {
      response.setHeader("content-type", "application/json")
      response.end(JSON.stringify({
        audioFormats: ["audio/wav"],
        partialTranscripts: false,
        serviceCapture: false,
      }))
      return
    }
    if (request.method === "GET" && url.pathname === "/models") {
      response.setHeader("content-type", "application/json")
      response.end(JSON.stringify(models))
      return
    }
    if (request.method === "POST" && url.pathname === "/models/parakeet_v2/prepare") {
      response.writeHead(200, { "content-type": "text/event-stream" })
      if (process.env.HEX_FAKE_PREPARE_BATCH === "1") {
        response.end(
          'data: {"type":"downloading","downloadedBytes":21,"totalBytes":42}\n\n'
          + 'data: {"type":"loading"}\n\n'
          + 'data: {"type":"ok"}\n\n',
        )
        return
      }
      response.write(': keep-alive\r\n\r\n')
      response.write('data:{"type":"downloading","downloadedBytes":21,')
      response.write('"totalBytes":42}\r\n\r\n')
      if (process.env.HEX_FAKE_PREPARE_HANG === "1") return
      if (process.env.HEX_FAKE_PREPARE_ERROR === "1") {
        response.end('data: {"type":"error","error":{"code":"load-failed","message":"no metal"}}\r\n\r\n')
        return
      }
      response.write('data: {"type":"verifying"}\r\n\r\n')
      response.write('data: {"type":"loading"}\r\n\r\n')
      response.write('data: {"type":"ok"}\r\n\r\n')
      return
    }
    if (request.method === "POST" && url.pathname === "/models/parakeet_v3/prepare") {
      response.writeHead(200, { "content-type": "text/plain" })
      response.end("not sse")
      return
    }
    if (request.method === "POST" && url.pathname === "/transcriptions") {
      if (process.env.HEX_FAKE_TRANSCRIBE_ERROR === "1") {
        response.writeHead(409, { "content-type": "application/json" })
        response.end(JSON.stringify({ code: "model-not-ready" }))
        return
      }
      const chunks = []
      request.on("data", (chunk) => chunks.push(chunk))
      request.on("end", () => {
        response.setHeader("content-type", "application/json")
        response.end(JSON.stringify({ transcript: "hello from hex", durationMs: 750 }))
      })
      return
    }
    response.writeHead(404, { "content-type": "application/json" })
    response.end(JSON.stringify({ code: "not_found" }))
  })

  server.listen(0, "127.0.0.1", () => {
    const address = server.address()
    if (typeof address === "object" && address !== null) {
      process.stdout.write(`${JSON.stringify({
        type: "ready",
        url: `http://127.0.0.1:${address.port}`,
        token,
        apiVersion: "1",
        pid: process.pid,
      })}\n`)
    }
  })

  process.stdin.resume()
  process.stdin.on("end", () => server.close(() => process.exit(0)))
}
