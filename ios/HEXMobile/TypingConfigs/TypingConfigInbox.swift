import Foundation
import Observation
import SwiftUI
import UIKit

/// A file that arrived to be opened as a typing config: from Files, a share sheet's Open in
/// Hex, AirDrop or Mail. Untrusted until the shared reader has read it.
struct TypingConfigArrival: Identifiable, Equatable {
    enum Content: Equatable {
        case file(Data)
        /// Over `TypingConfigLimits.maxBytes`: never read.
        case tooLarge(bytes: Int)
        case unreadable
    }

    let id = UUID()
    /// The file's name without its extension, cleaned for display.
    let name: String
    let content: Content

    var data: Data? {
        if case .file(let data) = content { data } else { nil }
    }
}

/// Typing configs arriving (`.onOpenURL` with a `.mhexconfig`): each file is read once, at most
/// 64 KB, and waits in line for its sheet. Nothing is applied or kept until the person chooses on
/// the sheet: opening a file never changes typing.
@MainActor
@Observable
final class TypingConfigInbox {
    static let shared = TypingConfigInbox()
    nonisolated static let fileExtension = "mhexconfig"
    /// How much of a file's name the sheet and the history show.
    nonisolated static let nameLimit = 60

    /// The sheet shows the first; the rest wait their turn.
    private(set) var queue: [TypingConfigArrival] = []
    /// The sheet stepped aside for Settings › Experiments (Typing configs off): the files stay
    /// in line and their sheet comes back when the switch turns on.
    private(set) var isParked = false
    /// Settings › Experiments, when the sheet's Open Experiments asked for it.
    var opensExperiments = false

    var current: TypingConfigArrival? { queue.first }
    var waiting: Int { max(0, queue.count - 1) }

    nonisolated static func handles(_ url: URL) -> Bool {
        url.isFileURL && url.pathExtension.lowercased() == fileExtension
    }

    /// Reads `url` off the main thread and puts it in line. The same bytes already in line
    /// aren't added twice.
    func receive(_ url: URL) {
        Task {
            let arrival = await Task.detached(priority: .userInitiated) { Self.read(url) }.value
            if let data = arrival.data, queue.contains(where: { $0.data == data }) { return }
            queue.append(arrival)
            isParked = false
            TypingConfigSheetPresenter.update(for: self)
        }
    }

    /// The sheet's file is done with (applied, kept for later or closed): the next one shows.
    func finishCurrent() {
        guard !queue.isEmpty else { return }
        queue.removeFirst()
        TypingConfigSheetPresenter.update(for: self)
    }

    /// Open Experiments: the sheet steps aside and Settings › Experiments opens at the switch.
    func parkForExperiments() {
        isParked = true
        opensExperiments = true
        TypingConfigSheetPresenter.update(for: self)
    }

    /// Experiments' switch turned on with files still in line: their sheet shows again.
    func experimentTurnedOn() {
        guard isParked else { return }
        isParked = false
        TypingConfigSheetPresenter.update(for: self)
    }

    func closeAll() {
        queue.removeAll()
        isParked = false
        TypingConfigSheetPresenter.update(for: self)
    }

    /// The file's bytes, or why there are none, never reading more than the limit allows. A
    /// copy iOS put in Documents/Inbox (Open in Hex, AirDrop, Mail) is deleted once read.
    nonisolated static func read(_ url: URL, limit: Int = TypingConfigLimits.maxBytes) -> TypingConfigArrival {
        let name = displayName(url.deletingPathExtension().lastPathComponent)
        let scoped = url.startAccessingSecurityScopedResource()
        defer {
            if scoped { url.stopAccessingSecurityScopedResource() }
            if url.path.contains("/Documents/Inbox/") { try? FileManager.default.removeItem(at: url) }
        }
        var content = TypingConfigArrival.Content.unreadable
        var coordinationError: NSError?
        NSFileCoordinator().coordinate(readingItemAt: url, options: .withoutChanges, error: &coordinationError) { readURL in
            content = readContent(readURL, limit: limit)
        }
        return TypingConfigArrival(name: name, content: content)
    }

    nonisolated private static func readContent(_ url: URL, limit: Int) -> TypingConfigArrival.Content {
        if let size = try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize, size > limit {
            return .tooLarge(bytes: size)
        }
        guard let handle = try? FileHandle(forReadingFrom: url) else { return .unreadable }
        defer { try? handle.close() }
        // A size the file provider didn't report, or got wrong: read one byte past the limit.
        guard let data = try? handle.read(upToCount: limit + 1) else { return .unreadable }
        if data.count > limit {
            let size = (try? handle.seekToEnd()).map(Int.init) ?? data.count
            return .tooLarge(bytes: size)
        }
        return data.isEmpty ? .unreadable : .file(data)
    }

    nonisolated private static func safe(_ raw: String, keepsNewlines: Bool) -> String {
        let scalars = raw.unicodeScalars.filter { scalar in
            (keepsNewlines && scalar == "\n") || (!CharacterSet.controlCharacters.contains(scalar)
                && !(0x202A...0x202E).contains(scalar.value)
                && !(0x2066...0x2069).contains(scalar.value)
                && scalar.value != 0x200E && scalar.value != 0x200F)
        }
        return String(String.UnicodeScalarView(scalars)).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// A name safe to show: no control or direction-changing characters, at most `nameLimit`.
    nonisolated static func displayName(_ raw: String) -> String {
        let cleaned = safe(raw, keepsNewlines: false)
        return cleaned.count > nameLimit ? String(cleaned.prefix(nameLimit - 1)) + "…" : cleaned
    }
}

/// Shows the typing config sheet over whatever is on screen (Home, a Settings page, the setup
/// sheet or Typing logs), so a file that arrives is never lost behind another sheet.
@MainActor
enum TypingConfigSheetPresenter {
    private static weak var controller: UIViewController?
    /// The sheet's content height, so a short message sits in a short sheet as the setup
    /// sheet does; the review grows to the full height and scrolls.
    private static var contentHeight: CGFloat = 320

    static func contentHeightChanged(_ height: CGFloat) {
        guard abs(height - contentHeight) > 0.5 else { return }
        contentHeight = height
        guard let sheet = controller?.sheetPresentationController else { return }
        sheet.animateChanges { sheet.invalidateDetents() }
    }

    static func update(for inbox: TypingConfigInbox) {
        if inbox.current == nil || inbox.isParked {
            controller?.dismiss(animated: true)
            controller = nil
            return
        }
        guard controller == nil else { return }
        let scene = UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .first { $0.activationState == .foregroundActive } ?? UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first
        guard var top = scene?.keyWindow?.rootViewController else {
            // Opened cold: the window comes a moment later.
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { update(for: inbox) }
            return
        }
        while let presented = top.presentedViewController, !presented.isBeingDismissed {
            top = presented
        }
        let host = UIHostingController(rootView: TypingConfigSheet(inbox: inbox))
        host.view.backgroundColor = UIColor(HexTheme.bgDeep)
        host.overrideUserInterfaceStyle = .dark
        // It stays until it's closed: a swipe down doesn't drop the file.
        host.isModalInPresentation = true
        if let sheet = host.sheetPresentationController {
            sheet.detents = [.custom(identifier: .init("content")) { context in
                min(contentHeight, context.maximumDetentValue)
            }]
            sheet.prefersGrabberVisible = false
        }
        controller = host
        top.present(host, animated: true)
    }
}
