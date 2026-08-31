import { accessSync, constants } from "node:fs"
import { createRequire } from "node:module"
import { dirname, resolve } from "node:path"
import { HexError } from "./errors.js"
import type { CreateOptions } from "./types.js"

const require = createRequire(import.meta.url)

const platformPackage = (): string => {
  if (process.platform === "darwin" && process.arch === "arm64") {
    return "@hex-ai/service-darwin-arm64"
  }
  throw new HexError(
    "startup-failed",
    `HEX does not include a helper for ${process.platform}-${process.arch}`,
  )
}

const packagedExecutable = (): string => {
  const packageName = platformPackage()
  let manifest: string
  try {
    manifest = require.resolve(`${packageName}/package.json`)
  } catch (cause) {
    throw new HexError("startup-failed", `The included HEX helper package ${packageName} is unavailable`, {
      cause,
    })
  }
  const executable = resolve(dirname(manifest), "bin", "hex-service")
  try {
    accessSync(executable, constants.X_OK)
  } catch (cause) {
    throw new HexError("startup-failed", `The included HEX helper is not executable: ${executable}`, {
      cause,
    })
  }
  return executable
}

export const resolveCommand = (
  options: Pick<CreateOptions, "command">,
): readonly [executable: string, ...arguments: readonly string[]] =>
  options.command ?? [packagedExecutable(), "service", "--embedded"]
