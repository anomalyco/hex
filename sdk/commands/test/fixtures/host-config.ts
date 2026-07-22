import { defineHexConfig } from "../../src/index.js"

export default defineHexConfig({
  commands: {
    greeting: {
      phrases: ["type greeting"],
      run: ({ hex }) => hex.typeText("hello from config"),
    },
  },
})
