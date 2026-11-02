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
    func insertText(_ text: String)
    func deleteBackward()
    func adjustTextPosition(byCharacterOffset offset: Int)
}

extension KeyboardTextTarget {
    var autocorrectionType: UITextAutocorrectionType? { .default }
    var enablesReturnKeyAutomatically: Bool? { false }
    func adjustTextPosition(byCharacterOffset offset: Int) {}
}

@MainActor
final class DocumentProxyTextTarget: KeyboardTextTarget {
    private let proxy: () -> UITextDocumentProxy

    init(_ proxy: @escaping () -> UITextDocumentProxy) {
        self.proxy = proxy
    }

    var documentContextBeforeInput: String? { proxy().documentContextBeforeInput }
    var hasText: Bool { proxy().hasText }
    var autocapitalizationType: UITextAutocapitalizationType? { proxy().autocapitalizationType }
    var autocorrectionType: UITextAutocorrectionType? { proxy().autocorrectionType }
    var keyboardType: UIKeyboardType? { proxy().keyboardType }
    var returnKeyType: UIReturnKeyType? { proxy().returnKeyType }
    var enablesReturnKeyAutomatically: Bool? { proxy().enablesReturnKeyAutomatically }
    var keyboardAppearance: UIKeyboardAppearance? { proxy().keyboardAppearance }
    func insertText(_ text: String) { proxy().insertText(text) }
    func deleteBackward() { proxy().deleteBackward() }
    func adjustTextPosition(byCharacterOffset offset: Int) { proxy().adjustTextPosition(byCharacterOffset: offset) }
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

    init(target: KeyboardTextTarget, autocorrect: AutocorrectEngine? = nil) {
        self.target = target
        self.autocorrect = autocorrect ?? AutocorrectEngine()
        rebuildLayout()
    }

    var isUppercase: Bool { layerState.layer == .letters && shift.isUppercase }

