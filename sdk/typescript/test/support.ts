import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { fileURLToPath } from "node:url"
import type { EmbeddedEndpoint } from "../src/protocol.js"

export const helper = fileURLToPath(new URL("./fixtures/fake-helper.mjs", import.meta.url))

export const options = () => ({ command: [process.execPath, helper] as const })

export const fixtureEndpoint: EmbeddedEndpoint = {
  type: "ready",
  url: "http://127.0.0.1:1",
  token: "fixture",
  apiVersion: "2",
  pid: 1,
}

export const withTempDir = async <A>(prefix: string, use: (directory: string) => Promise<A>): Promise<A> => {
  const directory = await mkdtemp(join(tmpdir(), prefix))
  try {
    return await use(directory)
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
}

export const processIsAlive = (pid: number): boolean => {
  try {
    process.kill(pid, 0)
    return true
  } catch {
    return false
  }
}
