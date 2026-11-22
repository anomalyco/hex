import Foundation
import os

/// When Hex downloads Parakeet V2 for Speech model in keyboard by itself (the app, or the keyboard
/// as it opens): only where the keyboard would use it.
nonisolated enum KeyboardParakeetDownloadPolicy {
    /// Why it doesn't download by itself yet.
    enum Hold: Equatable {
        /// Two-language dictation or the Multilingual model takes the keyboard's dictation.
        case otherEngine
        case notEnglish
        /// The keyboard hasn't opened on this iPhone yet, so its memory limit isn't known.
        case keyboardNotMeasured
        case keyboardTooSmall
        /// Cellular, a personal hotspot, Low Data Mode or no connection.
        case notOnWiFi
        case needsSpace
    }

    /// Free space it waits for: the model and the keyboard's compiled copy, with room to spare.
    static let freeSpaceNeeded: Int64 = 2_000_000_000

    /// Only for a keyboard that would use it: English, the keyboard's own limit measured and
    /// enough (`KeyboardParakeetBudget`), a connection that isn't cellular, a hotspot or Low
    /// Data Mode, and `freeSpaceNeeded` free (unknown space doesn't hold it).
    static func hold(otherEngine: Bool, english: Bool, keyboardLimit: Int?, unmetered: Bool?, freeBytes: Int64?) -> Hold? {
        if otherEngine { return .otherEngine }
        guard english else { return .notEnglish }
        guard let keyboardLimit else { return .keyboardNotMeasured }
        guard keyboardLimit >= KeyboardParakeetBudget.requiredLimit else { return .keyboardTooSmall }
        guard unmetered == true else { return .notOnWiFi }
        if let freeBytes, freeBytes < freeSpaceNeeded { return .needsSpace }
        return nil
    }

    /// The same, from the App Group's settings, as the app and the keyboard read them.
    static func hold(unmetered: Bool?, defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) -> Hold? {
        let languages = LanguageSettings(defaults: defaults)
        let set = languages.dictationLanguage
        let keyboard = languages.keyboardDictationLanguage()
        let multilingual = MultilingualParakeetSettings(defaults: defaults)
        return hold(
            otherEngine: TwoLanguageDictationSettings(defaults: defaults).isEnabled
                || multilingual.language(set: set, keyboard: keyboard) != nil,
            english: SpeechLanguagePicker.isEnglish(set ?? keyboard ?? Locale.preferredLanguages.first),
            keyboardLimit: KeyboardParakeetSettings(defaults: defaults).keyboardLimit,
            unmetered: unmetered,
            freeBytes: freeBytes()
        )
    }

    static func freeBytes() -> Int64? {
        let home = URL(fileURLWithPath: NSHomeDirectory(), isDirectory: true)
        return try? home.resourceValues(forKeys: [.volumeAvailableCapacityForImportantUsageKey])
            .volumeAvailableCapacityForImportantUsage
    }
}

