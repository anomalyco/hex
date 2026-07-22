import { describe, expect, it } from "vitest"
import {
  defineHexConfig,
  openApplication,
  openPath,
  openUrl,
  press,
  typeText,
} from "../src/index.js"

describe("command model", () => {
  it("builds every native descriptor", () => {
    expect(openUrl("https://example.com")).toEqual({ type: "openUrl", url: "https://example.com" })
    expect(openApplication("Slack")).toEqual({ type: "openApplication", application: "Slack" })
    expect(openPath("/tmp/example")).toEqual({ type: "openPath", path: "/tmp/example" })
    expect(press("escape")).toEqual({ type: "press", key: "escape" })
    expect(press({ key: "p", modifiers: ["command", "shift"], repeat: 2 })).toEqual({
      type: "press",
      key: "p",
      modifiers: ["command", "shift"],
      repeat: 2,
    })
    expect(typeText("fixed text")).toEqual({ type: "typeText", text: "fixed text" })
  })

  it("preserves normal TypeScript record composition", () => {
    const navigation = {
      home: { phrases: ["open home"] as const, action: openUrl("https://example.com") },
    }
    const config = defineHexConfig({
      commands: {
        ...navigation,
        slack: { phrases: ["open slack"], run: openApplication("Slack") },
      },
    })
    expect(Object.keys(config.commands)).toEqual(["home", "slack"])
  })
})
