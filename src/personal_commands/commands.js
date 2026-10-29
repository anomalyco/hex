// The `@hex/commands` module scripts import. `hex.d.ts` types it.

const frozen = (value) => Object.freeze(value)

export const openUrl = (url) => frozen({ type: "openUrl", url })
export const openApplication = (application) => frozen({ type: "openApplication", application })
export const openPath = (path) => frozen({ type: "openPath", path })
export const typeText = (text) => frozen({ type: "typeText", text })

export function press(input) {
  if (typeof input === "string") return frozen({ type: "press", key: input })
  const action = { type: "press", key: input.key }
  if (input.modifiers !== undefined) action.modifiers = [...input.modifiers]
  if (input.repeat !== undefined) action.repeat = input.repeat
  return frozen(action)
}

export const digit = ({ min = 0, max = 9 } = {}) => frozen({ type: "digit", min, max })
export const letter = () => frozen({ type: "letter" })
export const text = () => frozen({ type: "text" })

export function choice(input) {
  const entries = Array.isArray(input) ? input.map((value) => [value, [value]]) : Object.entries(input)
  const choices = {}
  for (const [value, aliases] of entries) choices[value] = [...(choices[value] ?? []), ...aliases]
  return frozen({ type: "choice", choices })
}

export const union = (...members) => frozen({ type: "union", members })

export const defineHexConfig = (config) => config
