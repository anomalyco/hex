// Writes a home seed with `count` retained dictations (hex-data/history.json),
// the same entries every time, so base and head read the same store.
import { existsSync, mkdirSync, writeFileSync } from "node:fs"
import { join } from "node:path"

const words = (
  "the deploy failed because cache key changed let's move standup to ten thirty tomorrow thanks for sending " +
  "this over i'll review it today and follow up with comments please ship the build after lunch can you check " +
  "why the test is flaky on windows we should rename the folder before the release notes go out"
).split(" ")
const apps = ["Slack", "WezTerm", "Firefox", "Code", "Outlook", null]

/** The seed folder for `count` entries, beside this file; built once. */
export function historySeed(count: number): string {
  const folder = join(import.meta.dir, ".generated", `history-${count}`)
  const file = join(folder, "hex-data", "history.json")
  if (existsSync(file)) return folder
  let state = 1
  const random = () => (state = (state * 1103515245 + 12345) % 2 ** 31) / 2 ** 31
  const start = Date.UTC(2026, 0, 1)
  const entries = Array.from({ length: count }, (_, i) => {
    const length = 8 + Math.floor(random() * 40)
    const text = Array.from({ length }, () => words[Math.floor(random() * words.length)]).join(" ")
    const sentence = text[0].toUpperCase() + text.slice(1) + "."
    const application = apps[Math.floor(random() * apps.length)]
    return {
      id: i + 1,
      timestamp_ms: start + i * 90_000,
      kind: "dictation",
      raw_text: text,
      final_text: sentence,
      ...(application ? { application } : {}),
      audio_ms: 3_000 + Math.floor(random() * 9_000),
      inference_ms: 120 + Math.floor(random() * 200),
      total_ms: 400 + Math.floor(random() * 800),
    }
  })
  mkdirSync(join(folder, "hex-data"), { recursive: true })
  writeFileSync(file, JSON.stringify({ version: 1, next_id: count + 1, entries }))
  return folder
}
