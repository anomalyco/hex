import { choice, defineHexConfig, digit, letter, openUrl, union } from "@hex/commands"

// Hex runs this file itself and reloads it when you save. It is plain
// TypeScript or JavaScript with no npm packages, filesystem, or network:
// handlers return actions, and transformations return text.
export default defineHexConfig({
  // Transformations appear as optional final steps in every dictation mode.
  // transformations: {
  //   "trim-whitespace": {
  //     name: "Trim whitespace",
  //     description: "Remove leading and trailing whitespace",
  //     transform: (text) => text.trim(),
  //   },
  // },
  // Uncomment this block to replace Hex's native voice-dictation protocol.
  // dictation: {
  //   start: ["begin note"],
  //   stop: ["finish note"],
  //   send: ["send note"],
  //   cancel: ["discard note"],
  // },
  commands: {
    "open-example": {
      phrases: ["open example"],
      group: "Websites",
      description: "Open the example site",
      action: openUrl("https://example.com"),
    },
    // A trailing {capture} placeholder collects the rest of the spoken
    // phrase: "search amazon for wool socks" -> captures.query === "wool socks".
    "search-amazon": {
      phrases: ["search amazon for {query}"],
      group: "Websites",
      description: "Search Amazon for the spoken words",
      run: ({ captures }) =>
        openUrl(`https://www.amazon.com/s?k=${encodeURIComponent(captures.query ?? "")}`),
    },
    // Explicit descriptors compose multiple captures and infer exact handler types.
    "control-key": {
      phrases: ["control {key}"],
      captures: { key: union(letter(), digit(), choice(["home", "end"] as const)) },
      group: "Keyboard",
      description: "Press Control plus a letter, digit, Home, or End",
      run: ({ hex, captures }) => hex.press({ key: String(captures.key), modifiers: ["control"] }),
    },
  },
})
