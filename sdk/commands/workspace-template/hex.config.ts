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
  },
})
