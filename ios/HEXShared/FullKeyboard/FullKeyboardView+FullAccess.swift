import UIKit

/// The strip's Full Access hints in the full keyboard: the Hex key tapped without Allow Full
/// Access says what dictation needs and where to turn it on, and a paste iOS asked about or
/// refused says where to stop it asking. Each is a strip notice for a few seconds, and only
/// the hint's own chip opens Hex.
extension FullKeyboardView {

    // MARK: - Without Full Access

    /// The Hex key without Full Access: the keys keep typing, and the strip says, for a few
    /// seconds, that dictation needs Allow Full Access and where to turn it on. Only its
    /// Open Hex chip opens Hex; nothing opens it by itself.
    func showFullAccessHint() {
        dismissStyleMenu()
        showsFullAccessHint = true
        refreshStrip()
        UIAccessibility.post(
            notification: .announcement,
            argument: "\(FullAccessHintView.title). \(FullAccessHintView.spokenPath)."
        )
        fullAccessHintHideWork?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.hideFullAccessHint() }
        fullAccessHintHideWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.fullAccessHintDuration, execute: work)
    }

    func hideFullAccessHint() {
        fullAccessHintHideWork?.cancel()
        fullAccessHintHideWork = nil
        guard showsFullAccessHint else { return }
        showsFullAccessHint = false
        refreshStrip()
    }

    func openSetupFromHint() {
        keyFeedback()
        hideFullAccessHint()
        model.onOpenURL?(KeyboardLaunchRequest.setup.url)
    }

    func pasteFromClipboard() {
        if stripOffer == .dictation {
            keyFeedback()
            typingLog?.ui("paste_dictation")
            model.onPasteDictation?()
            refreshStrip()
            return
        }
        let pasteboard = UIPasteboard.general
        // The read blocks while iOS asks Allow Paste, so its length tells whether it asked.
        let start = ProcessInfo.processInfo.systemUptime
        let read = KeyboardClipboardTracker.read(pasteboard)
        let readDuration = ProcessInfo.processInfo.systemUptime - start
        let text = read.flatMap { $0.isEmpty ? nil : $0 }
        let refused = text == nil && (pasteboard.hasStrings || pasteboard.hasURLs)
        let pointsToSetting = clipboard.shouldPointToAllowPaste(readDuration: readDuration, refused: refused)
        guard let text else {
            if pointsToSetting { showAllowPasteHint() }
            return
        }
        keyFeedback()
        clipboard.markPasted()
        if !pointsToSetting && readDuration < KeyboardClipboardTracker.askedReadDuration {
            clipboard.recordSilentPaste(pasteboard: pasteboard)
        }
        typingLog?.edit("paste", [TypingLogField("inserted", .string(text))])
        engine.commitHangulComposition()
        engine.forgetTextAroundKeys()
        engine.target.insertText(text)
        engine.textDidChange()
        typingLog?.context("paste")
        if pointsToSetting { showAllowPasteHint() }
        refreshStrip()
        onTextEdit?()
    }

    /// Two-line notice with a Settings chip after iOS asks Allow Paste or refuses a read.
    var allowPasteNotice: KeyboardStripNotice? {
        showsAllowPasteHint ? KeyboardClipboardTracker.allowPasteNotice : nil
    }

    /// For a few seconds after a paste iOS asked about (or refused), the strip offers a
    /// Settings chip pointing straight to Paste from Other Apps › Allow.
    func showAllowPasteHint() {
        showsAllowPasteHint = true
        refreshStrip()
        UIAccessibility.post(notification: .announcement, argument: KeyboardClipboardTracker.allowPasteHintAnnouncement)
        allowPasteHintHideWork?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.hideAllowPasteHint() }
        allowPasteHintHideWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.allowPasteHintDuration, execute: work)
    }

    /// Opens Hex to jump straight to iOS Settings › Apps › Hex.
    func openPasteSettingsFromHint() {
        hideAllowPasteHint()
        model.onOpenURL?(KeyboardLaunchRequest.pasteSettings.url)
    }

    /// Typing takes the strip back before the hint's time is up.
    func hideAllowPasteHint() {
        allowPasteHintHideWork?.cancel()
        allowPasteHintHideWork = nil
        guard showsAllowPasteHint else { return }
        showsAllowPasteHint = false
        refreshStrip()
    }
}
