import Foundation

/// Record typing logs' files: `TypingLogs/yyyy-MM-dd.ndjson` in the App Group, one per
/// local day, kept 14 days and at most 64 MB each. Events are encoded and appended on one
/// serial utility queue; every failure (no Full Access, a full disk) drops events silently.
nonisolated final class TypingLogStore: @unchecked Sendable {
    static let folderName = "TypingLogs"
    static let fileExtension = "ndjson"
    static let retentionDays = 14
    static let dailyCap = 64 * 1024 * 1024
    /// Room kept under the cap so the `cap_reached` line always fits.
    static let capReserve = 1024

    static var defaultDirectory: URL? {
        FileManager.default
            .containerURL(forSecurityApplicationGroupIdentifier: KeyboardBridge.appGroup)?
            .appendingPathComponent(folderName, isDirectory: true)
    }

    /// File names are Gregorian dates in the iPhone's time zone, whatever its calendar.
    static var localCalendar: Calendar {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = .autoupdatingCurrent
        return calendar
    }

    let directory: URL?
    let cap: Int
    private let calendar: Calendar
    private let queue = DispatchQueue(label: "anomaly.hex.typing-log.writer", qos: .utility)

    private struct OpenFile {
        let day: String
        let url: URL
        let handle: FileHandle
        let start: TimeInterval
        let end: TimeInterval
        var bytes: Int
        var capped: Bool
        /// Detail keys (layout hashes) already written in full to this file.
        var details: Set<String> = []
        var pending = Data()
    }

    /// Touched only on `queue`.
    private var file: OpenFile?

    init(directory: URL? = TypingLogStore.defaultDirectory, cap: Int = TypingLogStore.dailyCap, calendar: Calendar = TypingLogStore.localCalendar) {
        self.directory = directory
        self.cap = cap
        self.calendar = calendar
    }

    // MARK: - Writing

    /// Encodes and appends on the writer queue; returns at once.
    func append(_ events: [TypingLogEvent]) {
        guard !events.isEmpty, directory != nil else { return }
        queue.async { self.write(events) }
    }

    /// Deletes day files older than `retentionDays` (today counts as one).
    func prune(now: Date = Date()) {
        queue.async { Self.prune(in: self.directory, now: now, calendar: self.calendar) }
    }

    /// Closes the open file once everything queued before has been written.
    func close() {
        queue.async { self.closeFile() }
    }

    /// Blocks until everything queued so far is on disk (tests).
    func waitUntilWritten() {
        queue.sync {}
    }

    private func write(_ events: [TypingLogEvent]) {
        if let open = file, !FileManager.default.fileExists(atPath: open.url.path) {
            // Delete all ran in the app: start a new file, with layouts in full again.
            try? open.handle.close()
            file = nil
        }
        for event in events {
            let seconds = event.wallSeconds
            if file == nil || !(file!.start..<file!.end).contains(seconds) {
                writePending()
                closeFile()
                file = open(forDayOf: seconds)
            }
            guard var current = file, !current.capped else { continue }
            let includeDetail = event.detail.map { !current.details.contains($0.key) } ?? true
            let line = Data(TypingLogEncoder.line(event, includeDetail: includeDetail).utf8)
            if current.bytes + line.count > cap - Self.capReserve {
                let note = TypingLogEvent(
                    type: "cap_reached",
                    wallMS: event.wallMS,
                    monoMS: event.monoMS,
                    session: event.session,
                    seq: event.seq,
                    fields: [TypingLogField("bytes", .int(current.bytes)), TypingLogField("cap", .int(cap))]
                )
                let capLine = Data(TypingLogEncoder.line(note).utf8)
                current.pending.append(capLine)
                current.bytes += capLine.count
                current.capped = true
                file = current
                continue
            }
            if includeDetail, let key = event.detail?.key {
                current.details.insert(key)
            }
            current.pending.append(line)
            current.bytes += line.count
            file = current
        }
        writePending()
    }

    private func writePending() {
        guard var current = file, !current.pending.isEmpty else { return }
        do {
            try current.handle.write(contentsOf: current.pending)
            current.pending = Data()
            file = current
        } catch {
            try? current.handle.close()
            file = nil
        }
    }

    private func closeFile() {
        writePending()
        if let current = file {
            try? current.handle.close()
        }
        file = nil
    }

    private func open(forDayOf seconds: TimeInterval) -> OpenFile? {
        guard let directory else { return nil }
        let date = Date(timeIntervalSince1970: seconds)
        guard let interval = calendar.dateInterval(of: .day, for: date) else { return nil }
        let day = Self.dayName(for: date, calendar: calendar)
        let url = directory.appendingPathComponent("\(day).\(Self.fileExtension)")
        let manager = FileManager.default
        do {
            try manager.createDirectory(at: directory, withIntermediateDirectories: true)
            if !manager.fileExists(atPath: url.path) {
                guard manager.createFile(atPath: url.path, contents: nil) else { return nil }
            }
            let handle = try FileHandle(forWritingTo: url)
            let bytes = Int(try handle.seekToEnd())
            return OpenFile(
                day: day,
                url: url,
                handle: handle,
                start: interval.start.timeIntervalSince1970,
                end: interval.end.timeIntervalSince1970,
                bytes: bytes,
                capped: bytes > cap - Self.capReserve || (bytes > cap / 2 && Self.endsCapped(url))
            )
        } catch {
            return nil
        }
    }

    /// Whether a file already holds its `cap_reached` line (another process wrote it).
    private static func endsCapped(_ url: URL) -> Bool {
        guard let handle = try? FileHandle(forReadingFrom: url) else { return false }
        defer { try? handle.close() }
        guard let end = try? handle.seekToEnd(), end > 0 else { return false }
        try? handle.seek(toOffset: end > 2048 ? end - 2048 : 0)
        guard let tail = try? handle.readToEnd() else { return false }
        return tail.range(of: Data("\"type\":\"cap_reached\"".utf8)) != nil
    }

    // MARK: - Days

    static func dayName(for date: Date, calendar: Calendar = TypingLogStore.localCalendar) -> String {
        let parts = calendar.dateComponents([.year, .month, .day], from: date)
        return String(format: "%04d-%02d-%02d", parts.year ?? 0, parts.month ?? 0, parts.day ?? 0)
    }

    /// The day a file name stands for, or nil for anything else in the folder.
    static func day(fromFileName name: String) -> String? {
        let suffix = ".\(fileExtension)"
        guard name.hasSuffix(suffix) else { return nil }
        let day = String(name.dropLast(suffix.count))
        let digits = day.unicodeScalars.filter { CharacterSet.decimalDigits.contains($0) }.count
        guard day.count == 10, digits == 8, day.dropFirst(4).first == "-", day.dropFirst(7).first == "-" else {
            return nil
        }
        return day
    }

    static func prune(in directory: URL?, now: Date = Date(), calendar: Calendar = TypingLogStore.localCalendar) {
        guard let directory,
              let oldestKept = calendar.date(byAdding: .day, value: -(retentionDays - 1), to: now),
              let names = try? FileManager.default.contentsOfDirectory(atPath: directory.path)
        else { return }
        let cutoff = dayName(for: oldestKept, calendar: calendar)
        for name in names {
            guard let day = day(fromFileName: name), day < cutoff else { continue }
            try? FileManager.default.removeItem(at: directory.appendingPathComponent(name))
        }
    }

    // MARK: - Reading (the app)

    static func days(in directory: URL? = defaultDirectory) -> [TypingLogDay] {
        guard let directory,
              let names = try? FileManager.default.contentsOfDirectory(atPath: directory.path)
        else { return [] }
        return names.compactMap { name -> TypingLogDay? in
            guard let day = day(fromFileName: name) else { return nil }
            let url = directory.appendingPathComponent(name)
            let attributes = try? FileManager.default.attributesOfItem(atPath: url.path)
            let bytes = (attributes?[.size] as? NSNumber)?.intValue ?? 0
            let modified = attributes?[.modificationDate] as? Date ?? .distantPast
            return TypingLogDay(day: day, url: url, bytes: bytes, modified: modified)
        }
        .sorted { $0.day > $1.day }
    }

    /// Lines in a day file, counted in 1 MB chunks.
    static func countEvents(in url: URL) -> Int {
        guard let handle = try? FileHandle(forReadingFrom: url) else { return 0 }
        defer { try? handle.close() }
        var count = 0
        while let chunk = try? handle.read(upToCount: 1 << 20), !chunk.isEmpty {
            chunk.withUnsafeBytes { buffer in
                guard var cursor = buffer.baseAddress else { return }
                var remaining = buffer.count
                while remaining > 0, let hit = memchr(cursor, 0x0A, remaining) {
                    count += 1
                    let step = UnsafeRawPointer(hit) - cursor + 1
                    cursor += step
                    remaining -= step
                }
            }
        }
        return count
    }

    static func deleteAll(in directory: URL? = defaultDirectory) {
        for day in days(in: directory) {
            try? FileManager.default.removeItem(at: day.url)
        }
    }
}

nonisolated struct TypingLogDay: Identifiable, Equatable, Sendable {
    let day: String
    let url: URL
    let bytes: Int
    let modified: Date

    var id: String { day }

    var date: Date? {
        let parts = day.split(separator: "-").compactMap { Int($0) }
        guard parts.count == 3 else { return nil }
        return TypingLogStore.localCalendar.date(from: DateComponents(year: parts[0], month: parts[1], day: parts[2]))
    }
}
