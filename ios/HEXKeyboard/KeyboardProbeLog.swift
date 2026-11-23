import Foundation
import os

final class KeyboardProbeLog: @unchecked Sendable {
    static let shared = KeyboardProbeLog()

    private let logger = Logger(subsystem: "anomaly.hex.mobile.keyboard", category: "capture-probe")
    private let lock = NSLock()
    private let url = FileManager.default
        .containerURL(forSecurityApplicationGroupIdentifier: KeyboardBridge.appGroup)?
        .appendingPathComponent("keyboard-capture-probe.log")

    /// Past this size the log keeps only its newest `trimmedBytes`, so it never grows
    /// without limit in the App Group.
    static let maximumBytes: UInt64 = 512 * 1024
    static let trimmedBytes = 256 * 1024

    func write(_ message: String) {
        logger.notice("\(message, privacy: .public)")
        guard let url else { return }
        let line = "\(ISO8601DateFormatter().string(from: Date())) \(message)\n"
        lock.withLock {
            if let handle = try? FileHandle(forWritingTo: url) {
                let size = handle.seekToEndOfFile()
                handle.write(Data(line.utf8))
                try? handle.close()
                if size + UInt64(line.utf8.count) > Self.maximumBytes {
                    Self.trim(url)
                }
            } else {
                try? Data(line.utf8).write(to: url)
            }
        }
    }

    /// Keeps the newest `trimmedBytes`, from the first whole line in them.
    static func trim(_ url: URL, keeping bytes: Int = trimmedBytes) {
        guard let data = try? Data(contentsOf: url), data.count > bytes else { return }
        var tail = data.suffix(bytes)
        if let newline = tail.firstIndex(of: UInt8(ascii: "\n")) {
            tail = tail[tail.index(after: newline)...]
        }
        try? Data(tail).write(to: url, options: .atomic)
    }

    static func memory() -> String {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
        let result = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
            }
        }
        let footprint = result == KERN_SUCCESS ? Double(info.phys_footprint) / 1_048_576 : -1
        // The highest footprint this keyboard process has reached, for what iOS's limit saw.
        let peak = result == KERN_SUCCESS ? Double(info.ledger_phys_footprint_peak) / 1_048_576 : -1
        let available = Double(os_proc_available_memory()) / 1_048_576
        return String(format: "[footprint %.1f MB, peak %.1f MB, headroom %.1f MB]", footprint, peak, available)
    }
}
