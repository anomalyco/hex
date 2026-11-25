import XCTest
@testable import HEX

/// Every shared case in `tests/fixtures/hex-words/` (read in place, a folder resource of this
/// target): `read.json`, the phone's cases in `merge.json` and `write.json` (the desktop's are
/// the desktop app's to run), and `hex-words.schema.json` against what the writer writes.
/// Each test prints `HEX-WORDS <file>: <passed>/<run> (<n> desktop cases not run)`.
///
/// `TEST_RUNNER_HEX_WORDS_CROSSCHECK=<folder>` also writes the cross-check's `iphone.hexwords`
/// there from its `expected.json` and checks it against `apps.iphone.file`.
final class HexWordsSharedCasesTests: XCTestCase {
    private lazy var folder: URL = Bundle(for: Self.self).url(forResource: "hex-words", withExtension: nil)!

    private func load(_ name: String) throws -> NSDictionary {
        let data = try Data(contentsOf: folder.appendingPathComponent(name))
        return try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? NSDictionary)
    }

    private func cases(_ name: String) throws -> [NSDictionary] {
        try XCTUnwrap(load(name)["cases"] as? [NSDictionary])
    }

    /// Runs `body`, which returns its mismatches, and fails the test for each.
    private func run(_ name: String, _ body: () throws -> [String]) rethrows -> Bool {
        let problems = try body()
        for problem in problems { XCTFail("\(name): \(problem)") }
        return problems.isEmpty
    }

    private static func same(_ expected: Any?, _ actual: Any?) -> Bool {
        guard let expected = expected as AnyObject?, let actual = actual as AnyObject? else { return expected == nil && actual == nil }
        return expected.isEqual(actual)
    }

    // MARK: read.json

    func testReadCases() throws {
        let all = try cases("read.json")
        var passed = 0
        for item in all {
            let name = item["name"] as? String ?? "?"
            let data: Data
            if let file = item["file"] as? String {
                data = try Data(contentsOf: folder.appendingPathComponent(file))
            } else {
                let recipe = try XCTUnwrap(item["generate"] as? NSDictionary)
                let base = try Data(contentsOf: folder.appendingPathComponent(try XCTUnwrap(recipe["base"] as? String)))
                let size = try XCTUnwrap(recipe["padWithSpacesTo"] as? NSNumber).intValue
                data = base + Data(repeating: 0x20, count: size - base.count)
                XCTAssertEqual(data.count, size, name)
            }
            let expect = try XCTUnwrap(item["expect"] as? NSDictionary)
            let ok = run(name) { Self.compare(HexWordsReader.read(data), to: expect) }
            if ok { passed += 1 }
        }
        print("HEX-WORDS read.json: \(passed)/\(all.count)")
    }

    private static func compare(_ result: HexWordsReadResult, to expect: NSDictionary) -> [String] {
        switch result {
        case .refused(let reason):
            return Self.same(expect["result"], "refused") && Self.same(expect["reason"], reason.rawValue)
                ? [] : ["refused \(reason.rawValue), expected \(expect["result"] ?? "") \(expect["reason"] ?? "")"]
        case .read(let file):
            var madeBy: [String: String] = [:]
            madeBy["app"] = file.madeByApp
            madeBy["version"] = file.madeByVersion
            let actual: [String: Any] = [
                "result": "read",
                "version": file.version,
                "newerVersion": file.isNewerVersion,
                "madeBy": madeBy,
                "vocabulary": file.vocabulary.map { ["term": $0.term, "soundsLike": $0.soundsLike] as [String: Any] },
                "replacements": file.replacements.map { ["heard": $0.heard, "write": $0.write, "learned": $0.learned] as [String: Any] },
            ]
            var problems: [String] = []
            for case let key as String in expect.allKeys where key != "skipped" && key != "skippedCounts" {
                if !Self.same(expect[key], actual[key]) {
                    problems.append("\(key): expected \(expect[key] ?? "nil"), got \(actual[key] ?? "nil")")
                }
            }
            if let skipped = expect["skipped"] as? [NSDictionary] {
                let expected = Set(skipped.map { "\($0["path"] ?? "")|\($0["reason"] ?? "")" })
                let got = Set(file.skipped.map { "\($0.path)|\($0.reason.rawValue)" })
                if expected != got {
                    problems.append("skipped: missing \(expected.subtracting(got).sorted()), extra \(got.subtracting(expected).sorted())")
                }
            }
            if let counts = expect["skippedCounts"] as? [String: NSNumber] {
                let got = Dictionary(grouping: file.skipped, by: \.reason.rawValue).mapValues(\.count)
                if got != counts.mapValues(\.intValue) { problems.append("skippedCounts: got \(got)") }
            }
            return problems
        }
    }

    // MARK: merge.json

    func testPhoneMergeCases() throws {
        let all = try cases("merge.json")
        let phone = all.filter { $0["receiver"] as? String == "phone" }
        var passed = 0
        for item in phone {
            let name = item["name"] as? String ?? "?"
            let ok = try run(name) { try Self.mergeProblems(item) }
            if ok { passed += 1 }
        }
        print("HEX-WORDS merge.json: \(passed)/\(phone.count) (\(all.count - phone.count) desktop cases not run)")
    }

    private static func lists(_ value: Any?) -> HexWordsLists {
        let dictionary = value as? NSDictionary
        return HexWordsLists(
            vocabulary: dictionary?["vocabulary"] as? [String] ?? [],
            replacements: (dictionary?["replacements"] as? [NSDictionary] ?? []).map {
                WordReplacement(heard: $0["heard"] as? String ?? "", write: $0["write"] as? String ?? "")
            }
        )
    }

    private static func listsShape(_ lists: HexWordsLists) -> [String: Any] {
        ["vocabulary": lists.vocabulary, "replacements": lists.replacements.map { ["heard": $0.heard, "write": $0.write] }]
    }

    private static func mergeProblems(_ item: NSDictionary) throws -> [String] {
        let suite = "hex-words-merge-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let before = lists(item["before"])
        CustomVocabularySettings(defaults: defaults).isEnabled = true
        CustomVocabularySettings(defaults: defaults).terms = before.vocabulary
        WordReplacementsSettings(defaults: defaults).entries = before.replacements

        let incomingObject = try XCTUnwrap(item["incoming"] as? NSDictionary)
        let incoming = HexWordsFile(
            vocabulary: (incomingObject["vocabulary"] as? [NSDictionary] ?? []).map {
                HexWordsTerm(term: $0["term"] as? String ?? "", soundsLike: $0["soundsLike"] as? [String] ?? [])
            },
            replacements: (incomingObject["replacements"] as? [NSDictionary] ?? []).map {
                HexWordsReplacement(heard: $0["heard"] as? String ?? "", write: $0["write"] as? String ?? "", learned: ($0["learned"] as? Bool) ?? false)
            }
        )
        var limits = HexWordsMerge.Limits()
        if let override = item["limits"] as? NSDictionary {
            if let terms = override["terms"] as? NSNumber { limits.terms = terms.intValue }
            if let replacements = override["replacements"] as? NSNumber { limits.replacements = replacements.intValue }
        }
        var parts = HexWordsMerge.Parts()
        if let override = item["parts"] as? NSDictionary {
            if let vocabulary = override["vocabulary"] as? Bool { parts.vocabulary = vocabulary }
            if let replacements = override["replacements"] as? Bool { parts.replacements = replacements }
        }
        let useTheirs = Set(((item["choices"] as? [String: String]) ?? [:]).filter { $0.value == "useTheirs" }.keys)

        let store = HexWordsImport(defaults: defaults)
        var problems: [String] = []
        guard store.lists == before else { return ["the fixture's before didn't store as given: \(store.lists)"] }
        let result = store.add(incoming, parts: parts, useTheirs: useTheirs, limits: limits)
        let expect = try XCTUnwrap(item["expect"] as? NSDictionary)

        if !same(expect["after"], listsShape(store.lists)) { problems.append("after: got \(listsShape(store.lists))") }
        if !same(expect["after"], listsShape(result.after)) { problems.append("merge after: got \(listsShape(result.after))") }
        let summary = result.summary
        let gotSummary: [String: Int] = [
            "addedTerms": summary.addedTerms, "updatedTerms": summary.updatedTerms, "alreadyHaveTerms": summary.alreadyHaveTerms,
            "addedReplacements": summary.addedReplacements, "changedReplacements": summary.changedReplacements,
            "keptYours": summary.keptYours, "alreadyHaveReplacements": summary.alreadyHaveReplacements,
        ]
        if !same(expect["summary"], gotSummary) { problems.append("summary: got \(gotSummary)") }
        let notApplied = result.notApplied.map { ["path": $0.path, "reason": $0.reason.rawValue] }
        if !same(expect["notApplied"], notApplied) { problems.append("notApplied: got \(notApplied)") }
        let clashes = result.clashes.map { ["path": $0.path, "heard": $0.heard, "yours": $0.yours, "theirs": $0.theirs] }
        if !same(expect["clashes"], clashes) { problems.append("clashes: got \(clashes)") }

        if !store.undo() { problems.append("Undo wasn't available") }
        if store.lists != before { problems.append("Undo gave \(store.lists), not before") }
        if !CustomVocabularySettings(defaults: defaults).isEnabled { problems.append("Undo turned Custom vocabulary off") }
        return problems
    }

    // MARK: write.json

    func testPhoneWriteCases() throws {
        let all = try cases("write.json")
        let phone = all.filter { $0["writer"] as? String == "phone" }
        let schema = try load("hex-words.schema.json")
        var passed = 0
        for item in phone {
            let name = item["name"] as? String ?? "?"
            let input = try XCTUnwrap(item["input"] as? NSDictionary)
            let expect = try XCTUnwrap(item["expect"] as? NSDictionary)
            let ok = try run(name) {
                try Self.writeProblems(input: input, expectFile: expect["file"], notExported: expect["notExported"], schema: schema)
            }
            if ok { passed += 1 }
        }
        print("HEX-WORDS write.json: \(passed)/\(phone.count) (\(all.count - phone.count) desktop cases not run)")
    }

    private static func write(_ input: NSDictionary) -> HexWordsWriter.Output {
        let replacements = (input["replacements"] as? [NSDictionary] ?? []).map {
            WordReplacement(heard: $0["heard"] as? String ?? "", write: $0["write"] as? String ?? "")
        }
        let learned = (input["learnedFixes"] as? [NSDictionary] ?? []).map {
            HexWordsWriter.LearnedFix(heard: $0["heard"] as? String ?? "", meant: $0["meant"] as? String ?? "", active: $0["active"] as? Bool ?? false)
        }
        return HexWordsWriter.phone(
            vocabulary: input["vocabulary"] as? [String] ?? [],
            replacements: replacements,
            learnedFixes: learned,
            includeLearned: input["includeLearned"] as? Bool ?? false,
            appVersion: "9.0.0"
        )
    }

    private static func writeProblems(input: NSDictionary, expectFile: Any?, notExported: Any?, schema: NSDictionary) throws -> [String] {
        let output = write(input)
        var problems: [String] = []
        let bytes = [UInt8](output.data)
        if bytes.starts(with: [0xEF, 0xBB, 0xBF]) { problems.append("starts with a byte order mark") }
        if Array(String(decoding: bytes, as: UTF8.self).utf8) != bytes { problems.append("not UTF-8") }
        let parsed = try XCTUnwrap(JSONSerialization.jsonObject(with: output.data) as? NSDictionary)
        problems += SchemaCheck(root: schema).errors(in: parsed)
        let madeBy = parsed["madeBy"] as? NSDictionary
        if madeBy?["app"] as? String != HexWordsWriter.appName { problems.append("madeBy.app is \(madeBy?["app"] ?? "missing")") }
        let file = NSMutableDictionary(dictionary: parsed)
        file.removeObject(forKey: "madeBy")
        if !same(expectFile, file) { problems.append("file: got \(file)") }
        let expected = Set((notExported as? [NSDictionary] ?? []).map { "\($0["path"] ?? "")|\($0["reason"] ?? "")" })
        let got = Set(output.notExported.map { "\($0.path)|\($0.reason.rawValue)" })
        if expected != got { problems.append("notExported: got \(got.sorted())") }
        if case .read(let back) = HexWordsReader.read(output.data) {
            if !back.skipped.isEmpty { problems.append("reads back with skips: \(back.skipped)") }
        } else {
            problems.append("doesn't read back")
        }
        return problems
    }

    // MARK: madeBy

    /// `madeBy.version` is the running app's `CFBundleShortVersionString` (`MARKETING_VERSION`),
    /// never a placeholder: Export passes `HexWordsWriter.appVersion()`, and the writer writes
    /// whatever version it is given.
    func testWriterWritesTheBundlesVersion() throws {
        let bundleVersion = try XCTUnwrap(Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String)
        XCTAssertFalse(bundleVersion.isEmpty)
        XCTAssertEqual(HexWordsWriter.appVersion(), bundleVersion)
        for version in [HexWordsWriter.appVersion(), "1.0.7"] {
            let output = HexWordsWriter.phone(vocabulary: ["Rune"], replacements: [], appVersion: version)
            guard case .read(let file) = HexWordsReader.read(output.data) else { return XCTFail("refused") }
            XCTAssertEqual(file.madeByApp, "Hex for iPhone")
            XCTAssertEqual(file.madeByVersion, version)
        }
    }

    // MARK: The cross-check's iPhone file

    func testCrossCheckExport() throws {
        guard let path = ProcessInfo.processInfo.environment["HEX_WORDS_CROSSCHECK"] else {
            throw XCTSkip("Set TEST_RUNNER_HEX_WORDS_CROSSCHECK to the cross-check folder")
        }
        let folder = URL(fileURLWithPath: path, isDirectory: true)
        let expected = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: folder.appendingPathComponent("expected.json"))) as? NSDictionary)
        let app = try XCTUnwrap((expected["apps"] as? NSDictionary)?["iphone"] as? NSDictionary)
        let schema = try load("hex-words.schema.json")
        let output = Self.write(try XCTUnwrap(app["input"] as? NSDictionary))
        try output.data.write(to: folder.appendingPathComponent("iphone.hexwords"), options: .atomic)
        let problems = try Self.writeProblems(
            input: try XCTUnwrap(app["input"] as? NSDictionary), expectFile: app["file"], notExported: app["notExported"], schema: schema
        )
        for problem in problems { XCTFail("iphone.hexwords: \(problem)") }
    }
}

