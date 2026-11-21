import QuartzCore
import UIKit

/// Settings › Keyboard › Emoji in the full keyboard: the emoji key, its panel, the emoji search
/// (the letter keys type into it while the results sit above), and an emoji in the empty
/// strip for the word just finished ("pizza " offers 🍕).
///
/// The index (`EmojiLibrary`, a memory-mapped file and about 0.7 MB of offsets) loads on
/// the first emoji key press or finished word and stays while the keyboard runs, apart
/// from memory warnings; the panel, its grid and the search go when the panel closes.
extension FullKeyboardView {
    var emojiEnabled: Bool {
        emojiOverride ?? FullKeyboardSettings().emoji
    }

    /// Emoji in text fields that take any text: not web addresses, email addresses or
    /// ASCII-only fields.
    func refreshEmojiKey() {
        let document = routedTarget.document
        let takesEmoji: Bool = {
            guard case .keys(.text, _) = KeyboardFieldRouting.route(for: document.keyboardType) else { return false }
            return document.keyboardType != .asciiCapable
        }()
        let shows = emojiEnabled && takesEmoji
        guard engine.showsEmoji != shows else { return }
        engine.showsEmoji = shows
        plane.setNeedsDisplay()
        setNeedsLayout()
    }

    var isShowingEmoji: Bool { emojiPanel != nil }

    func loadEmojiLibrary() -> EmojiLibrary? {
        let language = engine.language.rawValue
        if let library = emojiLibrary, library.language == language || !EmojiLibrary.languages.contains(language) {
            return library
        }
        emojiLibrary = EmojiLibrary.load(language: language, bundle: Bundle(for: FullKeyboardView.self))
        return emojiLibrary
    }

    // MARK: - Panel

    func emojiKeyTapped() {
        if routedTarget.isSearching {
            endEmojiSearch()
        } else {
            openEmojiPanel()
        }
    }

    func openEmojiPanel() {
        guard emojiPanel == nil, !showsRecording else { return }
        let started = CACurrentMediaTime()
        guard let library = loadEmojiLibrary() else { return }
        typingLog?.ui("emoji_open")
        dismissStyleMenu()
        plane.cancelAllTouches()
        let panel = EmojiPanelView(library: library, usage: makeEmojiUsage())
        panel.palette = palette
        panel.showsKeyPopups = engine.showsKeyPopups
        panel.onKeyDown = { [weak self] in self?.keyFeedback() }
        panel.onSelectionStep = { [weak self] in self?.selectionFeedback() }
        panel.onInsert = { [weak self] text, entry, source in self?.insertEmoji(text, entry: entry, source: source) }
        panel.onDelete = { [weak self] in self?.engine.deleteBackward(source: "emoji") }
        panel.onDeleteRepeat = { [weak self] held in self?.engine.repeatDelete(heldFor: held) }
        panel.onLetters = { [weak self] in
            self?.typingLog?.ui("emoji_close")
            self?.closeEmojiPanel()
        }
        panel.onSearch = { [weak self] in self?.beginEmojiSearch() }
        insertSubview(panel, belowSubview: callout)
        emojiPanel = panel
        for view in [plane, stripView, hexKey, globeButton] as [UIView] { view.isHidden = true }
        callout.show(nil, label: "")
        accentCallout.isHidden = true
        fillEmojiPanel()
        setNeedsLayout()
        layoutIfNeeded()
        lastEmojiPanelLatency = CACurrentMediaTime() - started
    }

    /// The row's suggestions for the text before the cursor and your use, and the grid's
    /// Frequently used.
    func fillEmojiPanel() {
        guard let panel = emojiPanel else { return }
        let library = panel.library
        var usage: [Int: Float] = [:]
        var frequent: [Int] = []
        for (emoji, score) in panel.usage.ranked(limit: 32) {
            guard let entry = library.entryByEmoji[emoji], usage[entry] == nil else { continue }
            usage[entry] = score
            frequent.append(entry)
        }
        for emoji in EmojiSuggester.defaults where frequent.count < 16 {
            if let entry = library.entryByEmoji[emoji], !frequent.contains(entry) { frequent.append(entry) }
        }
        let context = EmojiContext(before: routedTarget.document.documentContextBeforeInput, stopwords: library.stopwords)
        let suggestions = EmojiSuggester.suggestions(context: context, library: library, usage: usage, limit: 10)
        panel.show(suggestions: suggestions, frequent: frequent)
    }

    func closeEmojiPanel() {
        guard let panel = emojiPanel else { return }
        if routedTarget.isSearching { endEmojiSearch() }
        panel.tearDown()
        panel.removeFromSuperview()
        emojiPanel = nil
        for view in [plane, stripView, hexKey] as [UIView] {
            view.isHidden = false
            view.alpha = 1
        }
        engine.textDidChange()
        refreshReturnKey()
        refreshStrip()
        setNeedsLayout()
    }

    func insertEmoji(_ text: String, entry: Int, source: String) {
        if routedTarget.isSearching {
            // The keys type into the search; the emoji goes into the field.
            suspendedTypingLog?.edit("insert", [
                TypingLogField("inserted", .string(text)),
                TypingLogField("src", .string("emoji_\(source)")),
            ])
            routedTarget.document.insertText(text)
            onTextEdit?()
        } else {
            engine.insertEmoji(text, source: "emoji_\(source)")
        }
        if let library = emojiPanel?.library ?? emojiLibrary, entry < library.entries.count {
            emojiPanel?.usage.record(library.entries[entry].emoji)
        }
    }

