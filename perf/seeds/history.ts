// A home seed holding a full History: 10,000 dictations, Hex's cap, one a
// minute back from when the seed was written, oldest first as the store
// keeps them. Written once; strobe copies it into each private home, so base
// and head read the same file. History is kept forever in strobe.json's
// settings, so no entry expires during a run.
import { existsSync, mkdirSync, writeFileSync } from "node:fs"
import { join } from "node:path"

export const HISTORY_ENTRIES = 10_000

const APPS = ["Slack", "Firefox", "WezTerm", "Zed", "Mail", "Notion"]
const WORDS =
  "the deploy failed because cache key changed so let's move standup to ten thirty tomorrow thanks for sending this over I'll review it today and follow up with comments please check build logs before merging".split(
    " ",
  )

// A sentence of 8 to 31 words picked by `seed`, so rows differ in length.
function sentence(seed: number): string {
  const count = 8 + (seed % 24)
  const words = Array.from({ length: count }, (_, i) => WORDS[(seed * 7 + i * 13) % WORDS.length])
  const text = words.join(" ")
  return `${text[0].toUpperCase()}${text.slice(1)}.`
}

export function historySeed(): string {
  const folder = join(import.meta.dir, ".generated", "history")
  const file = join(folder, "hex-data", "history.json")
  if (existsSync(file)) return folder
  mkdirSync(join(folder, "hex-data"), { recursive: true })
  const now = Date.now()
  const entries = Array.from({ length: HISTORY_ENTRIES }, (_, index) => {
    const id = index + 1
    const text = sentence(id)
    return {
      id,
      timestamp_ms: now - (HISTORY_ENTRIES - index) * 60_000,
      kind: "dictation",
      raw_text: text,
      final_text: text,
      application: APPS[id % APPS.length],
      audio_ms: 4_200,
      inference_ms: 180,
      total_ms: 640,
      model: "Parakeet Unified English",
    }
  })
  writeFileSync(file, JSON.stringify({ version: 1, next_id: HISTORY_ENTRIES + 1, entries }))
  return folder
}
