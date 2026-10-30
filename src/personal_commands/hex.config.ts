import { defineHexConfig } from "@hex/commands"

// Hex runs this file itself and reloads it when you save. It is plain
// TypeScript or JavaScript with no npm packages, filesystem, or network:
// transformations return text.
export default defineHexConfig({
  // Transformations appear as optional final steps in every dictation mode.
  transformations: {
    "trim-whitespace": {
      name: "Trim whitespace",
      description: "Remove leading and trailing whitespace",
      transform: (text) => text.trim(),
    },
  },
})
