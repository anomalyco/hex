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
        /// True once a spacebar press slides before its hold (`holdSpace`) while it switches
        /// language: letting go far enough along switches instead of typing a space.
        var isLanguageSwipe = false
        var trackpadAnchorX: CGFloat = 0
        /// Characters the trackpad has moved the cursor, for Record typing logs.
        var trackpadOffset = 0
        /// True when the likelier next letter claimed this touch from the key under it; a
        /// small move then keeps it rather than sliding back (`letterReach`).
        var claimed = false
    }

    static let accentHoldDelay: TimeInterval = 0.40
    static let accentSlop: CGFloat = 10
    static let trackpadActivationDistance: CGFloat = 10
    static let trackpadPointsPerCharacter: CGFloat = 7.5
    /// Holding the space bar this long without sliding starts the trackpad, as Apple's does;
    /// letting go then types no space. With two or more languages a slide moves the cursor
    /// only after it; a slide before it is a language swipe.
    static let trackpadHoldDelay: TimeInterval = 0.4
    /// A finger on the strip's lower half this soon after a key went down or came up (or
    /// while one is down) types the row-1 key below it (`takesStripTouch(at:)`).
    static let stripTouchGuard: TimeInterval = 0.25
    /// How far along the space bar a language swipe must go; a shorter one types a space.
    static let languageSwipeDistance: CGFloat = 24

    private(set) var shift = ShiftStateMachine()
    private(set) var layerState = KeyboardLayerState()
    private(set) var field: KeyboardFieldKind = .text
    private(set) var touches: [Int: ActiveTouch] = [:]
    /// Touch ids in the order they went down.
    private var touchOrder: [Int] = []
    private var lastSpaceAt: TimeInterval?
    /// The text before the cursor just after a suggestion typed its word and a space. A space
    /// typed next, while the text is still that, is swallowed, as on Apple's keyboard.
    private var suggestionSpaceBefore: String?
    /// When a key last went down or came up, for `takesStripTouch(at:)`.
    private var lastKeyTouchAt: TimeInterval?

    /// Where each letter of the word being typed was tapped (nil for one that came another
    /// way, such as an accent), for Spatial autocorrect.
    private var tokenTaps: [CGPoint?] = []
    /// Spatial autocorrect: where you tend to tap each letter, from its centre (Learn how I type).
    var tapOffsets: [Character: CGVector] = [:] {
        didSet { if tapOffsets != oldValue { syncDecoderKeys() } }
    }

    /// The layout language's word list, which lets the likelier of two neighbouring letters
    /// claim a few points of the other's touch target, as Apple's keyboard does ("th" then a
    /// tap on the e/d border types e). Used only while autocorrect is on in a text field
    /// that allows it; nil keeps every target fixed.
    var letterOdds: TypingDecoderModel? {
        didSet {
            if letterOdds !== oldValue {
                continuationCache = nil
                wordStartMass = nil
            }
        }
    }
    /// How far past its own target a likelier letter may reach, in points.
    var letterReach: CGFloat = 5
    /// Nats of odds (ln of how many times likelier) a letter needs for each point it claims.
    var letterOddsPerPoint = 0.8
    private var continuationCache: (prefix: [UInt8], mass: [Double]?)?
    /// The first letters' frequencies, the costliest to add up (every word), kept apart.
    private var wordStartMass: [Double]??

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
    /// Experiments › Emoji, in fields that take emoji.
    var showsEmoji = false { didSet { if showsEmoji != oldValue { rebuildLayout() } } }
    var horizontalInset: CGFloat = 0 { didSet { if horizontalInset != oldValue { rebuildLayout() } } }
    /// An iPhone, or the iPhone Duo folded or unfolded (`KeyboardPosture`).
    var posture: KeyboardPosture = .phone { didSet { if posture != oldValue { rebuildLayout() } } }
    /// How far iOS set the keyboard in from each side of the screen (the Duo).
    var windowInset: CGFloat = 0 { didSet { if windowInset != oldValue { rebuildLayout() } } }
    /// The letters, accents and spelling dictionary (Settings › Keyboard › Languages).
    var language: KeyboardLanguage = .english {
        didSet {
            guard language != oldValue else { return }
            cancelAllTouches()
            autocorrect.setLanguage(language)
            rebuildLayout(cause: "language")
            refreshSuggestions()
        }
    }
    /// Bilingual autocorrect's second language on these keys (`KeyboardLanguageList.partner`),
    /// or nil.
    var partnerLanguage: KeyboardLanguage? {
        get { autocorrect.partner?.language }
        set {
            guard newValue != autocorrect.partner?.language else { return }
            autocorrect.setPartner(newValue)
            refreshSuggestions()
        }
    }
    /// Settings › Keyboard › Languages has two or more: a quick swipe along the space bar
    /// switches language (`onLanguageSwipe`) and the cursor slides once the space bar is held
    /// (`trackpadHoldDelay`), as on SwiftKey. With one, any slide moves the cursor at once.
    var switchesLanguage = false
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
    /// Called when the emoji key is let go.
    var onEmojiKey: (() -> Void)?
    /// Called when a swipe along the space bar switches language: 1 for the next (a swipe
    /// to the left), -1 for the one before.
    var onLanguageSwipe: ((Int) -> Void)?

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
        suggestionSpaceBefore = nil
        tokenTaps.removeAll()
        swipedWord = nil
        autocorrect.clearPendingUndo()
        cancelAllTouches()
        rebuildLayout(cause: "field")
        refreshAutoCapitalization()
        refreshSuggestions()
    }

    /// Call after the text or the cursor changed outside these keys.
    func textDidChange() {
        lastSpaceAt = nil
        // Hosts that report the keys' own edits leave the text as the suggestion left it.
        if target.documentContextBeforeInput != suggestionSpaceBefore {
            suggestionSpaceBefore = nil
        }
        tokenTaps.removeAll()
        swipedWord = nil
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
        let next: [KeyboardSuggestion]
        if let swipeSuggestions {
            next = swipeSuggestions
        } else {
            autocorrect.decoderTaps = currentTokenTaps
            next = autocorrect.suggestions(
                beforeInput: target.documentContextBeforeInput,
                field: field,
                autocorrectionType: target.autocorrectionType
            )
        }
        if next != suggestions {
            suggestions = next
            typingLog?.suggestionsChanged(next)
            onChange?()
        }
    }

    func acceptSuggestion(_ suggestion: KeyboardSuggestion) {
        // The strip shows a swipe's readings: mid-swipe they're only shown, after it one
        // takes the swiped word's place.
        if isSwiping { return }
        if suggestion.kind == .swiped {
            replaceSwipedWord(with: suggestion.replacement)
            return
        }
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
        suggestionSpaceBefore = target.documentContextBeforeInput
        typingLog?.context("suggestion")
    }

    /// Types an emoji from the emoji panel: no spacing or autocorrect around it.
    func insertEmoji(_ text: String, source: String) {
        autocorrect.clearPendingUndo()
        learning?.didFinish(word: AutocorrectEngine.currentToken(in: target.documentContextBeforeInput))
        insert(text, source: source)
    }

    // MARK: - Touches

    /// The key a touch going down at `point` presses: the one under it, or a neighbouring
    /// letter within `letterReach` points that is enough likelier to come next in the word.
    func key(at point: CGPoint) -> KeyboardKey? {
        let point = pointOnKeys(point)
        guard let key = layout.hitTest(point) else { return nil }
        guard key.character != nil, let model = letterOdds, layerState.layer == .letters,
              field == .text, autocorrect.isEnabled, target.autocorrectionType != .no,
              shift.state != .locked, model.language == language.rawValue else { return key }
        let reach = letterReach
        let near = layout.keys.filter {
            $0.character != nil && $0.kind != key.kind && Self.distance(from: point, to: $0.hitFrame) < reach
        }
        guard !near.isEmpty, let continuations = continuationMass(model) else { return key }
        // An accented letter without a key of its own is typed from its base letter's (é from e).
        var mass = [Double](repeating: 0, count: continuations.count)
        let keyed = Set(layout.keys.compactMap { key in
            key.character.flatMap { $0.count == 1 ? model.codes[Character($0)] : nil }
        })
        for (code, m) in continuations.enumerated() where m > 0 {
            mass[keyed.contains(UInt8(code)) ? code : Int(model.baseCodes[code])] += m
        }
        let total = mass.reduce(0, +)
        func odds(_ k: KeyboardKey) -> Double? {
            guard let c = k.character, c.count == 1, let code = model.codes[Character(c)] else { return nil }
            return log(mass[Int(code)] + total * 1e-4)
        }
        guard let own = odds(key) else { return key }
        var best = key
        var bestScore = own
        for other in near {
            guard let theirs = odds(other) else { continue }
            let score = theirs - letterOddsPerPoint * Double(Self.distance(from: point, to: other.hitFrame))
            if score > bestScore {
                best = other
                bestScore = score
            }
        }
        return best
    }

    /// The word list's next-letter frequencies after the word being typed, or nil when they
    /// can't help: inside something that isn't a word, or a word the list doesn't have.
    private func continuationMass(_ model: TypingDecoderModel) -> [Double]? {
        let before = target.documentContextBeforeInput ?? ""
        let token = AutocorrectEngine.currentToken(in: before)
        if token == nil, let last = before.last, last.isLetter || last.isNumber || last == "'" { return nil }
        guard let prefix = model.encode((token ?? "").lowercased()) else { return nil }
        if prefix.isEmpty {
            if let cached = wordStartMass { return cached }
            let mass = model.continuations(after: [])
            wordStartMass = .some(mass)
            return mass
        }
        if let cache = continuationCache, cache.prefix == prefix { return cache.mass }
        let mass = model.continuations(after: prefix)
        continuationCache = (prefix, mass)
        return mass
    }

    /// Whether a finger going down at `point`, in the strip above the keys, belongs to the
    /// keys: in the strip's lower half while you type (a key is down, or one went down or came
    /// up within `stripTouchGuard`) it is a row-1 key aimed high, not a pick. Nobody reads a
    /// new suggestion and taps it that fast; Record typing logs showed such picks were the e
    /// below the left slot, which kept "th" and typed a space where "the" was meant.
    func takesStripTouch(at point: CGPoint) -> Bool {
        let strip = layout.stripFrame
        guard point.y < layout.touchAreaFrame.minY, point.y >= strip.midY,
              point.x >= strip.minX, point.x < strip.maxX else { return false }
        if !touches.isEmpty { return true }
        guard let last = lastKeyTouchAt else { return false }
        let elapsed = clock() - last
        return elapsed >= 0 && elapsed < Self.stripTouchGuard
    }

    /// `point`, or for one above the keys (a strip touch `takesStripTouch(at:)` gave them)
    /// the same spot on row 1's top edge.
    private func pointOnKeys(_ point: CGPoint) -> CGPoint {
        let top = layout.touchAreaFrame.minY
        return point.y < top ? CGPoint(x: point.x, y: top) : point
    }

    private static func distance(from point: CGPoint, to rect: CGRect) -> CGFloat {
        let dx = max(rect.minX - point.x, 0, point.x - rect.maxX)
        let dy = max(rect.minY - point.y, 0, point.y - rect.maxY)
        return (dx * dx + dy * dy).squareRoot()
    }

    func touchBegan(id: Int, at point: CGPoint) {
        let point = pointOnKeys(point)
        guard let key = key(at: point) else { return }
        lastKeyTouchAt = clock()
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
                commitCharacter(touch.key, at: touch.startPoint)
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
        touch.claimed = !key.hitFrame.contains(point)
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
        case .character, .space, .returnKey, .hex, .globe, .emoji:
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
        guard let options = AccentCatalog.options(for: base, uppercase: isUppercase, language: language),
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

        // 2. Spacebar cursor trackpad: horizontal drag >= 10 pt slides the text caret. With two
        // or more languages only once the space bar was held (`holdSpace`): a slide before
        // that is a language swipe.
        if touch.key.kind == .space {
            if touch.isLanguageSwipe {
                touches[id] = touch
                return
            }
            if !touch.isTrackpad, abs(point.x - touch.startPoint.x) >= Self.trackpadActivationDistance {
                if switchesLanguage {
                    touch.isLanguageSwipe = true
                    touches[id] = touch
                    return
                }
                startTrackpad(&touch, id: id, anchorX: point.x, hold: false)
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
                    tokenTaps.removeAll()
                    suggestionSpaceBefore = nil
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
        // A letter the next-letter odds claimed keeps the touch while it stays within reach.
        if touch.key.character != nil, !touch.committed,
           let under = layout.hitTest(point), under.character != nil, under.kind != touch.key.kind,
           !(touch.claimed && Self.distance(from: point, to: touch.key.hitFrame) < letterReach) {
            touch.claimed = false
            touch.key = under
            touch.startPoint = point
            touches[id] = touch
            onChange?()
            return
        }
        touches[id] = touch
    }

    /// The space bar held `trackpadHoldDelay` without sliding: the trackpad starts where the
    /// finger is, as Apple's does, so letting go types no space (a press held to move the
    /// cursor used to type one beside the space already there).
    func holdSpace(forTouch id: Int) {
        guard var touch = touches[id], touch.key.kind == .space, !touch.isTrackpad, !touch.isLanguageSwipe else { return }
        startTrackpad(&touch, id: id, anchorX: touch.point.x, hold: true)
    }

    private func startTrackpad(_ touch: inout ActiveTouch, id: Int, anchorX: CGFloat, hold: Bool) {
        touch.isTrackpad = true
        touch.trackpadAnchorX = anchorX
        lastSpaceAt = nil
        swipedWord = nil
        autocorrect.clearPendingUndo()
        touches[id] = touch
        var fields = [TypingLogField("id", .int(id))]
        if hold { fields.append(TypingLogField("hold", .bool(true))) }
        typingLog?.ui("trackpad_start", fields)
        onSelectionStep?()
        onChange?()
    }

    func touchEnded(id: Int, at point: CGPoint) {
        guard var touch = touches.removeValue(forKey: id) else { return }
        touchOrder.removeAll { $0 == id }
        lastKeyTouchAt = clock()
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
                commitCharacter(touch.key, at: touch.startPoint)
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
            } else if touch.isLanguageSwipe, let step = Self.languageSwipeStep(from: touch.startPoint, to: point) {
                typingLog?.ui("language_swipe", [
                    TypingLogField("id", .int(id)),
                    TypingLogField("step", .int(step)),
                ])
                onLanguageSwipe?(step)
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
        case .emoji:
            onEmojiKey?()
        case .delete, .hex, .globe:
            break
        }
        onChange?()
    }

    /// A language swipe's direction, or nil when it was too short or more up than along.
    private static func languageSwipeStep(from start: CGPoint, to end: CGPoint) -> Int? {
        let dx = end.x - start.x
        guard abs(dx) >= languageSwipeDistance, abs(dx) >= abs(end.y - start.y) else { return nil }
        return dx < 0 ? 1 : -1
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
        let hadTouches = !touches.isEmpty || activeAccentMenu != nil || isSwiping
        touches.removeAll()
        touchOrder.removeAll()
        isSwiping = false
        swipePreview = nil
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
        // Right after a swipe, delete takes back its whole word (and the space it added).
        let swiped = swipedWord
        swipedWord = nil
        if allowAutocorrectUndo, let swiped, deleteSwipedWord(swiped) { return }
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
            tokenTaps.removeAll()
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
        if let last = target.documentContextBeforeInput?.last, last.isLetter || last == "'", !tokenTaps.isEmpty {
            tokenTaps.removeLast()
        } else {
            tokenTaps.removeAll()
        }
        target.deleteBackward()
        lastSpaceAt = nil
        didEdit()
    }

    func deleteWordBackward() {
        swipedWord = nil
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
        tokenTaps.removeAll()
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

    private func commitCharacter(_ key: KeyboardKey, at point: CGPoint) {
        guard let character = key.character else { return }
        let text = layerState.layer == .letters && shift.isUppercase ? character.uppercased() : character
        if layerState.layer == .letters {
            shift.didType()
        }
        autocorrect.clearPendingUndo()
        insert(text, source: "key", at: point)
        logState("typed")
    }

    /// The taps of the word being typed when every letter came from a key, else nil.
    private var currentTokenTaps: [CGPoint]? {
        let taps = tokenTaps.compactMap { $0 }
        return taps.count == tokenTaps.count && !taps.isEmpty ? taps : nil
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
        // Space never autocorrects a swiped word: it is a reading, not a typo.
        let afterSwipe = swipedWord != nil
        swipedWord = nil
        let now = clock()
        if let before = suggestionSpaceBefore {
            suggestionSpaceBefore = nil
            if before.last == " ", before == target.documentContextBeforeInput {
                // The suggestion typed this space already. It still counts as one, so a quick
                // second press types ". ".
                typingLog?.edit("space_swallowed", [TypingLogField("after", .string("suggestion"))])
                lastSpaceAt = now
                tokenTaps.removeAll()
                layerState.didInsert(" ")
                rebuildLayoutIfNeeded()
                onChange?()
                return
            }
        }
        if DoubleSpacePeriod.shouldReplace(before: target.documentContextBeforeInput, lastSpaceAt: lastSpaceAt, now: now) {
            autocorrect.clearPendingUndo()
            typingLog?.edit("double_space_period", [
                TypingLogField("deleted", .string(" ")),
                TypingLogField("inserted", .string(". ")),
            ])
            target.deleteBackward()
            target.insertText(". ")
            tokenTaps.removeAll()
            lastSpaceAt = nil
            layerState.didInsert(" ")
            rebuildLayoutIfNeeded()
            didEdit()
            typingLog?.context("space")
            return
        }
        learning?.didFinish(word: AutocorrectEngine.currentToken(in: target.documentContextBeforeInput))
        syncLearning()
        autocorrect.decoderTaps = currentTokenTaps
        if !afterSwipe, let (original, replacement) = autocorrect.replacementOnSpace(
            beforeInput: target.documentContextBeforeInput,
            field: field,
            autocorrectionType: target.autocorrectionType
        ) {
            for _ in 0..<original.count {
                target.deleteBackward()
            }
            let inserted = replacement + " "
            tokenTaps.removeAll()
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
    private func insert(_ text: String, source: String?, at point: CGPoint? = nil) {
        swipedWord = nil
        if text.count == 1, text.first?.isLetter == true {
            // Spatial autocorrect reads taps as on the unsplit keyboard (`KeyboardLayout.unsplitX`).
            tokenTaps.append(point.map { CGPoint(x: layout.unsplitX($0.x), y: $0.y) })
        } else {
            tokenTaps.removeAll()
        }
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
        suggestionSpaceBefore = nil
        refreshAutoCapitalization()
        refreshSuggestions()
        onEdit?()
    }

    // MARK: - Swipe typing (Experiments › Swipe typing; SwipeTyping.swift)

    /// The word a swipe just typed, until anything else changes the text: delete takes it
    /// back whole, space never autocorrects it, and the strip offers the swipe's other
    /// readings, any of which takes its place.
    struct SwipedWord: Equatable {
        /// What the swipe put in: a space when one was needed, then the word.
        var inserted: String
        var word: String
        /// Its other readings, cased as the word is, best first.
        var alternatives: [String]
    }

    private(set) var swipedWord: SwipedWord?
    /// From a touch becoming a swipe until it ends.
    private(set) var isSwiping = false
    /// What the swipe under way reads as so far, for the strip.
    private(set) var swipePreview: String?

    /// Whether a letter touch may become a swipe now: on the letters page of a text field
    /// (not an address or a URL), with no accent menu open.
    var allowsSwipes: Bool {
        layerState.layer == .letters && field == .text && activeAccentMenu == nil
    }

    /// The letter keys a swipe is read against, where they're drawn.
    var swipeKeys: TypingDecoder.Keys {
        TypingDecoder.Keys(layout: layout) ?? TypingDecoder.Keys(centres: [:], pitch: .zero, bottomRowY: 0)
    }

    /// The word before a swipe's, lowercased, when only spaces (or the one the swipe will
    /// add) come between them.
    var swipePreviousWord: String? {
        let before = target.documentContextBeforeInput ?? ""
        return AutocorrectEngine.previousWord(SwipeTyping.needsSpace(before: before) ? before + " " : before, token: "")
    }

    /// A letter touch became a swipe: its key no longer types.
    func beginSwipe(touchID id: Int) {
        guard touches.removeValue(forKey: id) != nil else { return }
        touchOrder.removeAll { $0 == id }
        isSwiping = true
        swipePreview = nil
        typingLog?.ui("swipe_start", [TypingLogField("id", .int(id))])
        refreshSuggestions()
        onChange?()
    }

    /// The swipe under way reads as `words` so far (best first): the strip shows the first
    /// that may be offered.
    func showSwipePreview(_ words: [String]) {
        guard isSwiping else { return }
        let next = swipeReadings(words).first.map { swipeCased($0, capital: swipeCapital) }
        guard next != swipePreview else { return }
        swipePreview = next
        refreshSuggestions()
    }

    /// A swipe ended. `words` are its readings, best first (none when it was cancelled): the
    /// first that may be offered goes in, after a space unless the text before ends in one or
    /// in opening punctuation, as Apple's keyboard types a swiped word, with a capital when
    /// shift is on (all capitals with caps lock); the next three wait in the strip.
    func endSwipe(typing words: [String], points: Int, decodeTime: TimeInterval?) {
        guard isSwiping else { return }
        isSwiping = false
        swipePreview = nil
        // The finger came up off a key: a quick tap on the strip's lower half next is a key.
        lastKeyTouchAt = clock()
        let readings = swipeReadings(words)
        if let log = typingLog {
            log.ui("swipe_end", [
                TypingLogField("points", .int(points)),
                TypingLogField("ms", decodeTime.map { .ms($0 * 1000) } ?? .null),
                TypingLogField("readings", .strings(Array(readings.prefix(4)))),
            ])
        }
        guard let first = readings.first else {
            refreshSuggestions()
            onChange?()
            return
        }
        let before = target.documentContextBeforeInput
        let space = SwipeTyping.needsSpace(before: before)
        let capital = swipeCapital
        let word = swipeCased(first, capital: capital)
        let inserted = (space ? " " : "") + word
        if space { learning?.didFinish(word: AutocorrectEngine.currentToken(in: before)) }
        autocorrect.clearPendingUndo()
        tokenTaps.removeAll()
        typingLog?.edit("insert", [
            TypingLogField("inserted", .string(inserted)),
            TypingLogField("src", .string("swipe")),
        ])
        target.insertText(inserted)
        lastSpaceAt = nil
        layerState.didInsert(inserted)
        if layerState.layer == .letters { shift.didType() }
        swipedWord = SwipedWord(
            inserted: inserted,
            word: word,
            alternatives: readings.dropFirst().prefix(3).map { swipeCased($0, capital: capital) }
        )
        logState("typed")
        didEdit()
        typingLog?.context("swipe")
    }

    /// The strip while swiping (the word so far) and right after (the other readings, the
    /// likeliest in the centre slot); nil otherwise.
    private var swipeSuggestions: [KeyboardSuggestion]? {
        if isSwiping {
            return swipePreview.map { [KeyboardSuggestion(displayText: $0, replacement: $0, kind: .primary(autoReplace: true))] } ?? []
        }
        guard let swipedWord else { return nil }
        let items = swipedWord.alternatives.prefix(3).map { KeyboardSuggestion(displayText: $0, replacement: $0, kind: .swiped) }
        return items.count >= 2 ? [items[1], items[0]] + items.dropFirst(2) : items
    }

    /// `words` that may be offered after the word before (`AutocorrectEngine.mayOffer`), once each.
    private func swipeReadings(_ words: [String]) -> [String] {
        let previous = swipePreviousWord
        var seen = Set<String>()
        return words.filter { autocorrect.mayOffer($0, after: previous) && seen.insert($0.lowercased()).inserted }
    }

    /// Whether a swiped word starts with a capital: shift (one-shot, held, caps lock, or
    /// automatic), or the automatic capital the space the swipe adds brings ("Hi." then a
    /// swipe types " There").
    private var swipeCapital: Bool {
        if shift.isUppercase { return true }
        guard shift.state == .off, !shift.isHeld else { return false }
        let before = target.documentContextBeforeInput
        guard SwipeTyping.needsSpace(before: before) else { return false }
        return AutoCapitalization.shouldCapitalize(
            mode: AutoCapitalization.Mode(target.autocapitalizationType),
            before: (before ?? "") + " ",
            hasText: true
        )
    }

    /// A reading as it's typed: all capitals with caps lock, a capital first letter with
    /// `capital`, else a name the dictionary only knows capitalised ("monday" -> "Monday").
    private func swipeCased(_ word: String, capital: Bool) -> String {
        if shift.state == .locked { return word.uppercased() }
        guard let first = word.first else { return word }
        if capital { return first.uppercased() + word.dropFirst() }
        guard autocorrect.hasDictionary, word == word.lowercased(), word.count >= 2 else { return word }
        let capitalised = first.uppercased() + word.dropFirst()
        return autocorrect.checker.isWordValid(capitalised) && !autocorrect.checker.isWordValid(word) ? capitalised : word
    }

    /// Puts another of the swipe's readings in its word's place; the word joins the others.
    private func replaceSwipedWord(with text: String) {
        guard var swiped = swipedWord, let before = target.documentContextBeforeInput, before.hasSuffix(swiped.word) else {
            swipedWord = nil
            refreshSuggestions()
            return
        }
        typingLog?.edit("suggestion", [
            TypingLogField("slot", suggestions.firstIndex { $0.replacement == text }.map { .int($0) } ?? .null),
            TypingLogField("text", .string(text)),
            TypingLogField("deleted", .string(swiped.word)),
            TypingLogField("inserted", .string(text)),
            TypingLogField("candidates", TypingLogger.candidates(suggestions)),
        ])
        for _ in 0..<swiped.word.count {
            target.deleteBackward()
        }
        target.insertText(text)
        swiped.inserted = String(swiped.inserted.dropLast(swiped.word.count)) + text
        swiped.alternatives = Array(([swiped.word] + swiped.alternatives.filter { $0 != text }).prefix(3))
        swiped.word = text
        swipedWord = swiped
        lastSpaceAt = nil
        didEdit()
        typingLog?.context("suggestion")
    }

    /// Takes back what a swipe typed when it is still right before the cursor.
    private func deleteSwipedWord(_ swiped: SwipedWord) -> Bool {
        guard let before = target.documentContextBeforeInput, before.hasSuffix(swiped.inserted) else { return false }
        autocorrect.clearPendingUndo()
        typingLog?.edit("delete", [
            TypingLogField("deleted", .string(swiped.inserted)),
            TypingLogField("src", .string("swipe")),
        ])
        for _ in 0..<swiped.inserted.count {
            target.deleteBackward()
        }
        tokenTaps.removeAll()
        lastSpaceAt = nil
        didEdit()
        return true
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
            showsEmoji: showsEmoji,
            horizontalInset: horizontalInset,
            language: language,
            posture: posture,
            windowInset: windowInset
        )
        syncDecoderKeys()
        logState(cause)
    }

    /// The letter keys Spatial autocorrect scores taps against: this layout's, moved by
    /// `tapOffsets`. None off the letters page.
    private func syncDecoderKeys() {
        autocorrect.decoderKeys = TypingDecoder.Keys(layout: layout, offsets: tapOffsets)
    }

    /// Hands the layout and shift to typing logs, which writes a line when either changed.
    func logState(_ cause: String) {
        typingLog?.keyboardState(layout: layout, shift: shift, cause: cause)
    }
}
