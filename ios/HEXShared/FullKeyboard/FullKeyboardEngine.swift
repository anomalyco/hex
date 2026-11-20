import QuartzCore
import UIKit

/// The text field the keys type into. `UITextDocumentProxy` in the keyboard; a fake in tests.
@MainActor
protocol KeyboardTextTarget: AnyObject {
    var documentContextBeforeInput: String? { get }
    var hasText: Bool { get }
    var autocapitalizationType: UITextAutocapitalizationType? { get }
    var autocorrectionType: UITextAutocorrectionType? { get }
    var keyboardType: UIKeyboardType? { get }
    var returnKeyType: UIReturnKeyType? { get }
    var enablesReturnKeyAutomatically: Bool? { get }
    var keyboardAppearance: UIKeyboardAppearance? { get }
    var textContentType: UITextContentType? { get }
    func insertText(_ text: String)
    func deleteBackward()
    func adjustTextPosition(byCharacterOffset offset: Int)
}

extension KeyboardTextTarget {
    var autocorrectionType: UITextAutocorrectionType? { .default }
    var enablesReturnKeyAutomatically: Bool? { false }
    var textContentType: UITextContentType? { nil }
    func adjustTextPosition(byCharacterOffset offset: Int) {}
}

@MainActor
final class DocumentProxyTextTarget: KeyboardTextTarget {
    private let proxy: () -> UITextDocumentProxy?

    init(_ proxy: @escaping () -> UITextDocumentProxy?) {
        self.proxy = proxy
    }

    var documentContextBeforeInput: String? { proxy()?.documentContextBeforeInput }
    var hasText: Bool { proxy()?.hasText ?? false }
    var autocapitalizationType: UITextAutocapitalizationType? { proxy()?.autocapitalizationType }
    var autocorrectionType: UITextAutocorrectionType? { proxy()?.autocorrectionType }
    var keyboardType: UIKeyboardType? { proxy()?.keyboardType }
    var returnKeyType: UIReturnKeyType? { proxy()?.returnKeyType }
    var enablesReturnKeyAutomatically: Bool? { proxy()?.enablesReturnKeyAutomatically }
    var keyboardAppearance: UIKeyboardAppearance? { proxy()?.keyboardAppearance }
    var textContentType: UITextContentType? { proxy()?.textContentType ?? nil }
    func insertText(_ text: String) { proxy()?.insertText(text) }
    func deleteBackward() { proxy()?.deleteBackward() }
    func adjustTextPosition(byCharacterOffset offset: Int) { proxy()?.adjustTextPosition(byCharacterOffset: offset) }
}

/// Everything the keys do, apart from drawing: multitouch with rollover, shift, pages,
/// long-press accents, spacebar cursor trackpad, cautious autocorrect, double-space period,
/// auto-capitalization and accelerating delete. Touches are identified by any stable integer
/// so tests can drive it without `UITouch`.
@MainActor
final class FullKeyboardEngine {
    struct ActiveTouch: Equatable {
        var key: KeyboardKey
        var startPoint: CGPoint
        var point: CGPoint
        /// A character that already went in because another key went down after it.
        var committed: Bool
        /// The page a 123/ABC press came from; releasing over a character returns there.
        var returnLayer: KeyboardLayer?
        /// True once a spacebar press crosses `trackpadActivationDistance`.
        var isTrackpad = false
        var trackpadAnchorX: CGFloat = 0
        /// Characters the trackpad has moved the cursor, for Record typing logs.
        var trackpadOffset = 0
    }

    static let accentHoldDelay: TimeInterval = 0.40
    static let accentSlop: CGFloat = 10
    static let trackpadActivationDistance: CGFloat = 10
    static let trackpadPointsPerCharacter: CGFloat = 7.5

    private(set) var shift = ShiftStateMachine()
    private(set) var layerState = KeyboardLayerState()
    private(set) var field: KeyboardFieldKind = .text
    private(set) var touches: [Int: ActiveTouch] = [:]
    /// Touch ids in the order they went down.
    private var touchOrder: [Int] = []
    private var lastSpaceAt: TimeInterval?

    private(set) var activeAccentMenu: AccentMenu?
    private var activeAccentTouchID: Int?
    private(set) var suggestions: [KeyboardSuggestion] = []

