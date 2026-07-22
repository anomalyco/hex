import { defineHexConfig, openUrl } from "@hex/commands"

export default defineHexConfig({
  commands: {
    "open-example": {
      phrases: ["open example"],
      group: "Personal",
      description: "Open the example site",
      action: openUrl("https://example.com"),
    },
  },
})
