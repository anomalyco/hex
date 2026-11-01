# Hex transforms

Transforms are JavaScript or TypeScript functions that change dictated text
before Hex pastes it. This page is written for coding agents; it is the whole
API. Served at https://anoma.ly/hex/docs/transforms.

## Where they live

- The config is `~/.config/hex/hex.config.ts` (or `hex.config.js`; keep one).
  Run `hex commands init` to create it with `hex.d.ts` (types), `AGENTS.md`,
  and `tsconfig.json`. Typecheck with `npx -y -p typescript tsc` in that folder.
- Hex runs it in an embedded JavaScript engine (QuickJS, ES2023 modules) and
  reloads it about a third of a second after a file in the folder changes. A
  config that fails to load keeps the previous one; the error shows in Hex.
- TypeScript works by erasing types. Enums, namespaces, constructor parameter
  properties, and `import x = require()` are rejected.
- Imports: `@hex/commands`, `node:child_process`, `node:util`, and files in the
  folder by relative path (`./lib.ts`, `./data.json`). No npm packages. Imports
  load with the config; dynamic `import()` later is refused.
- A transform runs only where the person selects it: Hex's Output pane lists
  the steps, and `hex settings set dictation_processing.default_mode '<json>'`
  (or `dictation_processing.modes`) chooses them by id.

## Config shape

```ts
import { defineHexConfig } from "@hex/commands"

export default defineHexConfig({
  transformations: {
    "trim-whitespace": {
      name: "Trim whitespace",          // shown in Hex
      description: "Remove leading and trailing whitespace",
      timeout: 1000,                    // optional, milliseconds, 1 to 60000
      transform: (text, context) => text.trim(),
    },
  },
})
```

`transform(text, context)` returns a string or a promise of one. `context`
describes the foreground app, each field present only when known:

```ts
{ application?: string, browserHost?: string, browserUrl?: string, windowTitle?: string }
```

Selected transforms run in order after the mode's word replacements and
optional AI rewrite. Don't reuse the built-in ids `lowercase`,
`spongebob-case`, or `no-trailing-punctuation`.

## What a script has