    let target: KeyboardTextTarget
    let autocorrect: AutocorrectEngine
    /// Learn how I type (Experiments); nil while off.
    var journal: TypingJournal? {
        didSet { syncLearning() }
    }

    private var learning: TypingJournal? {
        guard let journal, TypingJournal.allows(field: field, target: target) else { return nil }
        return journal
    }
    /// Record typing logs (Experiments); nil while it is off, in fields it skips, and in the
    /// app's fixtures. Every call through it is skipped (arguments too) while nil.
    var typingLog: TypingLogger?
    var width: CGFloat = 375 { didSet { if width != oldValue { rebuildLayout() } } }
    var orientation: KeyboardOrientation = .portrait { didSet { if orientation != oldValue { rebuildLayout() } } }
    var showsGlobe = false { didSet { if showsGlobe != oldValue { rebuildLayout() } } }
    var horizontalInset: CGFloat = 0 { didSet { if horizontalInset != oldValue { rebuildLayout() } } }
    private(set) var layout = KeyboardLayout(width: 375)

    var clock: () -> TimeInterval = { CACurrentMediaTime() }
    /// Called whenever something visible changed (page, shift, pressed keys, suggestions, accents).
    var onChange: (() -> Void)?
    /// Called after the keys typed or deleted anything.
    var onEdit: (() -> Void)?
    /// Called when an accent cell or spacebar trackpad step ticks, for selection haptics.
    var onSelectionStep: (() -> Void)?
    /// Called just before the return key types its line break, so a send can be noticed.
    var onReturn: (() -> Void)?

    init(target: KeyboardTextTarget, autocorrect: AutocorrectEngine? = nil) {
        self.target = target
        self.autocorrect = autocorrect ?? AutocorrectEngine()
        rebuildLayout()
    }

    var isUppercase: Bool { layerState.layer == .letters && shift.isUppercase }

    var isTrackpadActive: Bool {
        touches.values.contains(where: \.isTrackpad)
    }

    /// Settings › Key pop-ups. Off: pressed keys only darken, as with iOS's Character Preview off.
    var showsKeyPopups = true

    /// The key whose balloon shows: the newest character still held (hidden while an accent menu is up).
    var previewKey: KeyboardKey? {
        guard activeAccentMenu == nil else { return nil }
        for id in touchOrder.reversed() {
            if let touch = touches[id], touch.key.character != nil, !touch.committed {
                return touch.key
            }
        }
        return nil
    }

    var pressedKinds: Set<KeyKind> {
        Set(touches.values.map(\.key.kind))
    }

    func displayLabel(for key: KeyboardKey) -> String {
        guard let character = key.character else { return key.label }
        return isUppercase && layerState.layer == .letters ? character.uppercased() : character
    }

    // MARK: - Field

    /// Call when the keyboard appears or the field changes.
    func configureForField() {
        switch KeyboardFieldRouting.route(for: target.keyboardType) {
        case .keys(let kind, let initialLayer):
            field = kind
            layerState.reset(to: initialLayer)
        case .handOff:
            field = .text
            layerState.reset(to: .numbers)
        }
        shift.reset()
        lastSpaceAt = nil
        autocorrect.clearPendingUndo()
        cancelAllTouches()
        rebuildLayout(cause: "field")
        refreshAutoCapitalization()
        refreshSuggestions()
    }

    /// Call after the text or the cursor changed outside these keys.
    func textDidChange() {
        lastSpaceAt = nil
        autocorrect.clearPendingUndo()
        refreshAutoCapitalization()
        refreshSuggestions()
    }

    func refreshAutoCapitalization() {
        let before = shift
        shift.refreshAutoCapitalization(
            AutoCapitalization.shouldCapitalize(
                mode: AutoCapitalization.Mode(target.autocapitalizationType),
                before: target.documentContextBeforeInput,
                hasText: target.hasText
            )
        )
        if shift != before {
            logState("auto")
            onChange?()
        }
    }

    func refreshSuggestions() {
        syncLearning()
        let next = autocorrect.suggestions(
            beforeInput: target.documentContextBeforeInput,
            field: field,
            autocorrectionType: target.autocorrectionType
        )
        if next != suggestions {
            suggestions = next
            typingLog?.suggestionsChanged(next)
            onChange?()
        }
    }

