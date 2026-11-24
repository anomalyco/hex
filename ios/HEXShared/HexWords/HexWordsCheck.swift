import Foundation

/// The check pass (`docs/specs/hex-words.md`, "Reading" step 5): one walk over the bytes before
/// `JSONSerialization` builds anything. It never recurses, so 10,000 levels can't exhaust a
/// stack, and it refuses for the first problem met reading from the start: the opening bracket
/// that goes past 8 levels, a key at its second appearance in one object (compared after
/// decoding escapes, byte for byte, never by Swift's canonical equivalence), or where the text
/// stops being strict RFC 8259 JSON, which `JSONSerialization` doesn't hold to: a repeated key
/// keeps its last value there, it decodes an escaped lone surrogate, and a number no 64-bit
/// float can hold is notJSON for this format.
nonisolated enum HexWordsCheck {
    static let maxDepth = 8

    private enum Expect {
        case value, valueOrClose, key, keyOrClose, colon, commaOrClose, end
    }

    private struct Container {
        let isObject: Bool
        var keys: Set<[UInt8]> = []
    }

    private enum Failure: Error {
        case refused(HexWordsRefusal)
    }

    /// nil when the text is strict JSON within the limits. `bytes` is valid UTF-8 without a BOM.
    static func check(_ bytes: [UInt8]) -> HexWordsRefusal? {
        do {
            try walk(bytes)
            return nil
        } catch Failure.refused(let reason) {
            return reason
        } catch {
            return .notJSON
        }
    }

    private static func walk(_ bytes: [UInt8]) throws {
        var stack: [Container] = []
        var expect = Expect.value
        var index = 0
        let count = bytes.count

        func afterValue() {
            expect = stack.isEmpty ? .end : .commaOrClose
        }

        while true {
            while index < count, isSpace(bytes[index]) { index += 1 }
            guard index < count else {
                guard expect == .end else { throw Failure.refused(.notJSON) }
                return
            }
            let byte = bytes[index]
            switch expect {
            case .value, .valueOrClose:
                if byte == UInt8(ascii: "]"), expect == .valueOrClose {
                    stack.removeLast()
                    index += 1
                    afterValue()
                    continue
                }
                switch byte {
                case UInt8(ascii: "{"), UInt8(ascii: "["):
                    guard stack.count < maxDepth else { throw Failure.refused(.tooDeep) }
                    let isObject = byte == UInt8(ascii: "{")
                    stack.append(Container(isObject: isObject))
                    index += 1
                    expect = isObject ? .keyOrClose : .valueOrClose
                case UInt8(ascii: "\""):
                    var ignored: [UInt8]?
                    index = try string(bytes, from: index, decoded: &ignored)
                    afterValue()
                case UInt8(ascii: "t"):
                    index = try literal("true", in: bytes, at: index)
                    afterValue()
                case UInt8(ascii: "f"):
                    index = try literal("false", in: bytes, at: index)
                    afterValue()
                case UInt8(ascii: "n"):
                    index = try literal("null", in: bytes, at: index)
                    afterValue()
                case UInt8(ascii: "-"), UInt8(ascii: "0")...UInt8(ascii: "9"):
                    index = try number(bytes, from: index)
                    afterValue()
                default:
                    throw Failure.refused(.notJSON)
                }
            case .key, .keyOrClose:
                if byte == UInt8(ascii: "}"), expect == .keyOrClose {
                    stack.removeLast()
                    index += 1
                    afterValue()
                    continue
                }
                guard byte == UInt8(ascii: "\"") else { throw Failure.refused(.notJSON) }
                var key: [UInt8]? = []
                index = try string(bytes, from: index, decoded: &key)
                guard stack[stack.count - 1].keys.insert(key ?? []).inserted else {
                    throw Failure.refused(.duplicateKey)
                }
                expect = .colon
            case .colon:
                guard byte == UInt8(ascii: ":") else { throw Failure.refused(.notJSON) }
                index += 1
                expect = .value
            case .commaOrClose:
                let isObject = stack[stack.count - 1].isObject
                if byte == UInt8(ascii: ",") {
                    index += 1
                    expect = isObject ? .key : .value
                } else if byte == UInt8(ascii: isObject ? "}" : "]") {
                    stack.removeLast()
                    index += 1
                    afterValue()
                } else {
                    throw Failure.refused(.notJSON)
                }
            case .end:
                throw Failure.refused(.notJSON)
            }
        }
    }

    private static func isSpace(_ byte: UInt8) -> Bool {
        byte == 0x20 || byte == 0x09 || byte == 0x0A || byte == 0x0D
    }

    private static func literal(_ word: String, in bytes: [UInt8], at index: Int) throws -> Int {
        let expected = Array(word.utf8)
        guard index + expected.count <= bytes.count else { throw Failure.refused(.notJSON) }
        for (offset, byte) in expected.enumerated() where bytes[index + offset] != byte {
            throw Failure.refused(.notJSON)
        }
        return index + expected.count
    }

    private static func isDigit(_ byte: UInt8) -> Bool {
        byte >= UInt8(ascii: "0") && byte <= UInt8(ascii: "9")
    }

    /// `-? (0 | [1-9][0-9]*) (. [0-9]+)? ([eE] [+-]? [0-9]+)?`
    private static func number(_ bytes: [UInt8], from start: Int) throws -> Int {
        var index = start
        let count = bytes.count
        if bytes[index] == UInt8(ascii: "-") { index += 1 }
        guard index < count, isDigit(bytes[index]) else { throw Failure.refused(.notJSON) }
        if bytes[index] == UInt8(ascii: "0") {
            index += 1
        } else {
            while index < count, isDigit(bytes[index]) { index += 1 }
        }
        if index < count, bytes[index] == UInt8(ascii: ".") {
            index += 1
            guard index < count, isDigit(bytes[index]) else { throw Failure.refused(.notJSON) }
            while index < count, isDigit(bytes[index]) { index += 1 }
        }
        if index < count, bytes[index] == UInt8(ascii: "e") || bytes[index] == UInt8(ascii: "E") {
            index += 1
            if index < count, bytes[index] == UInt8(ascii: "+") || bytes[index] == UInt8(ascii: "-") { index += 1 }
            guard index < count, isDigit(bytes[index]) else { throw Failure.refused(.notJSON) }
            while index < count, isDigit(bytes[index]) { index += 1 }
        }
        // A number a 64-bit float can't hold (`1e400`, a 400-digit integer) is notJSON here.
        guard let value = Double(String(decoding: bytes[start..<index], as: UTF8.self)), value.isFinite else {
            throw Failure.refused(.notJSON)
        }
        return index
    }

    private static func hex(_ bytes: [UInt8], at index: Int) throws -> UInt32 {
        guard index + 4 <= bytes.count else { throw Failure.refused(.notJSON) }
        var value: UInt32 = 0
        for byte in bytes[index..<(index + 4)] {
            let digit: UInt32
            switch byte {
            case UInt8(ascii: "0")...UInt8(ascii: "9"): digit = UInt32(byte - UInt8(ascii: "0"))
            case UInt8(ascii: "a")...UInt8(ascii: "f"): digit = UInt32(byte - UInt8(ascii: "a") + 10)
            case UInt8(ascii: "A")...UInt8(ascii: "F"): digit = UInt32(byte - UInt8(ascii: "A") + 10)
            default: throw Failure.refused(.notJSON)
            }
            value = value << 4 | digit
        }
        return value
    }

    /// Walks a string from its opening quote and returns the index after its closing one. With
    /// `decoded`, it also collects the string's UTF-8 with escapes decoded (object keys).
    private static func string(_ bytes: [UInt8], from start: Int, decoded: inout [UInt8]?) throws -> Int {
        var index = start + 1
        let count = bytes.count
        while index < count {
            let byte = bytes[index]
            switch byte {
            case UInt8(ascii: "\""):
                return index + 1
            case 0x00...0x1F:
                throw Failure.refused(.notJSON)
            case UInt8(ascii: "\\"):
                guard index + 1 < count else { throw Failure.refused(.notJSON) }
                let escape = bytes[index + 1]
                index += 2
                let simple: UInt8? = switch escape {
                case UInt8(ascii: "\""): UInt8(ascii: "\"")
                case UInt8(ascii: "\\"): UInt8(ascii: "\\")
                case UInt8(ascii: "/"): UInt8(ascii: "/")
                case UInt8(ascii: "b"): 0x08
                case UInt8(ascii: "f"): 0x0C
                case UInt8(ascii: "n"): 0x0A
                case UInt8(ascii: "r"): 0x0D
                case UInt8(ascii: "t"): 0x09
                default: nil
                }
                if let simple {
                    decoded?.append(simple)
                    continue
                }
                guard escape == UInt8(ascii: "u") else { throw Failure.refused(.notJSON) }
                var value = try hex(bytes, at: index)
                index += 4
                switch value {
                case 0xD800...0xDBFF:
                    guard index + 1 < count, bytes[index] == UInt8(ascii: "\\"), bytes[index + 1] == UInt8(ascii: "u") else {
                        throw Failure.refused(.notJSON)
                    }
                    let low = try hex(bytes, at: index + 2)
                    guard (0xDC00...0xDFFF).contains(low) else { throw Failure.refused(.notJSON) }
                    index += 6
                    value = 0x10000 + ((value - 0xD800) << 10) + (low - 0xDC00)
                case 0xDC00...0xDFFF:
                    throw Failure.refused(.notJSON)
                default:
                    break
                }
                if decoded != nil, let scalar = Unicode.Scalar(value) {
                    decoded?.append(contentsOf: Array(String(scalar).utf8))
                }
            default:
                decoded?.append(byte)
                index += 1
            }
        }
        throw Failure.refused(.notJSON)
    }
}