/// The JSON Schema 2020-12 keywords `hex-words.schema.json` uses, enough to check a writer.
private struct SchemaCheck {
    let root: NSDictionary

    func errors(in value: Any) -> [String] {
        var errors: [String] = []
        check(value, against: root, at: "$", errors: &errors)
        return errors
    }

    private func check(_ value: Any, against schema: NSDictionary, at path: String, errors: inout [String]) {
        if let ref = schema["$ref"] as? String, ref.hasPrefix("#/$defs/"),
           let target = (root["$defs"] as? NSDictionary)?[String(ref.dropFirst(8))] as? NSDictionary {
            check(value, against: target, at: path, errors: &errors)
        }
        for case let part as NSDictionary in schema["allOf"] as? [Any] ?? [] {
            check(value, against: part, at: path, errors: &errors)
        }
        if let constant = schema["const"], !(constant as AnyObject).isEqual(value) {
            errors.append("\(path): not \(constant)")
        }
        let isBool = (value as? NSNumber).map { CFGetTypeID($0) == CFBooleanGetTypeID() } ?? false
        switch schema["type"] as? String {
        case "object": if !(value is NSDictionary) { errors.append("\(path): not an object") }
        case "array": if !(value is NSArray) { errors.append("\(path): not an array") }
        case "string": if !(value is NSString) { errors.append("\(path): not a string") }
        case "boolean": if !isBool { errors.append("\(path): not a boolean") }
        default: break
        }
        if let object = value as? NSDictionary {
            let properties = schema["properties"] as? NSDictionary ?? [:]
            for case let key as String in schema["required"] as? [Any] ?? [] where object[key] == nil {
                errors.append("\(path): no \(key)")
            }
            for case let key as String in object.allKeys {
                if let property = properties[key] as? NSDictionary {
                    check(object[key]!, against: property, at: path + "." + key, errors: &errors)
                } else if (schema["additionalProperties"] as? Bool) == false {
                    errors.append("\(path): \(key) isn't allowed")
                }
            }
        }
        if let array = value as? [Any] {
            if let min = schema["minItems"] as? Int, array.count < min { errors.append("\(path): under \(min) items") }
            if let max = schema["maxItems"] as? Int, array.count > max { errors.append("\(path): over \(max) items") }
            if schema["uniqueItems"] as? Bool == true, NSSet(array: array).count != array.count {
                errors.append("\(path): items repeat")
            }
            if let items = schema["items"] as? NSDictionary {
                for (index, element) in array.enumerated() { check(element, against: items, at: "\(path)[\(index)]", errors: &errors) }
            }
        }
        if let string = value as? String {
            let length = string.unicodeScalars.count
            if let min = schema["minLength"] as? Int, length < min { errors.append("\(path): shorter than \(min)") }
            if let max = schema["maxLength"] as? Int, length > max { errors.append("\(path): longer than \(max)") }
            if let pattern = schema["pattern"] as? String {
                let expression = try? NSRegularExpression(pattern: pattern)
                let range = NSRange(string.startIndex..., in: string)
                if expression?.firstMatch(in: string, range: range) == nil { errors.append("\(path): doesn't match \(pattern)") }
            }
        }
    }
}
