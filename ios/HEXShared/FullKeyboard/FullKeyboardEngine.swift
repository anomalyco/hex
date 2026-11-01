import QuartzCore
import UIKit

/// The text field the keys type into. `UITextDocumentProxy` in the keyboard; a fake in tests.
@MainActor
protocol KeyboardTextTarget: AnyObject {
    var documentContextBeforeInput: String? { get }
    var hasText: Bool { get }
    var autocapitalizationType: UITextAutocapitalizationType? { get }
    var keyboardType: UIKeyboardType? { get }
    var returnKeyType: UIReturnKeyType? { get }
    var keyboardAppearance: UIKeyboardAppearance? { get }
    func insertText(_ text: String)
    func deleteBackward()
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
    var keyboardType: UIKeyboardType? { proxy().keyboardType }
    var returnKeyType: UIReturnKeyType? { proxy().returnKeyType }
    var keyboardAppearance: UIKeyboardAppearance? { proxy().keyboardAppearance }
    func insertText(_ text: String) { proxy().insertText(text) }
    func deleteBackward() { proxy().deleteBackward() }
}

/// Everything the keys do, apart from drawing: multitouch with rollover, shift, pages,
/// double-space period, auto-capitalization and delete. Touches are identified by any
/// stable integer so tests can drive it without `UITouch`.
@MainActor
final class FullKeyboardEngine {
    struct ActiveTouch: Equatable {
        var key: KeyboardKey
        var point: CGPoint
        /// A character that already went in because another key went down after it.
        var committed: Bool
        /// The page a 123/ABC press came from; releasing over a character returns there.
        var returnLayer: KeyboardLayer?
    }

    private(set) var shift = ShiftStateMachine()
    private(set) var layerState = KeyboardLayerState()
    private(set) var field: KeyboardFieldKind = .text
    private(set) var touches: [Int: ActiveTouch] = [:]
    /// Touch ids in the order they went down.
    private var touchOrder: [Int] = []
    private var lastSpaceAt: TimeInterval?

    let target: KeyboardTextTarget
    var width: CGFloat = 375 { didSet { if width != oldValue { rebuildLayout() } } }
    var orientation: KeyboardOrientation = .portrait { didSet { if orientation != oldValue { rebuildLayout() } } }
    var showsGlobe = false { didSet { if showsGlobe != oldValue { rebuildLayout() } } }
    var horizontalInset: CGFloat = 0 { didSet { if horizontalInset != oldValue { rebuildLayout() } } }
    private(set) var layout = KeyboardLayout(width: 375)

    var clock: () -> TimeInterval = { CACurrentMediaTime() }
    /// Called whenever something visible changed (page, shift, pressed keys).
    var onChange: (() -> Void)?
    /// Called after the keys typed or deleted anything.
    var onEdit: (() -> Void)?

    init(target: KeyboardTextTarget) {
        self.target = target
        rebuildLayout()
    }

    var isUppercase: Bool { layerState.layer == .letters && shift.isUppercase }

    /// The key whose balloon shows: the newest character still held.
    var previewKey: KeyboardKey? {
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
        cancelAllTouches()
        rebuildLayout()
        refreshAutoCapitalization()
    }

    /// Call after the text or the cursor changed outside these keys.
    func textDidChange() {
        lastSpaceAt = nil
        refreshAutoCapitalization()
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

    // MARK: - Touches

    func touchBegan(id: Int, at point: CGPoint) {
        guard let key = layout.hitTest(point) else { return }
        // Rollover: a new key going down commits any character still held.
        for other in touchOrder {
            guard var touch = touches[other], touch.key.character != nil, !touch.committed else { continue }
            commitCharacter(touch.key)
            touch.committed = true
            touches[other] = touch
        }

        var touch = ActiveTouch(key: key, point: point, committed: false, returnLayer: nil)
        switch key.kind {
        case .shift:
            shift.shiftDown(now: clock())
        case .delete:
            deleteBackward()
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

    func touchMoved(id: Int, to point: CGPoint) {
        guard var touch = touches[id] else { return }
        touch.point = point
        if touch.key.character != nil, !touch.committed,
           let under = layout.hitTest(point), under.character != nil, under.kind != touch.key.kind {
            touch.key = under
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
        switch touch.key.kind {
        case .character:
            if !touch.committed {
                commitCharacter(touch.key)
            }
        case .shift:
            shift.shiftUp()
        case .space:
            typeSpace()
        case .returnKey:
            insert("\n")
        case .layerSwitch:
            // Hold 123, slide to a key, let go: type it and go back.
            if let returnLayer = touch.returnLayer,
               let under = layout.hitTest(point),
               let character = under.character,
               under.kind != touch.key.kind {
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
        guard touches.removeValue(forKey: id) != nil else { return }
        touchOrder.removeAll { $0 == id }
        onChange?()
    }

    func cancelAllTouches() {
        let hadTouches = !touches.isEmpty
        touches.removeAll()
        touchOrder.removeAll()
        if shift.isHeld { shift.shiftUp() }
        if hadTouches { onChange?() }
    }

    // MARK: - Delete

    /// The delete key repeats while held; after `wordDeleteAfter` it removes whole words.
    static let repeatDelay: TimeInterval = 0.36
    static let repeatInterval: TimeInterval = 0.075
    static let wordDeleteAfter: TimeInterval = 1.4

    func repeatDelete(heldFor: TimeInterval) {
        if heldFor >= Self.wordDeleteAfter {
            deleteWordBackward()
        } else {
            deleteBackward()
        }
    }

    func deleteBackward() {
        target.deleteBackward()
        lastSpaceAt = nil
        didEdit()
    }

    func deleteWordBackward() {
        guard let before = target.documentContextBeforeInput, !before.isEmpty else {
            deleteBackward()
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
        insert(text)
    }

    private func typeSpace() {
        let now = clock()
        if DoubleSpacePeriod.shouldReplace(before: target.documentContextBeforeInput, lastSpaceAt: lastSpaceAt, now: now) {
            target.deleteBackward()
            target.insertText(". ")
            lastSpaceAt = nil
            layerState.didInsert(" ")
            rebuildLayoutIfNeeded()
            didEdit()
            return
        }
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
