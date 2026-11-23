import Foundation
import XCTest
@testable import HEX

/// The shipped autocorrect against the Swift oracle in one process, on the exporter's cases:
/// `HEX_AUTOCORRECT_INPROCESS_COUNT` for each language with a typing model and a quarter as
/// many for the others, plus the literal cases. The oracle answers as the port is decided to
/// (own words by key, ascending ties, case folding, the shared never-suggest list); every
/// answer, state and dictionary question must be the same. Goes with the oracle.
@MainActor
final class AutocorrectInProcessParity: XCTestCase {
    func testTheShippedAutocorrectAnswersAsTheOracle() throws {
        guard let count = ProcessInfo.processInfo.environment["HEX_AUTOCORRECT_INPROCESS_COUNT"].flatMap(Int.init) else {
            throw XCTSkip("set TEST_RUNNER_HEX_AUTOCORRECT_INPROCESS_COUNT to run the in-process parity")
        }
        try compare(count: count, system: false)
    }

    /// The same with Apple's dictionary (`SystemTextChecker`, and Hex's own words through
    /// `useOwnWords`) behind both: only the answers are compared, as nothing logs its questions.
    func testTheShippedAutocorrectAnswersAsTheOracleWithApplesDictionary() throws {
        guard let count = ProcessInfo.processInfo.environment["HEX_AUTOCORRECT_SYSTEM_COUNT"].flatMap(Int.init) else {
            throw XCTSkip("set TEST_RUNNER_HEX_AUTOCORRECT_SYSTEM_COUNT to run the parity with Apple's dictionary")
        }
        try compare(count: count, system: true)
    }

    private func compare(count: Int, system: Bool) throws {
        let repo = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent().path
        ACOracleFiles.repo = repo
        var oracleModels: [String: TapOracleModel] = [:]
        var models: [String: TypingDecoderModel] = [:]
        for code in ["en", "es", "de", "fr"] {
            let url = URL(fileURLWithPath: repo + "/ios/HEXKeyboard/TypingModel/\(code).hextm")
            oracleModels[code] = try XCTUnwrap(TapOracleModel(url: url))
            models[code] = try XCTUnwrap(TypingDecoderModel(url: url))
        }
        let (dictionaries, _) = AutocorrectParityExport.dictionaries(models: oracleModels)
        let shippedModels = models
        if system {
            ACOracle.ownWordsHook = { (oracle: ACOracle, model: TapOracleModel?) in
                guard let checker = oracle.checker as? ACSystemChecker else { return }
                let theirs: TypingDecoderModel? = model.flatMap { shippedModels[$0.language] }
                AutocorrectEngine.useOwnWords(theirs, in: checker.system)
            }
        }
        defer { ACOracle.ownWordsHook = nil }
        var keysets: [String: ACParityGenerator.KeySet] = [:]
        var keysetsByID: [Int: TapOracle.Keys] = [:]
        var nextID = 0
        var totals: [String: Int] = [:]
        var differ = 0, cases = 0, operations = 0, calls = 0, oracleDiffer = 0
        // `HEX_AUTOCORRECT_SYSTEM_ONLY=12,40`: only these cases (made as always), each also run
        // through a second oracle, to tell the dictionary's own variation from the engine's.
        let only = ProcessInfo.processInfo.environment["HEX_AUTOCORRECT_SYSTEM_ONLY"]
            .map { Set($0.split(separator: ",").compactMap { Int($0) }) }
        func check(_ c: ACParityGenerator.Case, _ language: KeyboardLanguage, _ generator: inout ACParityGenerator) {
            keysets = generator.keysets
            keysetsByID = generator.keysetsByID
            nextID = generator.nextKeysetID
            generator.keysetLines.removeAll()
            cases += 1
            if let only, !only.contains(cases - 1) { return }
            var runner = ACParityRunner(dictionaries: dictionaries, models: oracleModels, keysets: keysetsByID)
            runner.system = system
            let oracle = AutocorrectParityExport.run(runner, c, order: .byKey, tie: .ascending, shared: true, folding: true)
            let keys = keysetsByID.mapValues {
                TypingDecoder.Keys(centres: $0.centres, pitch: $0.pitch, bottomRowY: $0.bottomRowY, space: $0.space)
            }
            var wrapper = ACWrapperRunner(dictionaries: dictionaries, models: models, keysets: keys)
            wrapper.system = system
            let shipped = wrapper.run(c.ops, initialLearned: c.learned)
            operations += c.ops.count
            calls += oracle.calls
            totals[language.rawValue, default: 0] += 1
            if only != nil {
                let again = AutocorrectParityExport.run(runner, c, order: .byKey, tie: .ascending, shared: true, folding: true)
                if !ACWrapperRunner.same(again.outputs, oracle.outputs) {
                    oracleDiffer += 1
                    print("AUTOCORRECT-ORACLE-AGAIN case \(cases - 1): \(ACWrapperRunner.firstDifference(again.outputs, oracle.outputs, ops: c.ops))")
                }
            }
            if !ACWrapperRunner.same(shipped.outputs, oracle.outputs) || (!system && shipped.hash != oracle.hash) {
                differ += 1
                if differ <= 10 {
                    XCTFail("case \(cases - 1) (\(language.rawValue)): \(ACWrapperRunner.firstDifference(shipped.outputs, oracle.outputs, ops: c.ops)) calls \(shipped.calls)/\(oracle.calls)")
                }
            }
        }
        let languages: [KeyboardLanguage] = [.english, .spanish, .german, .french, .frenchCanadian, .frenchSwiss, .italian,
                                             .portuguese, .polish, .czech, .hungarian, .romanian, .norwegian, .russian, .ukrainian, .greek,
                                             .serbianLatin, .korean]
        for (n, language) in languages.enumerated() {
            var generator = ACParityGenerator(
                language: language, dictionaries: dictionaries, models: oracleModels, rng: TapParityRandom(state: 0xAC_0C0E_0000 &+ UInt64(n)),
                keysets: keysets, keysetsByID: keysetsByID, nextKeysetID: nextID
            )
            let main = [KeyboardLanguage.english, .spanish, .german, .french].contains(language)
            for _ in 0..<(main ? count : max(1, count / 4)) {
                autoreleasepool {
                    let c = generator.makeCase()
                    check(c, language, &generator)
                }
            }
        }
        var literalGenerator = ACParityGenerator(
            language: .english, dictionaries: dictionaries, models: oracleModels, rng: TapParityRandom(state: 0xAC_7E57),
            keysets: keysets, keysetsByID: keysetsByID, nextKeysetID: nextID
        )
        for c in ACParityLiterals.cases(repo: repo, generator: &literalGenerator, stride: 1) {
            check(c, .english, &literalGenerator)
        }
        print("AUTOCORRECT-INPROCESS system=\(system) cases=\(cases) ops=\(operations) calls=\(calls) differ=\(differ) oracleAgainDiffer=\(oracleDiffer) " + totals.keys.sorted().map { "\($0)=\(totals[$0]!)" }.joined(separator: " "))
        XCTAssertEqual(differ, 0, "of \(cases) cases")
    }
}