    func acceptSuggestion(_ suggestion: KeyboardSuggestion) {
        if let log = typingLog {
            let token = AutocorrectEngine.currentToken(in: target.documentContextBeforeInput) ?? ""
            log.edit("suggestion", [
                TypingLogField("slot", suggestions.firstIndex(of: suggestion).map { .int($0) } ?? .null),
                TypingLogField("text", .string(suggestion.displayText)),
                TypingLogField("deleted", .string(token)),
                TypingLogField("inserted", .string(suggestion.replacement + " ")),
                TypingLogField("candidates", TypingLogger.candidates(suggestions)),
            ])
        }
        if let journal = learning {
            let token = AutocorrectEngine.currentToken(in: target.documentContextBeforeInput)
            if let token, !token.isEmpty, token != suggestion.replacement {
                journal.record(.picked, typed: token, final: suggestion.replacement)
            } else {
                journal.didFinish(word: token)
                // Tapping what you typed over the autocorrection space would apply turns it down.
                if suggestion.kind == .keepLiteral, let token,
                   let autoFix = suggestions.first(where: \.willAutoReplace) {
                    journal.record(.keptTyped, typed: autoFix.replacement, final: token)
                }
            }
        }
        if let token = AutocorrectEngine.currentToken(in: target.documentContextBeforeInput) {
            for _ in 0..<token.count {
                target.deleteBackward()
            }
            if suggestion.kind == .keepLiteral {
                autocorrect.suppressAndKeep(token)
            }
        }
        autocorrect.clearPendingUndo()
        insert(suggestion.replacement + " ", source: nil)
        typingLog?.context("suggestion")
    }

    // MARK: - Touches

    func touchBegan(id: Int, at point: CGPoint) {
        guard let key = layout.hitTest(point) else { return }
        // Rollover: a new key going down commits any character or accent picker still held.
        for other in touchOrder {
            guard var touch = touches[other], touch.key.character != nil, !touch.committed else { continue }
            if activeAccentTouchID == other, let menu = activeAccentMenu {
                typingLog?.ui("accent_pick", [
                    TypingLogField("id", .int(other)),
                    TypingLogField("index", .int(menu.selectedIndex)),
                    TypingLogField("chosen", .string(menu.selectedOption)),
                    TypingLogField("rollover", .bool(true)),
                ])
                commitAccentSelection(menu.selectedOption)
                activeAccentMenu = nil
                activeAccentTouchID = nil
            } else {
                commitCharacter(touch.key)
            }
            touch.committed = true
            touches[other] = touch
        }

        var touch = ActiveTouch(
            key: key,
            startPoint: point,
            point: point,
            committed: false,
            returnLayer: nil
        )
        if key.character != nil {
            learning?.recordTap(on: key, at: point)
        }
        switch key.kind {
        case .shift:
            shift.shiftDown(now: clock())
            logState("shift_key")
        case .delete:
            deleteBackward(allowAutocorrectUndo: true)
        case .layerSwitch(let next):
            touch.returnLayer = layerState.layer
            layerState.switchTo(next)
            if next == .letters { shift.reset() }
            rebuildLayout(cause: "layer_key")
            if next == .letters { refreshAutoCapitalization() }
        case .character, .space, .returnKey, .hex, .globe:
            break
        }
        touches[id] = touch
        touchOrder.append(id)
        onChange?()
    }

    @discardableResult
    func openAccentMenuIfEligible(forTouch id: Int) -> Bool {
        guard activeAccentMenu == nil,
              let touch = touches[id],
              !touch.committed,
              let base = touch.key.character else { return false }
        let dx = touch.point.x - touch.startPoint.x
        let dy = touch.point.y - touch.startPoint.y
        guard (dx * dx + dy * dy).squareRoot() <= Self.accentSlop else { return false }
        guard let options = AccentCatalog.options(for: base, uppercase: isUppercase),
              options.count > 1 else { return false }
        activeAccentMenu = AccentMenu.build(
            for: touch.key.frame,
            rawOptions: options,
            in: layout.width,
            horizontalInset: layout.horizontalInset
        )
        activeAccentTouchID = id
        typingLog?.ui("accent_open", [
            TypingLogField("id", .int(id)),
            TypingLogField("key", .string(TypingLogger.keyID(touch.key.kind))),
            TypingLogField("options", .strings(activeAccentMenu?.options ?? [])),
        ])
        onChange?()
        return true
    }

