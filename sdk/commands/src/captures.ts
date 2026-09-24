import type { CaptureDescriptor, UnionCapture } from "./model.js"

const MAX_CHOICE_VALUES = 64
const MAX_CHOICE_ALIASES = 16
const MAX_CHOICE_VALUE_BYTES = 128
const MAX_CHOICE_WORD_BYTES = 64
const MAX_UNION_MEMBERS = 16
const MAX_UNION_DEPTH = 4
const MAX_UNION_SERIALIZED_BYTES = 16 * 1024

const utf8Length = (value: string): number => new TextEncoder().encode(value).byteLength
const boundedString = (value: unknown, label: string, maxBytes: number): string => {
  if (typeof value !== "string" || value.length === 0 || utf8Length(value) > maxBytes) {
    throw new Error(`${label} must be a non-empty string no longer than ${maxBytes} UTF-8 bytes`)
  }
  return value
}
const record = (value: unknown): Record<string, unknown> | undefined =>
  typeof value === "object" && value !== null && !Array.isArray(value)
    ? Object.fromEntries(Object.entries(value))
    : undefined

const spokenDigits: Readonly<Record<string, string>> = {
  zero: "0", one: "1", two: "2", three: "3", four: "4",
  five: "5", six: "6", seven: "7", eight: "8", nine: "9",
}
const letterAliases = new Set([
  "a", "ay", "alpha", "b", "bee", "bravo", "c", "see", "charlie", "d", "dee", "delta",
  "e", "echo", "f", "ef", "foxtrot", "g", "gee", "golf", "h", "aitch", "hotel", "i", "eye",
  "india", "j", "jay", "juliett", "k", "kay", "kilo", "l", "el", "lima", "m", "em", "mike",
  "n", "en", "november", "o", "oh", "oscar", "p", "pee", "papa", "q", "cue", "quebec",
  "r", "are", "romeo", "s", "ess", "sierra", "t", "tee", "tango", "u", "you", "uniform",
  "v", "vee", "victor", "w", "whiskey", "x", "xray", "y", "why", "yankee", "z", "zee", "zed", "zulu",
])

// Match spoken_text::normalize: ASCII punctuation/case and spoken digits.
export const normalizeSpokenWord = (value: string): string =>
  value.split(/\s+/u).flatMap((word) => {
    const trimmed = word.replace(
      /^[\x21-\x2f\x3a-\x40\x5b-\x60\x7b-\x7e]+|[\x21-\x2f\x3a-\x40\x5b-\x60\x7b-\x7e]+$/g,
      "",
    )
    if (trimmed.length === 0) return []
    const normalized = trimmed.replace(/[A-Z]/g, (character) => character.toLowerCase())
    return [spokenDigits[normalized] ?? normalized]
  }).join(" ")

const descriptorWords = (descriptor: Exclude<CaptureDescriptor, { readonly type: "text" }>): ReadonlySet<string> => {
  if (descriptor.type === "digit") {
    return new Set(Array.from({ length: descriptor.max - descriptor.min + 1 }, (_, index) => String(descriptor.min + index)))
  }
  if (descriptor.type === "letter") return letterAliases
  if (descriptor.type === "choice") return new Set(Object.values(descriptor.choices).flat())
  return new Set(descriptor.members.flatMap((member) => [...descriptorWords(member)]))
}

export const validateUnion = (rawMembers: unknown, label: string, depth = 0): UnionCapture => {
  if (depth >= MAX_UNION_DEPTH) throw new Error(`${label} union nesting may not exceed ${MAX_UNION_DEPTH}`)
  if (!Array.isArray(rawMembers) || rawMembers.length < 2) {
    throw new Error(`${label} union must contain at least two members`)
  }
  let serialized: string
  try {
    serialized = JSON.stringify({ type: "union", members: rawMembers })
  } catch {
    throw new Error(`${label} union must be serializable`)
  }
  if (utf8Length(serialized) > MAX_UNION_SERIALIZED_BYTES) {
    throw new Error(`${label} union exceeds ${MAX_UNION_SERIALIZED_BYTES} serialized bytes`)
  }
  const members = rawMembers.flatMap((member, index) => {
    const validated = validateCaptureDescriptor(member, `${label}.members[${index}]`, depth + 1)
    if (validated.type === "text") throw new Error(`${label} union does not accept text()`)
    return validated.type === "union" ? validated.members : [validated]
  })
  if (members.length > MAX_UNION_MEMBERS) {
    throw new Error(`${label} union may contain at most ${MAX_UNION_MEMBERS} flattened members`)
  }
  const spoken = new Set<string>()
  for (const member of members) {
    for (const word of descriptorWords(member)) {
      if (spoken.has(word)) throw new Error(`${label} union members overlap on spoken word ${word}`)
      spoken.add(word)
    }
  }
  return Object.freeze({ type: "union", members: Object.freeze(members) })
}

