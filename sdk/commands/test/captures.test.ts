import { describe, expect, it } from "vitest"
import { prepareConfig } from "../src/host.js"
import { choice, digit, letter, union } from "../src/model.js"
import type { AtomicCaptureDescriptor } from "../src/model.js"

const registration = (capture: unknown) => prepareConfig({
  commands: {
    choose: { phrases: ["choose {value}"], captures: { value: capture }, run: async () => {} },
  },
}).registration.commands[0]?.captures?.value

describe("constructor and host capture agreement", () => {
  it("preserves distinct non-ASCII case while normalizing ASCII and spoken digits", () => {
    const members = [
      choice({ upper: ["Ä"], lower: ["ä"] }),
      choice({ count: [" TWO! "], home: ["HOME"] }),
    ] as const
    const constructed = union(...members)
    expect(registration({ type: "union", members })).toEqual(constructed)
    expect(registration(constructed)).toEqual(constructed)
    expect(constructed.members).toEqual([
      { type: "choice", choices: { upper: ["Ä"], lower: ["ä"] } },
      { type: "choice", choices: { count: ["2"], home: ["home"] } },
    ])
    expect(members[1].choices.count).toEqual([" TWO! "]) // no mutation of supplied descriptors
    expect(Object.isFrozen(constructed.members)).toBe(true)
  })

  const overlaps: readonly (readonly [AtomicCaptureDescriptor, AtomicCaptureDescriptor])[] = [
    [digit(), choice({ count: [" TWO! "] })],
    [letter(), choice({ first: ["ALPHA!"] })],
    [choice({ first: ["Home"] }), choice({ second: ["home!"] })],
    [choice({ first: ["! home"] }), choice({ second: ["home"] })],
  ]
  it.each(overlaps)("rejects the same normalized overlaps in constructors and raw descriptors", (first, second) => {
    expect(() => union(first, second)).toThrow("overlap")
    expect(() => registration({ type: "union", members: [first, second] })).toThrow("overlap")
  })

  it("shares nesting, flattening, member bounds, and invalid-leaf rejection", () => {
    // Exercise structural input from JavaScript, including unflattened nested unions.
    type RawUnion = { readonly type: "union"; readonly members: readonly (AtomicCaptureDescriptor | RawUnion)[] }
    let nested: RawUnion = { type: "union", members: [choice(["first"]), choice(["second"])] }
    for (let depth = 0; depth < 3; depth++) {
      nested = { type: "union", members: [choice([`level${depth}`]), nested] }
    }
    expect(registration(nested)).toEqual(Reflect.apply(union, undefined, nested.members))
    expect(() => Reflect.apply(union, undefined, [choice(["outer"]), nested])).toThrow("nesting")
    expect(() => registration({ type: "union", members: [choice(["outer"]), nested] })).toThrow("nesting")

    const excessive: [AtomicCaptureDescriptor, AtomicCaptureDescriptor, ...AtomicCaptureDescriptor[]] = [
      choice(["first"]), choice(["second"]), ...Array.from({ length: 15 }, (_, index) => choice([`extra${index}`])),
    ]
    expect(() => union(...excessive)).toThrow("at most 16")
    expect(() => registration({ type: "union", members: excessive })).toThrow("at most 16")
    expect(() => union(digit({ min: 4, max: 2 }), letter())).toThrow("digit range")
    expect(() => registration({ type: "union", members: [digit({ min: 4, max: 2 }), letter()] })).toThrow("digit range")
  })
})