    func touchMoved(id: Int, to point: CGPoint) {
        guard var touch = touches[id] else { return }
        touch.point = point

        // 1. Active long-press accent picker: horizontal drag selects the alternate character.
        if activeAccentTouchID == id, var menu = activeAccentMenu {
            let nextIndex = menu.index(for: point.x)
            touches[id] = touch
            if nextIndex != menu.selectedIndex {
                menu.selectedIndex = nextIndex
                activeAccentMenu = menu
                onSelectionStep?()
                onChange?()
            }
            return
        }

        // 2. Spacebar cursor trackpad: horizontal drag >= 10 pt slides the text caret.
        if touch.key.kind == .space {
            if !touch.isTrackpad, abs(point.x - touch.startPoint.x) >= Self.trackpadActivationDistance {
                touch.isTrackpad = true
                touch.trackpadAnchorX = point.x
                lastSpaceAt = nil
                autocorrect.clearPendingUndo()
                touches[id] = touch
                typingLog?.ui("trackpad_start", [TypingLogField("id", .int(id))])
                onSelectionStep?()
                onChange?()
                return
            }
            if touch.isTrackpad {
                let delta = point.x - touch.trackpadAnchorX
                let rawSteps = Int((delta / Self.trackpadPointsPerCharacter).rounded(.towardZero))
                if rawSteps != 0 {
                    touch.trackpadAnchorX += CGFloat(rawSteps) * Self.trackpadPointsPerCharacter
                    touch.trackpadOffset += rawSteps
                    touches[id] = touch
                    typingLog?.edit("cursor", [TypingLogField("offset", .int(rawSteps))])
                    target.adjustTextPosition(byCharacterOffset: rawSteps)
                    onSelectionStep?()
                    refreshAutoCapitalization()
                    refreshSuggestions()
                    return
                }
                touches[id] = touch
                return
            }
        }

        // 3. Sliding across character keys updates the active key and re-anchors the hold timer.
        if touch.key.character != nil, !touch.committed,
           let under = layout.hitTest(point), under.character != nil, under.kind != touch.key.kind {
            touch.key = under
            touch.startPoint = point
            touches[id] = touch
            onChange?()
            return
        }
        touches[id] = touch
    }

    func touchEnded(id: Int, at point: CGPoint) {
        guard var touch = touches.removeValue(forKey: id) else { return }
        touchOrder.removeAll { $0 == id }
        touch.point = point

        if activeAccentTouchID == id, let menu = activeAccentMenu {
            let index = menu.index(for: point.x)
            let chosen = menu.options[index]
            activeAccentMenu = nil
            activeAccentTouchID = nil
            if !touch.committed {
                typingLog?.ui("accent_pick", [
                    TypingLogField("id", .int(id)),
                    TypingLogField("index", .int(index)),
                    TypingLogField("chosen", .string(chosen)),
                ])
                commitAccentSelection(chosen)
            }
            onChange?()
            return
        }

        switch touch.key.kind {
        case .character:
            if !touch.committed {
                commitCharacter(touch.key)
            }
        case .shift:
            shift.shiftUp()
            logState("shift_key")
        case .space:
            if touch.isTrackpad {
                typingLog?.ui("trackpad_end", [
                    TypingLogField("id", .int(id)),
                    TypingLogField("offset", .int(touch.trackpadOffset)),
                ])
                typingLog?.context("trackpad")
            } else {
                typeSpace()
            }
        case .returnKey:
            if target.enablesReturnKeyAutomatically == true && !target.hasText {
                typingLog?.edit("return", [
                    TypingLogField("returnKeyType", .string(TypingLogTraits.name(target.returnKeyType ?? .default))),
                    TypingLogField("ignored", .bool(true)),
                ])
                break
            }
            learning?.didFinish(word: AutocorrectEngine.currentToken(in: target.documentContextBeforeInput))
            autocorrect.clearPendingUndo()
            typingLog?.edit("return", [
                TypingLogField("inserted", .string("\n")),
                TypingLogField("returnKeyType", .string(TypingLogTraits.name(target.returnKeyType ?? .default))),
                TypingLogField("send", .bool(target.returnKeyType == .send)),
            ])
            onReturn?()
            insert("\n", source: nil)
            typingLog?.context("return")
        case .layerSwitch:
            // Hold 123, slide to a key, let go: type it and go back.
            if let returnLayer = touch.returnLayer,
               let under = layout.hitTest(point),
               let character = under.character,
               under.kind != touch.key.kind {
                autocorrect.clearPendingUndo()
                insert(character, source: "slide")
                layerState.switchTo(returnLayer)
                rebuildLayout(cause: "layer_slide")
                refreshAutoCapitalization()
            }
        case .delete, .hex, .globe:
            break
        }
        onChange?()
    }

