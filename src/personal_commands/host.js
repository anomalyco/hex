// Hex's side of a loaded config: the registration Rust compiles, and the
// handler and transformation calls it makes. Values cross as JSON strings.

import { openApplication, openPath, openUrl, press, typeText } from "@hex/commands"

const ACTIONS = new Set(["openUrl", "openApplication", "openPath", "press", "typeText"])

const isRecord = (value) => value !== null && typeof value === "object" && !Array.isArray(value)
const isAction = (value) => isRecord(value) && ACTIONS.has(value.type)
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
    transforms.set(id, definition.transform)
    return { id, name: definition.name, description: definition.description }
  })
}

function registerCommands(config, handlers) {
  return Object.entries(config.commands).map(([id, definition]) => {
    if (!isRecord(definition)) throw new Error(`commands.${id} must be an object`)
    if ((definition.action === undefined) === (definition.run === undefined)) {
      throw new Error(`commands.${id} must contain exactly one of action or run`)
    }
    if (!Array.isArray(definition.phrases)) throw new Error(`commands.${id}.phrases must be a list of phrases`)
    const field = definition.action === undefined ? "run" : "action"
    const value = definition[field]
    let execution
    if (typeof value === "function") {
      handlers.set(id, value)
      execution = { type: "handler" }
    } else if (isAction(value)) {
      execution = { type: "native", action: value }
    } else {
      throw new Error(`commands.${id}.${field} must be a function or an action such as openUrl(...)`)
    }
    const when = definition.when
    if (when !== undefined
      && (!isRecord(when) || (when.application === undefined) === (when.browserHost === undefined))) {
      throw new Error(`commands.${id}.when must contain exactly one of application or browserHost`)
    }
    return {
      id,
      phrases: definition.phrases,
      group: definition.group,
      description: definition.description,
      when,
      captures: definition.captures,
      execution,
    }
  })
}

export function install(config) {
  if (!isRecord(config) || !isRecord(config.commands)) {
    throw new Error("The config's default export must be a Hex config with a commands object")
  }
  const transforms = new Map()
  const handlers = new Map()
  const registration = JSON.stringify({
    dictation: config.dictation,
    transformations: registerTransformations(config, transforms),
    commands: registerCommands(config, handlers),
  })
  return {
    registration,
    async invoke(id, contextJson, capturesJson) {
      const handler = handlers.get(id)
      if (handler === undefined) throw new Error(`Command ${id} has no handler`)
      const actions = []
      const queue = (action) => {
        actions.push(action)
        return Promise.resolve()
      }
      const hex = Object.freeze({
        openUrl: (url) => queue(openUrl(url)),
        openApplication: (application) => queue(openApplication(application)),
        openPath: (path) => queue(openPath(path)),
        press: (input) => queue(press(input)),
        typeText: (text) => queue(typeText(text)),
      })
      const returned = await handler({ hex, context: parse(contextJson), captures: parse(capturesJson) })
      for (const action of returned === undefined ? [] : Array.isArray(returned) ? returned : [returned]) {
        if (!isAction(action)) {
          throw new Error(`Command ${id} must return an action, a list of actions, or nothing`)
        }
        actions.push(action)
      }
      return JSON.stringify(actions)
    },
    async transform(idsJson, text, contextJson) {
      const context = parse(contextJson)
      let output = text
      for (const id of JSON.parse(idsJson)) {
        const transform = transforms.get(id)
        if (transform === undefined) throw new Error(`Unknown transformation: ${id}`)
        output = await transform(output, context)
        if (typeof output !== "string") throw new Error(`Transformation ${id} did not return a string`)
      }
      return output
    },
  }
}