    var isTrackpadActive: Bool {
        touches.values.contains(where: \.isTrackpad)
    }

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
        rebuildLayout()
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
        if shift != before { onChange?() }
    }

    func refreshSuggestions() {
        let next = autocorrect.suggestions(
            beforeInput: target.documentContextBeforeInput,
            field: field,
            autocorrectionType: target.autocorrectionType
        )
        if next != suggestions {
            suggestions = next
            onChange?()
        }
    }

    func acceptSuggestion(_ suggestion: KeyboardSuggestion) {
        if let token = AutocorrectEngine.currentToken(in: target.documentContextBeforeInput) {
            for _ in 0..<token.count {
                target.deleteBackward()
            }
            if suggestion.kind == .keepLiteral {
                autocorrect.suppressAndKeep(token)
            }
        }
        autocorrect.clearPendingUndo()
        insert(suggestion.replacement + " ")
    }

    // MARK: - Touches

    func touchBegan(id: Int, at point: CGPoint) {
        guard let key = layout.hitTest(point) else { return }
        // Rollover: a new key going down commits any character or accent picker still held.
        for other in touchOrder {
            guard var touch = touches[other], touch.key.character != nil, !touch.committed else { continue }
            if activeAccentTouchID == other, let menu = activeAccentMenu {
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
        switch key.kind {
        case .shift:
            shift.shiftDown(now: clock())
        case .delete:
            deleteBackward(allowAutocorrectUndo: true)
        case .layerSwitch(let next):
            touch.returnLayer = layerState.layer
            layerState.switchTo(next)
            if next == .letters { shift.reset() }
            rebuildLayout()
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
                onSelectionStep?()
                onChange?()
                return
            }
            if touch.isTrackpad {
                let delta = point.x - touch.trackpadAnchorX
                let rawSteps = Int((delta / Self.trackpadPointsPerCharacter).rounded(.towardZero))
                if rawSteps != 0 {
                    touch.trackpadAnchorX += CGFloat(rawSteps) * Self.trackpadPointsPerCharacter
                    touches[id] = touch
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
        case .space:
            if !touch.isTrackpad {
                typeSpace()
            }
        case .returnKey:
            if target.enablesReturnKeyAutomatically == true && !target.hasText {
                break
            }
            autocorrect.clearPendingUndo()
            insert("\n")
        case .layerSwitch:
            // Hold 123, slide to a key, let go: type it and go back.
            if let returnLayer = touch.returnLayer,
               let under = layout.hitTest(point),
               let character = under.character,
               under.kind != touch.key.kind {
                autocorrect.clearPendingUndo()
                insert(character)
                layerState.switchTo(returnLayer)
                rebuildLayout()
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
        if shift.isHeld { shift.shiftUp() }
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
            deleteBackward(allowAutocorrectUndo: false)
            deleteBackward(allowAutocorrectUndo: false)
        } else {
            deleteBackward(allowAutocorrectUndo: false)
        }
    }

    func deleteBackward(allowAutocorrectUndo: Bool = true) {
        if allowAutocorrectUndo,
           let undo = autocorrect.consumePendingUndo(),
           let before = target.documentContextBeforeInput,
           before.hasSuffix(undo.insertedText) {
            for _ in 0..<undo.insertedText.count {
                target.deleteBackward()
            }
            target.insertText(undo.original)
            autocorrect.suppressAndKeep(undo.original)
            lastSpaceAt = nil
            didEdit()
            return
        }
        autocorrect.clearPendingUndo()
        target.deleteBackward()
        lastSpaceAt = nil
        didEdit()
    }

    func deleteWordBackward() {
        autocorrect.clearPendingUndo()
        guard let before = target.documentContextBeforeInput, !before.isEmpty else {
            deleteBackward(allowAutocorrectUndo: false)
            return
        }
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
        for _ in 0..<max(1, count) {
            target.deleteBackward()
        }
        lastSpaceAt = nil
        didEdit()
    }

    // MARK: - Typing

    private func commitCharacter(_ key: KeyboardKey) {
        guard let character = key.character else { return }
        let text = layerState.layer == .letters && shift.isUppercase ? character.uppercased() : character
        if layerState.layer == .letters {
            shift.didType()
        }
        autocorrect.clearPendingUndo()
        insert(text)
    }

    private func commitAccentSelection(_ chosen: String) {
        if layerState.layer == .letters {
            shift.didType()
        }
        autocorrect.clearPendingUndo()
        insert(chosen)
    }

    private func typeSpace() {
        let now = clock()
        if DoubleSpacePeriod.shouldReplace(before: target.documentContextBeforeInput, lastSpaceAt: lastSpaceAt, now: now) {
            autocorrect.clearPendingUndo()
            target.deleteBackward()
            target.insertText(". ")
            lastSpaceAt = nil
            layerState.didInsert(" ")
            rebuildLayoutIfNeeded()
            didEdit()
            return
        }
        if let (original, replacement) = autocorrect.replacementOnSpace(
            beforeInput: target.documentContextBeforeInput,
            field: field,
            autocorrectionType: target.autocorrectionType
        ) {
            for _ in 0..<original.count {
                target.deleteBackward()
            }
            let inserted = replacement + " "
            target.insertText(inserted)
            autocorrect.recordAppliedCorrection(original: original, insertedText: inserted)
            lastSpaceAt = now
            layerState.didInsert(" ")
            rebuildLayoutIfNeeded()
            didEdit()
            return
        }
        autocorrect.clearPendingUndo()
        insert(" ")
        lastSpaceAt = now
    }

    private func insert(_ text: String) {
        target.insertText(text)
        lastSpaceAt = nil
        layerState.didInsert(text)
        rebuildLayoutIfNeeded()
        didEdit()
    }

    private func didEdit() {
        refreshAutoCapitalization()
        refreshSuggestions()
        onEdit?()
    }

    // MARK: - Layout

    private func rebuildLayoutIfNeeded() {
        if layout.layer != layerState.layer {
            rebuildLayout()
        }
    }

    private func rebuildLayout() {
        layout = KeyboardLayout(
            width: width,
            orientation: orientation,
            layer: layerState.layer,
            field: field,
            showsGlobe: showsGlobe,
            horizontalInset: horizontalInset
        )
    }
}