    func touchCancelled(id: Int) {
        if activeAccentTouchID == id {
            activeAccentMenu = nil
            activeAccentTouchID = nil
            typingLog?.ui("accent_cancel", [TypingLogField("id", .int(id))])
        }
        guard touches.removeValue(forKey: id) != nil else { return }
        touchOrder.removeAll { $0 == id }
        onChange?()
    }

    func cancelAllTouches() {
        let hadTouches = !touches.isEmpty || activeAccentMenu != nil
        touches.removeAll()
        touchOrder.removeAll()
        activeAccentMenu = nil
        activeAccentTouchID = nil
        if shift.isHeld {
            shift.shiftUp()
            logState("cancel")
        }
        if hadTouches { onChange?() }
    }

    // MARK: - Delete

    /// The delete key repeats after 360 ms, accelerates at 850 ms, and removes whole words after 1.4 s.
    static let repeatDelay: TimeInterval = 0.36
    static let repeatInterval: TimeInterval = 0.075
    static let fastRepeatAfter: TimeInterval = 0.85
    static let wordDeleteAfter: TimeInterval = 1.4

    func repeatDelete(heldFor: TimeInterval) {
        autocorrect.clearPendingUndo()
        if heldFor >= Self.wordDeleteAfter {
            deleteWordBackward()
        } else if heldFor >= Self.fastRepeatAfter {
            deleteBackward(allowAutocorrectUndo: false, source: "repeat_fast")
            deleteBackward(allowAutocorrectUndo: false, source: "repeat_fast")
        } else {
            deleteBackward(allowAutocorrectUndo: false, source: "repeat")
        }
    }

    func deleteBackward(allowAutocorrectUndo: Bool = true, source: String = "key") {
        if allowAutocorrectUndo,
           let undo = autocorrect.consumePendingUndo(),
           let before = target.documentContextBeforeInput,
           before.hasSuffix(undo.insertedText) {
            typingLog?.edit("autocorrect_undo", [
                TypingLogField("deleted", .string(undo.insertedText)),
                TypingLogField("inserted", .string(undo.original)),
            ])
            for _ in 0..<undo.insertedText.count {
                target.deleteBackward()
            }
            target.insertText(undo.original)
            autocorrect.suppressAndKeep(undo.original)
            learning?.record(
                .undone,
                typed: undo.insertedText.trimmingCharacters(in: .whitespaces),
                final: undo.original
            )
            lastSpaceAt = nil
            didEdit()
            return
        }
        autocorrect.clearPendingUndo()
        learning?.willErase(word: AutocorrectEngine.currentToken(in: target.documentContextBeforeInput))
        if let log = typingLog {
            let deleted = target.documentContextBeforeInput?.last.map { String($0) }
            log.edit("delete", [
                deleted.map { TypingLogField("deleted", .string($0)) } ?? TypingLogField("deletedCount", .int(1)),
                TypingLogField("src", .string(source)),
            ])
        }
        target.deleteBackward()
        lastSpaceAt = nil
        didEdit()
    }

    func deleteWordBackward() {
        autocorrect.clearPendingUndo()
        guard let before = target.documentContextBeforeInput, !before.isEmpty else {
            deleteBackward(allowAutocorrectUndo: false, source: "repeat_word")
            return
        }
        learning?.willErase(word: AutocorrectEngine.currentToken(in: before))
        var count = 0
        var index = before.endIndex
        while index > before.startIndex, before[before.index(before: index)].isWhitespace {
            index = before.index(before: index)
            count += 1
        }
        while index > before.startIndex, !before[before.index(before: index)].isWhitespace {
            index = before.index(before: index)
            count += 1
        }
        typingLog?.edit("delete_word", [
            TypingLogField("deleted", .string(String(before.suffix(max(1, count))))),
            TypingLogField("src", .string("repeat_word")),
        ])
        for _ in 0..<max(1, count) {
            target.deleteBackward()
        }
        lastSpaceAt = nil
        didEdit()
        typingLog?.context("delete_word")
    }

