// A home seed holding the default English model, hard-linked from this
// machine's own Hex install, or from the folder HEX_SEED_MODELS names on a
// machine without one (read only; strobe copies the seed into each private
// home). Returns null when no installed model is found.
import { copyFileSync, existsSync, linkSync, mkdirSync } from "node:fs"
import { homedir } from "node:os"
import { join } from "node:path"

const MODELS = ["parakeet-unified-en-0.6b-Q8_0.gguf", "silero-vad-v6.2-F32.gguf"]

function installedModels(): string | null {
  if (process.env.HEX_SEED_MODELS && existsSync(join(process.env.HEX_SEED_MODELS, MODELS[0]))) return process.env.HEX_SEED_MODELS
  const roots = [
    process.env.APPDATA && join(process.env.APPDATA, "hex"),
    join(homedir(), "Library", "Application Support", "hex"),
    join(process.env.XDG_DATA_HOME ?? join(homedir(), ".local", "share"), "hex"),
  ]
  for (const root of roots) {
    if (root && existsSync(join(root, "models", MODELS[0]))) return join(root, "models")
  }
  return null
}

/** Links the installed models into `folder`'s `hex-data/models`; false when there are none. */
export function seedModels(folder: string): boolean {
  const models = join(folder, "hex-data", "models")
  if (existsSync(join(models, MODELS[0]))) return true
  const source = installedModels()
  if (!source) return false
  mkdirSync(models, { recursive: true })
  for (const name of MODELS.flatMap((model) => [model, `${model}.verified.json`])) {
    if (!existsSync(join(source, name))) continue
    // Another volume can't hold a link; a copy keeps the modified time the
    // model's receipt checks.
    try {
      linkSync(join(source, name), join(models, name))
    } catch {
      copyFileSync(join(source, name), join(models, name))
    }
  }
  return true
}

export function listeningSeed(): string | null {
  const folder = join(import.meta.dir, ".generated", "listening")
  return seedModels(folder) ? folder : null
}
