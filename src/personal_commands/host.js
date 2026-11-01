// Hex's side of a loaded config: the transformation registration Rust compiles,
// and the transformation calls it makes, one transformation at a time so each
// runs under its own timeout. Values cross as JSON strings.

const isRecord = (value) => value !== null && typeof value === "object" && !Array.isArray(value)
const parse = (json) => Object.freeze(JSON.parse(json))

function registerTransformations(config, transforms) {
  if (config.transformations === undefined) return []
  if (!isRecord(config.transformations)) throw new Error("transformations must be an object")
  return Object.entries(config.transformations).map(([id, definition]) => {
    if (!isRecord(definition)) throw new Error(`transformations.${id} must be an object`)
    if (typeof definition.name !== "string") throw new Error(`transformations.${id}.name must be a string`)
    if (typeof definition.transform !== "function") {
      throw new Error(`transformations.${id}.transform must be a function`)
    }
    const { timeout } = definition
    if (timeout !== undefined && !(Number.isInteger(timeout) && timeout >= 1 && timeout <= 60000)) {
      throw new Error(`transformations.${id}.timeout must be whole milliseconds from 1 to 60000`)
    }
    transforms.set(id, definition.transform)
    return { id, name: definition.name, description: definition.description, timeout }
  })
}

export function install(config) {
  if (!isRecord(config)) {
    throw new Error("The config's default export must be a Hex config object")
  }
  const transforms = new Map()
  const registration = JSON.stringify({
    transformations: registerTransformations(config, transforms),
  })
  return {
    registration,
    async transform(id, text, contextJson) {
      const transform = transforms.get(id)
      if (transform === undefined) throw new Error(`Unknown transformation: ${id}`)
      const output = await transform(text, parse(contextJson))
      if (typeof output !== "string") throw new Error(`Transformation ${id} did not return a string`)
      return output
    },
  }
}