    // MARK: - Typing

    private func commitCharacter(_ key: KeyboardKey) {
        guard let character = key.character else { return }
        let text = layerState.layer == .letters && shift.isUppercase ? character.uppercased() : character
        if layerState.layer == .letters {
            shift.didType()
        }
        autocorrect.clearPendingUndo()
        insert(text, source: "key")
        logState("typed")
    }

    private func commitAccentSelection(_ chosen: String) {
        if layerState.layer == .letters {
            shift.didType()
        }
        autocorrect.clearPendingUndo()
        insert(chosen, source: "accent")
        logState("typed")
    }

    private func typeSpace() {
        let now = clock()
        if DoubleSpacePeriod.shouldReplace(before: target.documentContextBeforeInput, lastSpaceAt: lastSpaceAt, now: now) {
            autocorrect.clearPendingUndo()
            typingLog?.edit("double_space_period", [
                TypingLogField("deleted", .string(" ")),
                TypingLogField("inserted", .string(". ")),
            ])
            target.deleteBackward()
            target.insertText(". ")
            lastSpaceAt = nil
            layerState.didInsert(" ")
            rebuildLayoutIfNeeded()
            didEdit()
            typingLog?.context("space")
            return
        }
        learning?.didFinish(word: AutocorrectEngine.currentToken(in: target.documentContextBeforeInput))
        syncLearning()
        if let (original, replacement) = autocorrect.replacementOnSpace(
            beforeInput: target.documentContextBeforeInput,
            field: field,
            autocorrectionType: target.autocorrectionType
        ) {
            for _ in 0..<original.count {
                target.deleteBackward()
            }
            let inserted = replacement + " "
            typingLog?.edit("autocorrect", [
                TypingLogField("from", .string(original)),
                TypingLogField("to", .string(replacement)),
                TypingLogField("deleted", .string(original)),
                TypingLogField("inserted", .string(inserted)),
            ])
            target.insertText(inserted)
            autocorrect.recordAppliedCorrection(original: original, insertedText: inserted)
            learning?.record(.autocorrected, typed: original, final: replacement)
            lastSpaceAt = now
            layerState.didInsert(" ")
            rebuildLayoutIfNeeded()
            didEdit()
            typingLog?.context("space")
            return
        }
        autocorrect.clearPendingUndo()
        insert(" ", source: "key")
        lastSpaceAt = now
        typingLog?.context("space")
    }

    /// `source` names the edit in typing logs; nil when the caller writes its own line.
    private func insert(_ text: String, source: String?) {
        if let source {
            typingLog?.edit("insert", [
                TypingLogField("inserted", .string(text)),
                TypingLogField("src", .string(source)),
            ])
        }
        target.insertText(text)
        lastSpaceAt = nil
        layerState.didInsert(text)
        rebuildLayoutIfNeeded()
        didEdit()
    }

    /// Hands what the journal learned (typos, rejected corrections) to autocorrect;
    /// nothing while Learn how I type is off.
    private func syncLearning() {
        autocorrect.applyLearning(journal?.typingLearning ?? TypingLearning())
    }

    private func didEdit() {
        refreshAutoCapitalization()
        refreshSuggestions()
        onEdit?()
    }

    // MARK: - Layout

    private func rebuildLayoutIfNeeded() {
        if layout.layer != layerState.layer {
            rebuildLayout(cause: "layer_auto")
        }
    }

    private func rebuildLayout(cause: String = "layout") {
        layout = KeyboardLayout(
            width: width,
            orientation: orientation,
            layer: layerState.layer,
            field: field,
            showsGlobe: showsGlobe,
            horizontalInset: horizontalInset
        )
        logState(cause)
    }

    /// Hands the layout and shift to typing logs, which writes a line when either changed.
    func logState(_ cause: String) {
        typingLog?.keyboardState(layout: layout, shift: shift, cause: cause)
    }
}
