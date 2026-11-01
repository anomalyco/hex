// `node:util` for scripts: `promisify`, honoring `promisify.custom` as Node
// does, so `promisify(execFile)` resolves `{ stdout, stderr }`.

const custom = Symbol.for("nodejs.util.promisify.custom")

export function promisify(original) {
  if (typeof original !== "function") throw new TypeError('The "original" argument must be of type function')
  if (original[custom] !== undefined) {
    const promised = original[custom]
    if (typeof promised !== "function") throw new TypeError('The "util.promisify.custom" property must be of type function')
    return promised
  }
  function promised(...args) {
    return new Promise((resolve, reject) => {
      original.call(this, ...args, (error, value) => (error ? reject(error) : resolve(value)))
    })
  }
  Object.setPrototypeOf(promised, Object.getPrototypeOf(original))
  Object.defineProperty(promised, custom, { value: promised })
  return promised
}

promisify.custom = custom

export default { promisify }