- Plain JavaScript built-ins, `console.log` (to Hex's log), `setTimeout`,
  `clearTimeout`, `queueMicrotask`, `AbortController`, `AbortSignal`
  (`abort`, `timeout`, `any`), and `DOMException`.
- `fetch`, `Headers`, and `Response` (below).
- `node:child_process` (`exec`, `execFile`) and `node:util` (`promisify`).
- No filesystem, environment, `process`, `require`, `setInterval`, `URL`,
  `TextEncoder`, `FormData`, `Blob`, or streams. Read files with a program
  through `execFile` if you must.
- 64 MiB of memory for the config.

## `fetch`

The WHATWG shape, with a subset:

```ts
const response = await fetch(url, { method, headers, body, signal })
response.ok; response.status; response.statusText; response.url
response.headers.get(name); response.headers.has(name); [...response.headers.entries()]
await response.text(); await response.json(); await response.arrayBuffer()
```

- `url` is an absolute `http:` or `https:` URL. Redirects are followed.
- `body` is a string (`JSON.stringify(...)` for JSON); not allowed with GET or
  HEAD. A string body without a `content-type` header gets
  `text/plain;charset=UTF-8`.
- `headers` is an object, an array of pairs, or `Headers`.
- The response is read whole before `fetch` resolves (16 MiB at most); there
  is no streaming, `FormData`, or `Blob`.
- HTTP error statuses resolve normally (`response.ok` is false). Network
  errors reject with `TypeError: fetch failed: …`; an aborted `signal` rejects
  with its reason (`AbortError`, or `TimeoutError` for `AbortSignal.timeout`).
- TLS uses the operating system's certificates.

Example, a local model through Ollama:

```ts
import { defineHexConfig } from "@hex/commands"

export default defineHexConfig({
  transformations: {
    "ollama-tidy": {
      name: "Tidy with Ollama",
      timeout: 20000,
      transform: async (text) => {
        const response = await fetch("http://localhost:11434/api/generate", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            model: "llama3.2",
            prompt: `Fix punctuation and capitalization. Reply with the text only.\n\n${text}`,
            stream: false,
          }),
        })
        if (!response.ok) throw new Error(`Ollama answered ${response.status}`)
        const data: { response: string } = await response.json()
        return data.response.trim()
      },
    },
  },
})
```

## `node:child_process`

Node's `exec` and `execFile`, with callbacks and `util.promisify`:

```ts
import { exec, execFile } from "node:child_process"
import { promisify } from "node:util"

const run = promisify(execFile)                         // resolves { stdout, stderr }
const { stdout } = await run("pandoc", ["-t", "plain"], { input: text })
execFile(file, args, options, (error, stdout, stderr) => {})
exec(command, options, (error, stdout, stderr) => {})
```

- `execFile(file, args, options, callback)` runs the program with an argument
  array and no shell. **Use it for anything involving dictated text**; never
  build a shell command out of the text.
- `exec(command, options, callback)` runs `/bin/sh -c command` on macOS and
  Linux, `cmd.exe /d /s /c "command"` on Windows, as Node does.
- Options: `cwd` (default: the config folder), `env` (added to a minimal
  environment of `PATH`, `HOME`, user, locale, and temp variables; Hex's own
  environment is not passed on; on macOS `PATH` also has `/opt/homebrew/bin`
  and `/usr/local/bin`), `timeout` (ms, then SIGTERM), `maxBuffer` (bytes per
  stream, default 1 MiB, at most 16 MiB), `signal`, and Hex's addition
  `input`: a string written to stdin, which is then closed. Without `input`,
  stdin is empty.
- `stdout` and `stderr` are strings (UTF-8). There is no `encoding`, `shell`,
  `uid`, `killSignal`, `spawn`, `fork`, or sync variants.
- Errors follow Node: a non-zero exit gives `Error: Command failed: <cmd>`
  with `code` (exit code), `killed`, `signal`, and `cmd`; a missing program
  gives `code: "ENOENT"`; a `timeout` gives `killed: true, signal: "SIGTERM"`;
  too much output gives a `RangeError` with
  `code: "ERR_CHILD_PROCESS_STDIO_MAXBUFFER"`; an aborted `signal` gives an
  `AbortError`. Promisified errors also carry `stdout` and `stderr`.
- The returned child has only `kill()` (SIGTERM to the program and its
  children) and `killed`; there is no `pid` or stream access.

Example, piping the text through a script:

```ts
import { defineHexConfig } from "@hex/commands"
import { execFile } from "node:child_process"
import { promisify } from "node:util"

const run = promisify(execFile)

export default defineHexConfig({
  transformations: {
    "house-style": {
      name: "House style",
      transform: async (text) => {
        const { stdout } = await run("python3", ["./house_style.py"], { input: text })
        return stdout.trimEnd()
      },
    },
  },
})
```

## Trust prompt

The first time a config uses `fetch` or `node:child_process`, Hex asks the
person in its Output pane, listing the hosts and programs it can see written
literally in the source (`fetch("https://…")`, `execFile("pandoc", …)`),
otherwise just the capability. The approval is pinned to a hash of every file
the config loaded: any edit asks again. Until it is allowed, `fetch` rejects
and `exec`/`execFile` call back with an error saying so, and the transform
falls back. A config can't approve itself; the approval is stored in Hex's
own data folder.

## Timeouts, limits, and fallback

- Each transform has `timeout` milliseconds: 1000 by default, 30000 when the
  config uses `fetch` or `node:child_process`, 60000 at most. That limit covers
  everything the transform waits for.
- When a transform finishes or runs out of time, Hex abandons its requests and
  ends every program it started, with the program's children (a process group
  on macOS and Linux, a job object on Windows). Quitting Hex does the same.
  Fire-and-forget work does not outlive the transform.
- At most 8 requests and 4 programs run at once; more wait their turn.
- If a transform throws, returns something other than a string, or runs out
  of time, Hex pastes the text it had before the transforms and logs why. It
  never blocks dictation; the microphone keeps working while it waits.
- Input and output text are each limited to 48 KiB.
- Hex logs the host or program, status or exit code, duration, and byte
  counts, never the text, request or response bodies, arguments, or
  environment values.