    /// An emoji picked from the strip: counted like one from the panel.
    func recordEmojiUse(_ emoji: String) {
        guard let library = emojiLibrary, let entry = library.entryByEmoji[emoji] else { return }
        makeEmojiUsage().record(library.entries[entry].emoji)
    }

    // MARK: - Search

    func beginEmojiSearch() {
        guard let panel = emojiPanel, !routedTarget.isSearching else { return }
        typingLog?.ui("emoji_search")
        suspendedTypingLog = engine.typingLog
        engine.typingLog = nil
        routedTarget.query = ""
        panel.isHidden = true

        let field = EmojiSearchField()
        field.palette = palette
        field.onClose = { [weak self] in self?.endEmojiSearch() }
        addSubview(field)
        emojiSearchField = field
        let results = EmojiRowView()
        results.renderer = panel.glyphs
        results.palette = palette
        results.fontSize = panel.rowFontSize
        results.emptyText = "No emoji"
        results.onPick = { [weak self] index in
            guard let self, index < self.emojiResultEntries.count, let panel = self.emojiPanel else { return }
            self.keyFeedback()
            let entry = self.emojiResultEntries[index]
            self.insertEmoji(panel.display(for: entry), entry: entry, source: "search")
        }
        addSubview(results)
        emojiResults = results

        plane.isHidden = false
        emojiSearchLift = engine.layout.metrics.stripHeight
        onPreferredHeightChange?()
        engine.configureForField()
        refreshReturnKey()
        refreshEmojiSearch()
        setNeedsLayout()
        layoutIfNeeded()
    }

    /// Typing changed the query: the results follow, your suggestions while it's empty.
    func refreshEmojiSearch() {
        guard let panel = emojiPanel, let query = routedTarget.query else { return }
        emojiSearchField?.query = query
        let library = panel.library
        let ranked = panel.usage.ranked(limit: 32)
        var boost: [Int: Float] = [:]
        for (emoji, score) in ranked {
            if let entry = library.entryByEmoji[emoji] { boost[entry] = score }
        }
        emojiResultEntries = query.trimmingCharacters(in: .whitespaces).isEmpty
            ? panel.suggestions
            : EmojiSuggester.search(query, library: library, usage: boost)
        emojiResults?.setItems(emojiResultEntries.map { (panel.display(for: $0), library.name(of: $0)) })
    }

    func endEmojiSearch() {
        guard routedTarget.isSearching else { return }
        routedTarget.query = nil
        plane.cancelAllTouches()
        engine.typingLog = suspendedTypingLog
        suspendedTypingLog = nil
        emojiSearchField?.removeFromSuperview()
        emojiSearchField = nil
        emojiResults?.removeFromSuperview()
        emojiResults = nil
        emojiResultEntries = []
        emojiSearchLift = 0
        onPreferredHeightChange?()
        engine.configureForField()
        refreshReturnKey()
        if let panel = emojiPanel {
            plane.isHidden = true
            panel.isHidden = false
            fillEmojiPanel()
        }
        setNeedsLayout()
    }

    func layoutEmoji(content: CGRect, lift: CGFloat) {
        let layout = engine.layout
        let m = layout.metrics
        if let panel = emojiPanel {
            panel.frame = routedTarget.isSearching ? content : bounds
            panel.metrics = m
            panel.horizontalInset = layout.horizontalInset
        }
        let inset = layout.horizontalInset
        emojiSearchField?.frame = CGRect(x: inset, y: m.stripTop + lift, width: bounds.width - 2 * inset, height: m.stripHeight)
        emojiResults?.frame = CGRect(x: inset + m.side, y: 0, width: bounds.width - 2 * (inset + m.side), height: lift)
    }

    // MARK: - Strip

    /// Up to three emoji for the word just finished, when the strip would be empty.
    func emojiForFinishedWord() -> [KeyboardSuggestion] {
        guard engine.showsEmoji, emojiPanel == nil, !routedTarget.isSearching,
              engine.target.autocorrectionType != .no else { return [] }
        let before = routedTarget.document.documentContextBeforeInput
        guard let before, before.hasSuffix(" "), !before.hasSuffix("  ") else { return [] }
        guard let library = loadEmojiLibrary() else { return [] }
        let usage = makeEmojiUsage()
        return EmojiSuggester.forFinishedWord(before, library: library).map { entry in
            let item = library.entries[entry]
            let tone = item.hasTones ? usage.tone(for: item.emoji) : 0
            let emoji = tone > 0 ? item.tones[tone - 1] : item.emoji
            return KeyboardSuggestion(displayText: emoji, replacement: emoji, kind: .emoji)
        }
    }

    // MARK: - Fixtures

    /// Fixtures: open the panel, optionally searching `query`.
    func debugOpenEmoji(search query: String? = nil) {
        openEmojiPanel()
        guard let query else { return }
        beginEmojiSearch()
        for character in query {
            routedTarget.insertText(String(character))
        }
        refreshEmojiSearch()
    }
}