/// Who downloads Parakeet V2 into the App Group now, so the app and the keyboard never both do
/// (`SpeechModelBackgroundDownload`, `ParakeetTranscriber.downloadModel`). The app's download
/// runs while Hex is in front and says so every few seconds; the keyboard's runs in iOS's own
/// process, without the keyboard, until it ends or its tasks time out.
nonisolated struct SpeechModelDownloadClaim {
    enum Owner: String {
        case app
        case keyboard
    }

    static let ownerKey = "speechModel.parakeetV2.download.owner"
    static let startedAtKey = "speechModel.parakeetV2.download.startedAt"
    static let refreshedAtKey = "speechModel.parakeetV2.download.refreshedAt"
    static let bytesKey = "speechModel.parakeetV2.download.bytes"
    static let endedAtKey = "speechModel.parakeetV2.download.endedAt"

    /// The keyboard's tasks time out after this (`timeoutIntervalForResource`), so its claim does too.
    static let keyboardLife: TimeInterval = 24 * 60 * 60
    /// The app's download says so more often than this while it runs.
    static let appLife: TimeInterval = 30
    /// Bytes said more recently than this are shown as a bar.
    static let freshProgress: TimeInterval = 10

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    var owner: Owner? {
        defaults?.string(forKey: Self.ownerKey).flatMap(Owner.init(rawValue:))
    }

    private func date(_ key: String) -> Date? {
        (defaults?.object(forKey: key) as? Double).map { Date(timeIntervalSince1970: $0) }
    }

    /// Started and not ended.
    var isOpen: Bool { owner != nil && date(Self.endedAtKey) == nil }

    func isLive(_ owner: Owner, now: Date = Date()) -> Bool {
        guard self.owner == owner, isOpen else { return false }
        switch owner {
        case .keyboard:
            guard let started = date(Self.startedAtKey) else { return false }
            return now.timeIntervalSince(started) < Self.keyboardLife
        case .app:
            guard let refreshed = date(Self.refreshedAtKey) else { return false }
            return now.timeIntervalSince(refreshed) < Self.appLife
        }
    }

    func claim(_ owner: Owner, now: Date = Date()) {
        defaults?.set(owner.rawValue, forKey: Self.ownerKey)
        defaults?.set(now.timeIntervalSince1970, forKey: Self.startedAtKey)
        defaults?.set(now.timeIntervalSince1970, forKey: Self.refreshedAtKey)
        defaults?.set(0, forKey: Self.bytesKey)
        defaults?.removeObject(forKey: Self.endedAtKey)
    }

    func refresh(_ owner: Owner, bytes: Int64, now: Date = Date()) {
        guard self.owner == owner else { return }
        defaults?.set(now.timeIntervalSince1970, forKey: Self.refreshedAtKey)
        defaults?.set(bytes, forKey: Self.bytesKey)
    }

    /// Ends `owner`'s claim; another's stays.
    func end(_ owner: Owner, now: Date = Date()) {
        guard self.owner == owner, isOpen else { return }
        defaults?.set(now.timeIntervalSince1970, forKey: Self.endedAtKey)
    }

    /// The bytes downloaded, when said in the last `freshProgress` seconds.
    func freshBytes(now: Date = Date()) -> Int64? {
        guard isOpen, let refreshed = date(Self.refreshedAtKey),
              now.timeIntervalSince(refreshed) < Self.freshProgress else { return nil }
        return (defaults?.object(forKey: Self.bytesKey) as? NSNumber)?.int64Value
    }
}

