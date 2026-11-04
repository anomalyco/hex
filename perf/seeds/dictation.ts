// A home seed for dictation benches: the default English model (as the
// listening seed) and `speech.wav`, one spoken sentence and then silence,
// which the copy hears instead of a microphone (HEX_AUDIO_FIXTURE). The
// speech comes from this machine's own text-to-speech voice (Windows and
// macOS at 48 kHz, a microphone's usual rate; Linux through espeak-ng), so
// no one is recorded. Returns null without a model or a voice.
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import { join } from "node:path"
import { seedModels } from "./listening"

const TEXT = "Let's move the design review to Thursday afternoon, and send the updated notes to the whole team before then."

/** Silence after the sentence, so a dictation that stops there ends on a pause. */
export const SILENCE_MS = 2_000

function speak(path: string): boolean {
  if (process.platform === "win32") {
    const script = [
      "Add-Type -AssemblyName System.Speech",
      "$s = New-Object System.Speech.Synthesis.SpeechSynthesizer",
      "$f = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(48000, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono)",
      `$s.SetOutputToWaveFile('${path}', $f)`,
      `$s.Speak('${TEXT.replaceAll("'", "''")}')`,
      "$s.Dispose()",
    ].join("; ")
    Bun.spawnSync(["powershell", "-NoProfile", "-Command", script])
  } else if (process.platform === "darwin") {
    Bun.spawnSync(["say", "-o", path, "--file-format=WAVE", "--data-format=LEI16@48000", TEXT])
  } else if (Bun.which("espeak-ng")) {
    // 22.05 kHz: espeak-ng's own rate; Hex resamples any rate.
    Bun.spawnSync(["espeak-ng", "-w", path, TEXT])
  }
  return existsSync(path)
}

/** Where a WAV's `data` chunk starts and its size in bytes. */
function data(wav: Buffer): { at: number; size: number } {
  let at = 12
  while (at + 8 <= wav.length && wav.toString("ascii", at, at + 4) !== "data") at += 8 + wav.readUInt32LE(at + 4)
  return { at, size: wav.readUInt32LE(at + 4) }
}

/** The 16-bit mono WAV at `path` with `ms` of silence appended. */
function padded(path: string, ms: number): Buffer {
  const wav = readFileSync(path)
  const { at, size } = data(wav)
  const rate = wav.readUInt32LE(24)
  const silence = Buffer.alloc(Math.round((rate * ms) / 1000) * 2)
  const out = Buffer.concat([wav.subarray(0, at + 8 + size), silence])
  out.writeUInt32LE(size + silence.length, at + 4)
  out.writeUInt32LE(out.length - 8, 4)
  return out
}

/** How long the sentence in `speech.wav` speaks, from its header. */
export function speechMs(folder: string): number {
  const wav = readFileSync(join(folder, "speech.wav"))
  return (data(wav).size / 2 / wav.readUInt32LE(24)) * 1000 - SILENCE_MS
}

export function dictationSeed(): string | null {
  const folder = join(import.meta.dir, ".generated", "dictation")
  if (!seedModels(folder)) return null
  const speech = join(folder, "speech.wav")
  if (existsSync(speech)) return folder
  const spoken = join(folder, "spoken.wav")
  if (!speak(spoken)) return null
  writeFileSync(speech, padded(spoken, SILENCE_MS))
  rmSync(spoken)
  return folder
}
