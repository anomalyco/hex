import OSLog
import SwiftUI
import UIKit

/// Files other apps hand to Hex: AirDrop, Files and Open in Hex arrive here as a copy iOS makes
/// in Hex's own container (`LSSupportsOpeningDocumentsInPlace` is NO): `Documents/Inbox`, or
/// `tmp/<bundle id>-Inbox` on iOS 27. Today that is only Hex words
/// files; another kind (a typing config, #2529) would add its own `Item` case and sheet.
///
/// Each file is copied to a private temporary folder under a name Hex picks, read there, shown
/// in a sheet over whatever is on screen, and both copies are deleted when the sheet closes:
/// after Add, Cancel or a refusal. Nothing applies without the sheet's Add, and only counts and
/// reasons are logged, never what a file holds.
@MainActor
enum HexInbox {
    enum Item {
        /// nil: the file couldn't be opened at all.
        case words(HexWordsReadResult?)
    }

    private static let log = Logger(subsystem: "anomaly.hex.mobile", category: "inbox")
    private static let folderName = "HexInbox"

    private static var privateFolder: URL {
        FileManager.default.temporaryDirectory.appendingPathComponent(folderName, isDirectory: true)
    }

    /// Where iOS puts the copies it hands Hex.
    private static var inboxFolders: [URL] {
        var folders: [URL] = []
        if let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first {
            folders.append(documents.appendingPathComponent("Inbox", isDirectory: true))
        }
        if let bundle = Bundle.main.bundleIdentifier {
            folders.append(FileManager.default.temporaryDirectory.appendingPathComponent(bundle + "-Inbox", isDirectory: true))
        }
        return folders
    }

    /// Handles a file URL Hex was asked to open; false for any other URL.
    @discardableResult
    static func open(_ url: URL) -> Bool {
        guard url.isFileURL else { return false }
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }

        var leftovers: [URL] = []
        if isInInbox(url) { leftovers.append(url) }
        let result: HexWordsReadResult?
        if let size = try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize, size > HexWordsReader.maxBytes {
            result = .refused(.tooLarge)
        } else if let copy = privateCopy(of: url) {
            leftovers.append(copy)
            result = HexWordsReader.read(contentsOf: copy)
        } else {
            result = HexWordsReader.read(contentsOf: url)
        }
        logResult(result)
        // On a cold launch the window may not be up yet when the URL arrives.
        DispatchQueue.main.async {
            present(.words(result)) {
                for file in leftovers { try? FileManager.default.removeItem(at: file) }
            }
        }
        return true
    }

    /// Clears copies a sheet never got to delete (Hex ended while it showed). Inbox copies
    /// are cleared only once they are an hour old, so a file that is arriving now stays.
    static func sweep(now: Date = Date()) {
        try? FileManager.default.removeItem(at: privateFolder)
        for inbox in inboxFolders {
            guard let files = try? FileManager.default.contentsOfDirectory(
                at: inbox, includingPropertiesForKeys: [.contentModificationDateKey]
            ) else { continue }
            for file in files {
                let modified = (try? file.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate
                if let modified, now.timeIntervalSince(modified) < 3600 { continue }
                try? FileManager.default.removeItem(at: file)
            }
        }
    }

    private static func isInInbox(_ url: URL) -> Bool {
        let path = url.standardizedFileURL.resolvingSymlinksInPath().path
        return inboxFolders.contains { path.hasPrefix($0.standardizedFileURL.resolvingSymlinksInPath().path + "/") }
    }

    /// A copy under a name Hex picks: a file's own name is never used as a path.
    private static func privateCopy(of url: URL) -> URL? {
        let folder = privateFolder
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let copy = folder.appendingPathComponent(UUID().uuidString)
        do {
            try FileManager.default.copyItem(at: url, to: copy)
            return copy
        } catch {
            return nil
        }
    }

    private static func logResult(_ result: HexWordsReadResult?) {
        switch result {
        case nil:
            log.notice("words file couldn't be opened")
        case .refused(let reason):
            log.notice("words file refused: \(reason.rawValue, privacy: .public)")
        case .read(let file):
            let reasons = Dictionary(grouping: file.skipped, by: \.reason.rawValue)
                .map { "\($0.key) \($0.value.count)" }.sorted().joined(separator: ", ")
            log.notice("words file read: version \(file.version), \(file.vocabulary.count) terms, \(file.replacements.count) replacements, skipped: \(reasons, privacy: .public)")
        }
    }

    private static func present(_ item: Item, onClose: @escaping () -> Void) {
        let scene = UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .first { $0.activationState == .foregroundActive }
            ?? UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first
        guard var top = scene?.keyWindow?.rootViewController ?? scene?.windows.first?.rootViewController else {
            onClose()
            return
        }
        while let presented = top.presentedViewController, !presented.isBeingDismissed {
            top = presented
        }
        let view: AnyView
        switch item {
        case .words(let result):
            view = AnyView(HexWordsReviewSheet(result: result, onClose: onClose))
        }
        let controller = UIHostingController(rootView: view)
        controller.overrideUserInterfaceStyle = .dark
        top.present(controller, animated: true)
    }
}
