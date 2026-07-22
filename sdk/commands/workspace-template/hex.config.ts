import { defineHexConfig } from "@hex/commands"

export default defineHexConfig({
  commands: {
    "open-example": {
      phrases: ["open example"],
      group: "Websites",
      description: "Open the example site",
      run: ({ hex }) => hex.openUrl("https://example.com"),
    },
  },
})
