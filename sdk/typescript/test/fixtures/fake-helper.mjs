import http from "node:http"
import fs from "node:fs"
import path from "node:path"

if (process.env.HEX_FAKE_PID_PATH) fs.writeFileSync(process.env.HEX_FAKE_PID_PATH, String(process.pid))
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
  const dictations = new Map()
  const ownerToken = "hex_capture_0000000000000000000000000000000000000000000000000000000000000000"
  let finishAttempts = 0
  const server = http.createServer((request, response) => {
    if (request.headers.authorization !== `Bearer ${token}`) {
      response.writeHead(401).end()
      return
    }
    const url = new URL(request.url ?? "/", "http://127.0.0.1")
    if (request.method === "GET" && url.pathname === "/health") {
      response.setHeader("content-type", "application/json")
      response.end(JSON.stringify({ version: "test", apiVersion: "2" }))
      return
    }
    if (request.method === "GET" && url.pathname === "/capabilities") {
      response.setHeader("content-type", "application/json")
      response.end(JSON.stringify({
        audioFormats: ["audio/wav"],
        partialTranscripts: false,
        serviceCapture: process.env.HEX_FAKE_SERVICE_CAPTURE === "1",
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
        if (process.env.HEX_FAKE_TRANSCRIBE_HANG === "1") {
          response.writeHead(200)
          response.flushHeaders()
          return
        }
        response.end(JSON.stringify({ transcript: "hello from hex", durationMs: 750 }))
      })
      return
    }
    if (process.env.HEX_FAKE_SERVICE_CAPTURE === "1" && request.method === "POST" && url.pathname === "/dictations") {
      const id = 1
      dictations.set(id, {})
      response.writeHead(201, { "content-type": "application/json" })
      response.end(JSON.stringify({ id, ownerToken, sampleRate: 48_000 }))
      return
    }
    if (
      process.env.HEX_FAKE_SERVICE_CAPTURE === "1"
      && url.pathname.startsWith("/dictations/1/")
      && request.headers["x-hex-dictation-token"] !== ownerToken
    ) {
      response.writeHead(404, { "content-type": "application/json" })
      response.end(JSON.stringify({ code: "dictation-not-found" }))
      return
    }
    if (process.env.HEX_FAKE_SERVICE_CAPTURE === "1" && request.method === "GET" && url.pathname === "/dictations/1/levels") {
      response.writeHead(200, { "content-type": "text/event-stream" })
      dictations.get(1).levels = response
      if (process.env.HEX_FAKE_LEVEL_BURST === "1") {
        for (let index = 0; index < 40; index++) {
          response.write(`data: {"rmsDb":${index},"peakDb":${index}}\n\n`)
        }
      } else {
        response.write('data: {"rmsDb":-24.5,"peakDb":-8}\n\n')
      }
      return
    }
    if (process.env.HEX_FAKE_SERVICE_CAPTURE === "1" && request.method === "GET" && url.pathname === "/dictations/1/audio") {
      response.writeHead(200, { "content-type": "application/octet-stream" })
      dictations.get(1).audio = response
      const samples = Buffer.alloc(8)
      samples.writeFloatLE(0.25, 0)
      samples.writeFloatLE(-0.5, 4)
      response.write(samples)
      return
    }
    if (process.env.HEX_FAKE_SERVICE_CAPTURE === "1" && request.method === "POST" && url.pathname === "/dictations/1/finish") {
      finishAttempts += 1
      if (process.env.HEX_FAKE_FINISH_RETRY === "1" && finishAttempts === 1) {
        response.writeHead(503, { "content-type": "application/json" })
        response.end(JSON.stringify({ code: "service-capture-busy" }))
        return
      }
      dictations.get(1)?.levels?.end()
      dictations.get(1)?.audio?.end()
      response.writeHead(200, { "content-type": "application/json" })
      response.end(JSON.stringify({ transcript: "running app text", durationMs: 1234 }))
      return
    }
    if (process.env.HEX_FAKE_SERVICE_CAPTURE === "1" && request.method === "POST" && url.pathname === "/dictations/1/heartbeat") {
      response.writeHead(204).end()
      return
    }
    if (process.env.HEX_FAKE_SERVICE_CAPTURE === "1" && request.method === "POST" && url.pathname === "/dictations/1/cancel") {
      dictations.get(1)?.levels?.end()
      dictations.get(1)?.audio?.end()
      response.writeHead(204).end()
      return
    }
    response.writeHead(404, { "content-type": "application/json" })
    response.end(JSON.stringify({ code: "not_found" }))
  })

  server.listen(0, "127.0.0.1", () => {
    const address = server.address()
    if (typeof address === "object" && address !== null) {
      if (process.env.HEX_FAKE_DISCOVERY_PATH) {
        fs.writeFileSync(process.env.HEX_FAKE_DISCOVERY_PATH, JSON.stringify({
          port: address.port,
          token,
          apiVersion: "2",
          pid: process.pid,
        }))
      }
      const announce = () => process.stdout.write(`${JSON.stringify({
        type: "ready",
        url: `http://127.0.0.1:${address.port}`,
        token,
        apiVersion: "2",
        pid: process.pid,
      })}\n`)
      const gate = process.env.HEX_FAKE_HANDSHAKE_GATE
      if (gate) {
        const check = () => {
          if (!fs.existsSync(gate)) return
          watcher.close()
          announce()
        }
        const watcher = fs.watch(path.dirname(gate), check)
        check()
      } else {
        announce()
      }
    }
  })

  process.stdin.resume()
  process.stdin.on("end", () => server.close(() => process.exit(0)))
}
