import { defineHexConfig } from "@hex/commands"

export default defineHexConfig({
  // Transformations appear as optional final steps in every dictation mode.
  // transformations: {
  //   lowercase: {
  //     name: "Lowercase",
  //     description: "Convert the final text to lowercase",
  //     transform: (text) => text.toLowerCase(),
  //   },
  // },
  // Uncomment this block to replace HEX's native voice-dictation protocol.
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
      run: ({ hex }) => hex.openUrl("https://example.com"),
    },
    // A trailing {capture} placeholder collects the rest of the spoken
    // phrase: "search amazon for wool socks" -> captures.query === "wool socks".
    "search-amazon": {
      phrases: ["search amazon for {query}"],
      group: "Websites",
      description: "Search Amazon for the spoken words",
      run: ({ hex, captures }) =>
        hex.openUrl(`https://www.amazon.com/s?k=${encodeURIComponent(captures.query ?? "")}`),
    },
  },
})
