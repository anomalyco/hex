#!/usr/bin/env bun
import { evaluateConfig, runHost } from "./host.js"

const entrypoint = process.argv[2]
if (entrypoint === undefined) {
  console.error("usage: hex-commands-host <config-entrypoint>")
  process.exit(2)
}

try {
  const config = await evaluateConfig(entrypoint)
  await runHost({
    config,
    input: process.stdin,
    write: (frame) => new Promise<void>((resolve, reject) => {
      process.stdout.write(`${JSON.stringify(frame)}\n`, (error) => error ? reject(error) : resolve())
    }),
  })
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error))
  process.exitCode = 1
}