export const validateCaptureDescriptor = (
  rawDescriptor: unknown,
  label: string,
  depth = 0,
): CaptureDescriptor => {
  const descriptor = record(rawDescriptor)
  if (descriptor?.type === "digit") {
    const { min, max } = descriptor
    if (typeof min !== "number" || typeof max !== "number"
      || !Number.isInteger(min) || !Number.isInteger(max)
      || min < 0 || max > 9 || min > max) {
      throw new Error(`${label} digit range must be within 0 through 9`)
    }
    if (Object.keys(descriptor).some((key) => !["type", "min", "max"].includes(key))) {
      throw new Error(`${label} contains unsupported fields`)
    }
    return Object.freeze({ type: "digit", min, max })
  }
  if (descriptor?.type === "letter") {
    if (Object.keys(descriptor).length !== 1) throw new Error(`${label} contains unsupported fields`)
    return Object.freeze({ type: "letter" })
  }
  if (descriptor?.type === "text") {
    if (Object.keys(descriptor).length !== 1) throw new Error(`${label} contains unsupported fields`)
    return Object.freeze({ type: "text" })
  }
  if (descriptor?.type === "choice") {
    if (Object.keys(descriptor).some((key) => !["type", "choices"].includes(key))) {
      throw new Error(`${label} contains unsupported fields`)
    }
    const rawChoices = record(descriptor.choices)
    const entries = rawChoices === undefined ? [] : Object.entries(rawChoices)
    if (entries.length === 0 || entries.length > MAX_CHOICE_VALUES) {
      throw new Error(`${label} choice must contain 1 through ${MAX_CHOICE_VALUES} values`)
    }
    const validatedChoices: Array<readonly [string, readonly string[]]> = []
    const spoken = new Set<string>()
    for (const [value, rawAliases] of entries) {
      boundedString(value, `${label} choice value`, MAX_CHOICE_VALUE_BYTES)
      if (!Array.isArray(rawAliases) || rawAliases.length === 0 || rawAliases.length > MAX_CHOICE_ALIASES) {
        throw new Error(`${label}.${value} must contain 1 through ${MAX_CHOICE_ALIASES} aliases`)
      }
      const aliases = rawAliases.map((alias, index) => {
        const bounded = boundedString(alias, `${label}.${value}[${index}]`, MAX_CHOICE_WORD_BYTES)
        const normalized = normalizeSpokenWord(bounded)
        if (normalized.length === 0 || normalized.includes(" ") || utf8Length(normalized) > MAX_CHOICE_WORD_BYTES) {
          throw new Error(`${label} aliases must normalize to exactly one spoken word`)
        }
        if (spoken.has(normalized)) throw new Error(`${label} contains duplicate spoken alias ${normalized}`)
        spoken.add(normalized)
        return normalized
      })
      validatedChoices.push([value, Object.freeze(aliases)])
    }
    return Object.freeze({ type: "choice", choices: Object.freeze(Object.fromEntries(validatedChoices)) })
  }
  if (descriptor?.type === "union") {
    if (Object.keys(descriptor).some((key) => !["type", "members"].includes(key))) {
      throw new Error(`${label} contains unsupported fields`)
    }
    return validateUnion(descriptor.members, label, depth)
  }
  throw new Error(`${label} must be digit(), letter(), choice(), text(), or union()`)
}