/// Speech model in keyboard's download started by the keyboard itself as it opens, so the model
/// arrives without anyone opening Hex: a background `URLSession` (Wi-Fi only) whose transfers
/// run in iOS's own process, cost the keyboard no memory and go on after it closes. Each file
/// lands in the App Group's V2 folder as `.download`, gets its name only once its size and
/// SHA-256 match (`ParakeetV2Files`), and the folder is marked complete once all 21 have. A task
/// that fails keeps its resume data (`.resume`) for the next try, which the next opening of the
/// keyboard starts. When the keyboard isn't running as a transfer ends, iOS wakes Hex, which
/// takes the files in the same way (`handleEvents`).
nonisolated final class SpeechModelBackgroundDownload: NSObject, URLSessionDownloadDelegate, @unchecked Sendable {
    static let identifier = "anomaly.hex.mobile.keyboard.speech-model"
    static let shared = SpeechModelBackgroundDownload()

    private static let log = Logger(subsystem: "anomaly.hex.mobile", category: "speech-model-download")

    private let lock = NSLock()
    private var session: URLSession?
    private var eventsFinished: (@Sendable () -> Void)?
    private var progressSaidAt = Date.distantPast
    private let queue: OperationQueue = {
        let queue = OperationQueue()
        queue.maxConcurrentOperationCount = 1
        queue.qualityOfService = .utility
        return queue
    }()

    private var directory: URL { SpeechModelStore.parakeetV2 }

    private var activeSession: URLSession {
        lock.withLock {
            if let session { return session }
            let configuration = URLSessionConfiguration.background(withIdentifier: Self.identifier)
            configuration.sharedContainerIdentifier = KeyboardBridge.appGroup
            configuration.isDiscretionary = false
            configuration.sessionSendsLaunchEvents = true
            configuration.allowsCellularAccess = false
            configuration.allowsExpensiveNetworkAccess = false
            configuration.allowsConstrainedNetworkAccess = false
            configuration.timeoutIntervalForResource = SpeechModelDownloadClaim.keyboardLife
            let session = URLSession(configuration: configuration, delegate: self, delegateQueue: queue)
            self.session = session
            return session
        }
    }

    /// The keyboard, after its first frame and launch work, off the main thread: starts the
    /// download where Hex would start it by itself, picks up the one it started (a file iOS
    /// dropped gets a new task), or stops it once the switch is off.
    static func keyboardOpened() {
        DispatchQueue.global(qos: .utility).async { shared.keyboardOpenedNow() }
    }

    func keyboardOpenedNow(now: Date = Date()) {
        let claim = SpeechModelDownloadClaim()
        let settings = KeyboardParakeetSettings()
        guard settings.isEnabled, KeyboardParakeetSettings.fits(), !SpeechModelStore.isComplete(directory) else {
            if claim.owner == .keyboard, claim.isOpen {
                Self.log.notice("keyboard: download no longer wanted, cancelling")
                activeSession.getAllTasks { $0.forEach { $0.cancel() } }
                claim.end(.keyboard, now: now)
            }
            return
        }
        if claim.isLive(.app, now: now) {
            Self.log.notice("keyboard: Hex is downloading the model itself")
            return
        }
        if claim.isLive(.keyboard, now: now) {
            startMissingFiles()
            return
        }
        if let hold = KeyboardParakeetDownloadPolicy.hold(unmetered: true) {
            Self.log.notice("keyboard: no download (\(String(describing: hold), privacy: .public))")
            return
        }
        Self.log.notice("keyboard: starting the speech model's download")
        claim.claim(.keyboard, now: now)
        startMissingFiles()
    }

    /// Hex, woken by iOS for the keyboard's download (the keyboard wasn't running as a transfer
    /// ended): takes the finished files in as the keyboard would, then lets iOS suspend it.
    func handleEvents() async {
        await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
            let once = OnceFlag()
            let finish: @Sendable () -> Void = { if once.claim() { continuation.resume() } }
            lock.withLock { eventsFinished = finish }
            _ = activeSession
            DispatchQueue.global().asyncAfter(deadline: .now() + 25, execute: finish)
        }
    }

    /// A task for each file that hasn't got its name and has none running: from its resume data
    /// when an earlier task left some.
    private func startMissingFiles() {
        let session = activeSession
        let directory = directory
        session.getAllTasks { tasks in
            let running = Set(tasks.filter { $0.state == .running || $0.state == .suspended }.compactMap(\.taskDescription))
            var started = 0
            for file in ParakeetV2Files.files where !running.contains(file.path) {
                let destination = directory.appendingPathComponent(file.path)
                guard PinnedModelFile.size(of: destination) != file.size else { continue }
                let resume = destination.appendingPathExtension("resume")
                let task: URLSessionDownloadTask
                if let data = try? Data(contentsOf: resume) {
                    task = session.downloadTask(withResumeData: data)
                    try? FileManager.default.removeItem(at: resume)
                } else {
                    task = session.downloadTask(with: ParakeetV2Files.url(for: file))
                }
                task.taskDescription = file.path
                task.countOfBytesClientExpectsToReceive = file.size
                task.resume()
                started += 1
            }
            Self.log.notice("download: \(started, privacy: .public) new tasks, \(running.count, privacy: .public) running")
        }
    }

    /// Moves a finished transfer in beside its file as `.download` and gives it the file's name
    /// once its size and SHA-256 match; false (and nothing left) otherwise.
    static func place(_ location: URL, as file: PinnedModelFile, in directory: URL) -> Bool {
        let fileManager = FileManager.default
        let destination = directory.appendingPathComponent(file.path)
        let incoming = destination.appendingPathExtension("download")
        SpeechModelStore.prepareRoot()
        try? fileManager.createDirectory(at: destination.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? fileManager.removeItem(at: incoming)
        do {
            try fileManager.moveItem(at: location, to: incoming)
        } catch {
            return false
        }
        guard file.matches(incoming) else {
            try? fileManager.removeItem(at: incoming)
            return false
        }
        try? fileManager.removeItem(at: destination)
        do {
            try fileManager.moveItem(at: incoming, to: destination)
        } catch {
            try? fileManager.removeItem(at: incoming)
            return false
        }
        for suffix in ["partial", "partial.etag", "resume"] {
            try? fileManager.removeItem(at: destination.appendingPathExtension(suffix))
        }
        return true
    }

    // MARK: - URLSessionDownloadDelegate

    func urlSession(
        _ session: URLSession,
        downloadTask: URLSessionDownloadTask,
        didWriteData bytesWritten: Int64,
        totalBytesWritten: Int64,
        totalBytesExpectedToWrite: Int64
    ) {
        let now = Date()
        let due: Bool = lock.withLock {
            guard now.timeIntervalSince(progressSaidAt) >= 1 else { return false }
            progressSaidAt = now
            return true
        }
        guard due else { return }
        let directory = directory
        session.getAllTasks { tasks in
            let inFlight = tasks.compactMap { $0 as? URLSessionDownloadTask }.reduce(Int64(0)) { $0 + $1.countOfBytesReceived }
            SpeechModelDownloadClaim().refresh(.keyboard, bytes: ParakeetV2Files.placedBytes(in: directory) + inFlight)
        }
    }

    func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask, didFinishDownloadingTo location: URL) {
        guard let path = downloadTask.taskDescription,
              let file = ParakeetV2Files.files.first(where: { $0.path == path }) else { return }
        let status = (downloadTask.response as? HTTPURLResponse)?.statusCode ?? 0
        guard status == 200 else {
            Self.log.error("download: \(path, privacy: .public) answered \(status, privacy: .public)")
            return
        }
        // Turned off meanwhile: the download was deleted and nothing comes back.
        guard KeyboardParakeetSettings().isEnabled else { return }
        guard Self.place(location, as: file, in: directory) else {
            Self.log.error("download: \(path, privacy: .public) arrived damaged")
            return
        }
        guard ParakeetV2Files.isDownloaded(in: directory), !SpeechModelStore.isComplete(directory) else { return }
        SpeechModelStore.markComplete(directory)
        ParakeetV2Files.removeLeftovers(in: directory)
        SpeechModelDownloadClaim().end(.keyboard)
        Self.log.notice("download: the speech model is complete")
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        if let error {
            let ns = error as NSError
            if let data = ns.userInfo[NSURLSessionDownloadTaskResumeData] as? Data, let path = task.taskDescription,
               KeyboardParakeetSettings().isEnabled {
                let resume = directory.appendingPathComponent(path).appendingPathExtension("resume")
                SpeechModelStore.prepareRoot()
                try? FileManager.default.createDirectory(at: resume.deletingLastPathComponent(), withIntermediateDirectories: true)
                try? data.write(to: resume, options: .atomic)
            }
            Self.log.error("download: \(task.taskDescription ?? "?", privacy: .public) stopped: \(ns.domain, privacy: .public) \(ns.code, privacy: .public)")
        }
        // The last task ended with files still missing: this download is over, and the next
        // opening of the keyboard (or Hex, by itself) starts another from what's there.
        let directory = directory
        session.getAllTasks { tasks in
            let left = tasks.filter { $0.state == .running || $0.state == .suspended }
            guard left.isEmpty, !SpeechModelStore.isComplete(directory) else { return }
            SpeechModelDownloadClaim().end(.keyboard)
            Self.log.notice("download: ended with files missing")
        }
    }

    func urlSessionDidFinishEvents(forBackgroundURLSession session: URLSession) {
        let finish = lock.withLock { () -> (@Sendable () -> Void)? in
            defer { eventsFinished = nil }
            return eventsFinished
        }
        finish?()
    }
}

/// True the first time only, from any thread.
private nonisolated final class OnceFlag: @unchecked Sendable {
    private let lock = NSLock()
    private var done = false

    func claim() -> Bool {
        lock.withLock {
            guard !done else { return false }
            done = true
            return true
        }
    }
}
