import CoreGraphics
import Foundation

/// Record typing logs (Experiments): one JSON object per line. The schema is in
/// `ios/docs/typing-logs.md`; bump `version` when a field changes meaning.
nonisolated enum TypingLogSchema {
    static let version = 1
}

/// A JSON value in a typing log line. Keys keep the order they were added in.
nonisolated indirect enum TypingLogValue: Equatable, Sendable {
    case int(Int)
    /// A number written with at most `places` decimals, trailing zeros dropped.
    case number(Double, places: Int)
    case string(String)
    case bool(Bool)
    case null
    case array([TypingLogValue])
    case object([TypingLogField])

    /// Points: two decimals is finer than any touch iOS reports.
    static func pt(_ value: CGFloat) -> TypingLogValue { .number(Double(value), places: 2) }
    /// Milliseconds, to the microsecond.
    static func ms(_ value: Double) -> TypingLogValue { .number(value, places: 3) }
    /// `[x, y, width, height]` in points.
    static func rect(_ rect: CGRect) -> TypingLogValue {
        .array([.pt(rect.minX), .pt(rect.minY), .pt(rect.width), .pt(rect.height)])
    }
    static func text(_ value: String?) -> TypingLogValue { value.map(TypingLogValue.string) ?? .null }
    static func strings(_ values: [String]) -> TypingLogValue { .array(values.map(TypingLogValue.string)) }
}

nonisolated struct TypingLogField: Equatable, Sendable {
    let key: String
    let value: TypingLogValue

    init(_ key: String, _ value: TypingLogValue) {
        self.key = key
        self.value = value
    }
}

/// One line. `detail` holds fields written only the first time `detail.key` appears in a
/// day's file (a layout's key list); later lines with the same key say `"repeat": true`.
/// It is a closure so the main thread never builds it: it runs on the writer's queue.
nonisolated struct TypingLogEvent: Sendable {
    struct Detail: Sendable {
        let key: String
        let fields: @Sendable () -> [TypingLogField]
    }

    let type: String
    /// Wall clock, ms since 1970.
    let wallMS: Int64
    /// Monotonic ms (`CACurrentMediaTime`, the clock `UITouch.timestamp` uses).
    let monoMS: Double
    let session: String
    let seq: Int
    var fields: [TypingLogField]
    var detail: Detail?

    init(
        type: String,
        wallMS: Int64,
        monoMS: Double,
        session: String,
        seq: Int,
        fields: [TypingLogField] = [],
        detail: Detail? = nil
    ) {
        self.type = type
        self.wallMS = wallMS
        self.monoMS = monoMS
        self.session = session
        self.seq = seq
        self.fields = fields
        self.detail = detail
    }

    var wallSeconds: TimeInterval { TimeInterval(wallMS) / 1000 }
}

/// Writes events as NDJSON: `{"v":1,"t":…,"mt":…,"type":…,"session":…,"seq":…, fields…}\n`.
nonisolated enum TypingLogEncoder {
    /// One line, newline included. `includeDetail` writes the event's detail fields, else
    /// `"repeat": true` when it has a detail.
    static func line(_ event: TypingLogEvent, includeDetail: Bool = true) -> String {
        var out = String()
        out.reserveCapacity(160)
        out += "{\"v\":"
        out += String(TypingLogSchema.version)
        out += ",\"t\":"
        out += String(event.wallMS)
        out += ",\"mt\":"
        appendNumber(event.monoMS, places: 3, to: &out)
        out += ",\"type\":"
        appendString(event.type, to: &out)
        out += ",\"session\":"
        appendString(event.session, to: &out)
        out += ",\"seq\":"
        out += String(event.seq)
        for field in event.fields {
            appendField(field, to: &out)
        }
        if let detail = event.detail {
            if includeDetail {
                for field in detail.fields() {
                    appendField(field, to: &out)
                }
            } else {
                appendField(TypingLogField("repeat", .bool(true)), to: &out)
            }
        }
        out += "}\n"
        return out
    }

    private static func appendField(_ field: TypingLogField, to out: inout String) {
        out += ","
        appendString(field.key, to: &out)
        out += ":"
        appendValue(field.value, to: &out)
    }

    static func appendValue(_ value: TypingLogValue, to out: inout String) {
        switch value {
        case .int(let int):
            out += String(int)
        case .number(let number, let places):
            appendNumber(number, places: places, to: &out)
        case .string(let string):
            appendString(string, to: &out)
        case .bool(let bool):
            out += bool ? "true" : "false"
        case .null:
            out += "null"
        case .array(let values):
            out += "["
            for (index, item) in values.enumerated() {
                if index > 0 { out += "," }
                appendValue(item, to: &out)
            }
            out += "]"
        case .object(let fields):
            out += "{"
            for (index, field) in fields.enumerated() {
                if index > 0 { out += "," }
                appendString(field.key, to: &out)
                out += ":"
                appendValue(field.value, to: &out)
            }
            out += "}"
        }
    }

    /// Fixed-point without exponents; NaN and infinity become `null`.
    static func appendNumber(_ value: Double, places: Int, to out: inout String) {
        guard value.isFinite else {
            out += "null"
            return
        }
        let places = max(0, min(6, places))
        var scale: Int64 = 1
        for _ in 0..<places { scale *= 10 }
        let scaled = (value * Double(scale)).rounded()
        guard abs(scaled) < 9e15 else {
            out += String(value)
            return
        }
        var units = Int64(scaled)
        if units < 0 {
            out += "-"
            units = -units
        }
        out += String(units / scale)
        var fraction = units % scale
        guard fraction != 0 else { return }
        var digits = places
        while fraction % 10 == 0 {
            fraction /= 10
            digits -= 1
        }
        let text = String(fraction)
        out += "."
        if text.count < digits {
            out += String(repeating: "0", count: digits - text.count)
        }
        out += text
    }

    static func appendString(_ string: String, to out: inout String) {
        out += "\""
        var plainStart = string.unicodeScalars.startIndex
        let scalars = string.unicodeScalars
        var index = scalars.startIndex
        while index < scalars.endIndex {
            let scalar = scalars[index]
            let escape: String? = switch scalar {
            case "\"": "\\\""
            case "\\": "\\\\"
            case "\n": "\\n"
            case "\r": "\\r"
            case "\t": "\\t"
            default: scalar.value < 0x20 ? String(format: "\\u%04x", scalar.value) : nil
            }
            if let escape {
                out.unicodeScalars.append(contentsOf: scalars[plainStart..<index])
                out += escape
                plainStart = scalars.index(after: index)
            }
            index = scalars.index(after: index)
        }
        out.unicodeScalars.append(contentsOf: scalars[plainStart..<scalars.endIndex])
        out += "\""
    }
}
