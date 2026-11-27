// Writes guard-reference.json: what the iOS CleanupGuard decides, and what
// CleanupReconstruction then rebuilds, for the hand-written sentences at both
// of their recorded P(DELETE) sets, for seeded random markings of the same
// sentences, and for the probe sentences below. The Rust guard and renderer
// must reproduce every case (parity_tests.rs).
//
// Run from the repository root after changing either Swift file:
//
//   swiftc -O ios/HEXShared/Formatting/CleanupGuard.swift \
//     ios/HEXShared/Formatting/CleanupReconstruction.swift \
//     src/dictation/cleanup/fixtures/guard_reference.swift -o /tmp/guard-reference
//   /tmp/guard-reference ios/HEXMobileTests/Fixtures/cleanup-tagger-handwritten.json \
//     > src/dictation/cleanup/fixtures/guard-reference.json
import Foundation

private struct Handwritten: Decodable {
    struct Case: Decodable {
        let input: String
        let pDelete: [Double]
        let checkpointPDelete: [Double]
    }

    let cases: [Case]
}

private struct Reference: Encodable {
    let words: [String]
    let pDelete: [Double]
    let terms: [String]
    let labels: [Int]
    let output: String
}

/// SplitMix64, so a regenerated fixture has the same cases.
private struct Random {
    var state: UInt64

    mutating func next() -> UInt64 {
        state &+= 0x9E37_79B9_7F4A_7C15
        var z = state
        z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
        z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
        return z ^ (z >> 31)
    }

    mutating func below(_ bound: Int) -> Int {
        Int(next() % UInt64(bound))
    }
}

/// One value on each side of every guard threshold (0.98, 0.995, 0.999).
private let levels = [0.985, 0.997, 0.9995]

private let probes = [
    "Tell Hye-jin I said thanks.",
    "Let's do it later, maybe after dinner.",
    "So w- we should go.",
    "It costs 15 euros, er, 50 euros.",
    "Give me 3, uh, 4 of them.",
    "I I think the, the office is closed.",
    "Did you, have you seen it?",
    "It was like super expensive.",
    "It was like a dream.",
    "I like it and I would like to go.",
    "It was really, really good.",
    "It's, you know, complicated.",
    "You know I mean what I say.",
    "I mean, it works, you know, mostly.",
    "Send it to John, I mean Mike, by Friday.",
    "Meet me Tuesday, no wait, Thursday.",
    "The follow-up is at 3pm, sorry, 4pm.",
    "We use Postgres, actually MySQL, for that.",
    "Um, so, uh, yeah.",
    "um uh",
    "Like, I was like, whatever.",
    "No, no, no, that's not what I meant.",
    "Never, ever, ever share that link outside the team.",
    "I said stop, stop, stop the deploy.",
    "Um is a filler word, and so is uh.",
    "Wait wait wait, go back one slide.",
    "She said \"um\" a lot.",
    "I don't, I do want the red one.",
    "Call Jean-Luc about the the budget, it's $50, no, $15.",
    "It's 100% done... uh, 90% done.",
    "We went to to Paris. Then, um, Rome.",
    "The thing is; the the plan \u{2014} I mean the schedule \u{2014} changed.",
    "Caf\u{E9}, um, caf\u{E9} is closed.",
    "that's that saves us what four hundred euros",
    "i'm leaning towards the apartment because um over nine nights",
    "I'm sorry, I meant the second one.",
    "Scratch that, make it five.",
    "He said: Actually it works.",
    "uh-huh, mhm, I, I'm, I'm not sure.",
    "A B C, C D.",
    "- - -",
    "It is... well, it was, like, 5 or 6, um, 7.",
    "Maybe, maybe not.",
    "I think I think we should should go go.",
    "Can you, um, can you send it?",
]

@main
private enum GuardReference {
    static func main() throws {
        let path = CommandLine.arguments[1]
        let handwritten = try JSONDecoder().decode(Handwritten.self, from: Data(contentsOf: URL(fileURLWithPath: path)))
        var random = Random(state: 0x4845_58)
        var cases: [Reference] = []

        for item in handwritten.cases {
            let words = split(item.input)
            cases.append(reference(words, item.pDelete, terms: []))
            cases.append(reference(words, item.checkpointPDelete, terms: []))
            cases.append(reference(words, marking(words.count, &random), terms: terms(words, &random)))
        }
        for probe in probes {
            let words = split(probe)
            for level in levels {
                cases.append(reference(words, Array(repeating: level, count: words.count), terms: []))
            }
            for round in 0..<4 {
                let terms = round % 2 == 0 ? [] : terms(words, &random)
                cases.append(reference(words, marking(words.count, &random), terms: terms))
            }
        }

        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        let lines = try cases.map { String(decoding: try encoder.encode($0), as: UTF8.self) }
        print("{\"reference\": \"ios/HEXShared/Formatting/CleanupGuard.swift\", \"cases\": [")
        print(lines.joined(separator: ",\n"))
        print("]}")
    }

    private static func split(_ text: String) -> [String] {
        text.split(whereSeparator: \.isWhitespace).map(String.init)
    }

    private static func reference(_ words: [String], _ probabilities: [Double], terms: [String]) -> Reference {
        let deletes = CleanupGuard.deletes(
            words, probabilities: probabilities, protectedTerms: CleanupGuard.protectedTerms(terms)
        )
        return Reference(
            words: words,
            pDelete: probabilities,
            terms: terms,
            labels: deletes.map { $0 ? 1 : 0 },
            output: CleanupReconstruction.reconstruct(words, deletes: deletes)
        )
    }

    /// Runs of marked words at random levels between runs of unmarked ones.
    private static func marking(_ count: Int, _ random: inout Random) -> [Double] {
        var marked = random.below(2) == 0
        return (0..<count).map { _ in
            if random.below(3) == 0 { marked.toggle() }
            return marked ? levels[random.below(levels.count)] : 0.5
        }
    }

    /// One or two of the sentence's own words as custom vocabulary.
    private static func terms(_ words: [String], _ random: inout Random) -> [String] {
        (0...random.below(2)).map { _ in words[random.below(words.count)] }
    }
}
