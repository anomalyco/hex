import UIKit

/// The Hex key right of space. Warm session: an orange core, and a hold records here.
/// Otherwise the mark is faded with ↗, and a hold leaves for Hex to start one.
/// A tap dictates, a hold dictates until released (push to talk), and sliding opens the
/// style menu (`HexKeyGesture`).
@MainActor
final class HexKeyControl: UIControl {
    var palette = FullKeyboardPalette(isDark: false) { didSet { if palette != oldValue { setNeedsDisplay() } } }
    /// Sits in the suggestion strip instead of the bottom row (prototype).
    var inStrip = false { didSet { if inStrip != oldValue { setNeedsDisplay() } } }
    var isWarm = false { didSet { if isWarm != oldValue { setNeedsDisplay(); updateAccessibility() } } }
    /// Dictations paste translated into this target (BCP 47): the key wears its code ("EN")
    /// in the corner, so a dictation never comes out in another language by surprise.
    var translatingTo: String? { didSet { if translatingTo != oldValue { setNeedsDisplay(); updateAccessibility() } } }
    var cornerRadius: CGFloat = FullKeyboardPalette.cornerRadius { didSet { if cornerRadius != oldValue { setNeedsDisplay() } } }
    /// The drawn keycap inside the control's (larger) touch target.
    var keycapFrame: CGRect = .zero { didSet { if keycapFrame != oldValue { setNeedsDisplay() } } }

    /// The finger landed on the key, with when (wall clock, from the touch's own timestamp).
    var onPress: ((TimeInterval) -> Void)?
    /// The press became a slide (the menu) or was cancelled before it was a tap or a hold.
    var onPressAbandoned: (() -> Void)?
    /// A tap or still hold: dictate, or open Hex.
    var onHold: (() -> Void)?
    /// The finger lifted after a still hold: push to talk's finish.
    var onHoldRelease: (() -> Void)?
    /// Points are in this control's coordinates.
    var onMenuOpen: ((CGPoint) -> Void)?
    var onMenuDrag: ((CGPoint) -> Void)?
    var onMenuRelease: ((CGPoint) -> Void)?

    private var gesture = HexKeyGesture()
    private var holdWork: DispatchWorkItem?

    override init(frame: CGRect) {
        super.init(frame: frame)
        isOpaque = false
        backgroundColor = .clear
        contentMode = .redraw
        isAccessibilityElement = true
        accessibilityTraits = .button
        updateAccessibility()
        accessibilityCustomActions = [
            UIAccessibilityCustomAction(name: "Dictation styles") { [weak self] _ in
                guard let self else { return false }
                self.onMenuOpen?(CGPoint(x: self.bounds.midX, y: self.bounds.midY))
                return true
            },
        ]
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    override var isHighlighted: Bool {
        didSet { if isHighlighted != oldValue { setNeedsDisplay() } }
    }

    private func updateAccessibility() {
        let label = isWarm ? "Dictate with Hex" : "Open Hex to dictate"
        accessibilityLabel = translatingTo.map { "\(label), \(TranslationPasteChoice.translatingLabel(target: $0))" } ?? label
        accessibilityHint = "Tap to dictate, hold to talk, slide for styles"
    }

    override func accessibilityActivate() -> Bool {
        onHold?()
        return true
    }

    override func beginTracking(_ touch: UITouch, with event: UIEvent?) -> Bool {
        gesture.touchDown(at: touch.location(in: self))
        isHighlighted = true
        holdWork?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.holdElapsed() }
        holdWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + HexKeyGesture.holdDuration, execute: work)
        onPress?(Date().timeIntervalSince1970 - max(0, ProcessInfo.processInfo.systemUptime - touch.timestamp))
        return true
    }

    override func continueTracking(_ touch: UITouch, with event: UIEvent?) -> Bool {
        let point = touch.location(in: self)
        switch gesture.touchMoved(to: point) {
        case .openMenu:
            stopTimer()
            isHighlighted = false
            onPressAbandoned?()
            onMenuOpen?(point)
        case .dragMenu:
            onMenuDrag?(point)
        default:
            break
        }
        return true
    }

    override func endTracking(_ touch: UITouch?, with event: UIEvent?) {
        stopTimer()
        isHighlighted = false
        switch gesture.touchUp() {
        case .releaseMenu:
            if let touch { onMenuRelease?(touch.location(in: self)) }
        case .tap:
            onHold?()
        case .release:
            onHoldRelease?()
        default:
            break
        }
    }

    override func cancelTracking(with event: UIEvent?) {
        stopTimer()
        isHighlighted = false
        if gesture.cancel() == .abandon {
            onPressAbandoned?()
        }
    }

    private func holdElapsed() {
        holdWork = nil
        guard gesture.holdElapsed() == .hold else { return }
        isHighlighted = false
        onHold?()
    }

    private func stopTimer() {
        holdWork?.cancel()
        holdWork = nil
    }

    override func draw(_ rect: CGRect) {
        guard let context = UIGraphicsGetCurrentContext() else { return }
        let cap = keycapFrame == .zero ? bounds : keycapFrame
        if !inStrip {
            context.fillKeycap(
                cap,
                fill: isHighlighted ? palette.keyPressedFill : palette.keyFill,
                radius: cornerRadius
            )
        } else if isHighlighted {
            palette.keyPressedFill.setFill()
            UIBezierPath(roundedRect: cap, cornerRadius: cap.height / 2).fill()
        }
        // In the strip the mark keeps its 19 pt in the 26 pt pill as in the Duo's 36 pt one.
        let markSize: CGFloat = inStrip ? min(19, cap.height) : min(19, cap.height * 0.56)
        let markRect = CGRect(x: cap.midX - markSize / 2, y: cap.midY - markSize / 2, width: markSize, height: markSize)
        if isWarm {
            HexMarkPath.draw(in: context, rect: markRect, color: palette.textBase, coreColor: palette.warmCore)
        } else {
            // A tap dictates inside the keyboard now, so no ↗ "opens Hex" badge.
            HexMarkPath.draw(in: context, rect: markRect, color: palette.textBase, coreColor: nil)
        }
        if let translatingTo {
            drawTranslationBadge(TranslationPasteChoice.badge(target: translatingTo), keycap: cap, mark: markRect)
        }
    }

    /// "EN" in a small capsule at the keycap's top trailing corner, over the mark's edge.
    private func drawTranslationBadge(_ code: String, keycap cap: CGRect, mark: CGRect) {
        let attributes: [NSAttributedString.Key: Any] = [
            .font: UIFont.systemFont(ofSize: 8.5, weight: .bold),
            .foregroundColor: palette.isDark ? UIColor.black : UIColor.white,
        ]
        let size = (code as NSString).size(withAttributes: attributes)
        let height: CGFloat = 12
        let width = max(height, ceil(size.width) + 6)
        // Inside the keycap, clear of its corner radius; in the strip (no keycap) beside the mark.
        let x = inStrip ? mark.maxX - width / 2 : min(cap.maxX - width - 2, mark.maxX - 3)
        let y = inStrip ? mark.minY - height / 2 : cap.minY + 2
        let badge = CGRect(x: x, y: y, width: width, height: height)
        palette.textBase.setFill()
        UIBezierPath(roundedRect: badge, cornerRadius: height / 2).fill()
        (code as NSString).draw(
            at: CGPoint(x: badge.midX - size.width / 2, y: badge.midY - size.height / 2),
            withAttributes: attributes
        )
    }
}

/// While recording, this replaces the suggestion strip only, so the keys stay put: a Dot
/// Scope well that takes the rest of the strip, then ✕ and Done, ✓ where the Hex key was.
/// With the Hex key on the left it mirrors: ✓, ✕, then the well.
@MainActor
final class HexKeyRecordingPanel: UIView {
    var palette = FullKeyboardPalette(isDark: false) { didSet { if palette != oldValue { applyPalette() } } }
    var onDone: (() -> Void)?
    var onCancel: (() -> Void)?
    var onCycleStyle: (() -> Void)?

    let scope: DotScopeHostView
    private let styleButton = UIButton(type: .custom)
    private let well = UIView()
    private let cancelButton = PanelKeyButton()
    private let doneButton = PanelKeyButton()
    private let doneSpinner = UIActivityIndicatorView(style: .medium)
    private var isTranscribing = false
    /// Width of one modifier key, so ✕ lines up with 123 + space's start.
    var sideKeyWidth: CGFloat = 50 { didSet { setNeedsLayout() } }
    var horizontalInset: CGFloat = 0 { didSet { setNeedsLayout() } }
    var metrics = KeyboardMetrics.portrait { didSet { setNeedsLayout() } }
    /// The Hex key sits on the left (Settings › Keyboard › Hex key): ✓ and ✕ lead.
    var hexOnLeading = false { didSet { if hexOnLeading != oldValue { setNeedsLayout() } } }
    /// Record typing logs is recording: the mark the strip shows, at the well's end away
    /// from ✕ and ✓.
    var showsLoggingMark = false {
        didSet {
            guard showsLoggingMark != oldValue else { return }
            if showsLoggingMark, loggingMark == nil {
                let mark = TypingLogMarkView()
                mark.textColor = palette.textMuted
                addSubview(mark)
                loggingMark = mark
            }
            loggingMark?.isHidden = !showsLoggingMark
            setNeedsLayout()
        }
    }
    private var loggingMark: TypingLogMarkView?

    init(loudness: @escaping () -> Float) {
        var tuning = DotScopeTuning.bar(leadingInset: 0, trailingInset: 0, fade: 24, spacing: 5)
        tuning.height = Float(KeyboardMetrics.portrait.stripPillHeight)
        // Cap drawables at 2 inside the keyboard extension to save ~2.2 MB of IOSurface RAM.
        scope = DotScopeHostView(tuning: tuning, maximumDrawableCount: 2)
        super.init(frame: .zero)
        scope.drawPlate = false
        scope.loudnessProvider = loudness
        scope.reduceMotion = UIAccessibility.isReduceMotionEnabled
        scope.useDemoVoice = ProcessInfo.processInfo.arguments.contains("--snap-state")

        styleButton.titleLabel?.font = .systemFont(ofSize: 13, weight: .medium)
        styleButton.semanticContentAttribute = .forceRightToLeft
        styleButton.addTarget(self, action: #selector(cycleStyle), for: .touchUpInside)
        styleButton.accessibilityHint = "Changes the dictation style"

        well.layer.cornerRadius = 20
        well.layer.cornerCurve = .continuous
        well.clipsToBounds = true
        well.addSubview(scope)

        cancelButton.symbol = "xmark"
        cancelButton.accessibilityLabel = "Cancel dictation"
        cancelButton.addTarget(self, action: #selector(cancel), for: .touchUpInside)
        doneButton.symbol = "checkmark"
        doneButton.isPrimary = true
        doneButton.accessibilityLabel = "Done"
        doneButton.addTarget(self, action: #selector(done), for: .touchUpInside)

        doneSpinner.hidesWhenStopped = true
        doneSpinner.color = palette.textBase
        doneSpinner.isUserInteractionEnabled = false
        for view in [styleButton, well, cancelButton, doneButton, doneSpinner] {
            addSubview(view)
        }
        applyPalette()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    func tearDown() {
        scope.phase = .hidden
        scope.tearDown()
    }

    func update(phase: DotScopePhase, style: DictationStyle) {
        isTranscribing = phase == .processing
        scope.phase = phase
        // The scope, then two round buttons, ✕ and ✓; once the recording stops the ✓ turns into
        // the spinner in the same spot, so there's one place to look, and ✕ still cancels.
        doneButton.symbol = isTranscribing ? nil : "checkmark"
        doneButton.isPrimary = true
        doneButton.isEnabled = !isTranscribing
        doneButton.accessibilityLabel = isTranscribing ? "Transcribing" : "Done"
        if isTranscribing { doneSpinner.startAnimating() } else { doneSpinner.stopAnimating() }
        let config = UIImage.SymbolConfiguration(pointSize: 9, weight: .semibold)
        styleButton.setImage(
            UIImage(systemName: "chevron.down", withConfiguration: config)?
                .withTintColor(palette.textMuted, renderingMode: .alwaysOriginal),
            for: .normal
        )
        styleButton.setTitle(style.label + " ", for: .normal)
        styleButton.accessibilityLabel = "Style: \(style.label)"
        setNeedsLayout()
    }

    private func applyPalette() {
        styleButton.setTitleColor(palette.textMuted, for: .normal)
        well.backgroundColor = palette.wellFill
        scope.isDark = palette.isDark
        cancelButton.palette = palette
        doneButton.palette = palette
        doneSpinner.color = palette.textBase
        loggingMark?.textColor = palette.textMuted
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        let innerLeft = horizontalInset + metrics.side
        let innerRight = bounds.width - horizontalInset - metrics.side
        // The strip's pill height, inset from its top and bottom as its pills are.
        let rowH = metrics.stripPillHeight
        let rowY = (bounds.height - rowH) / 2

        doneButton.cornerRadius = rowH / 2
        cancelButton.cornerRadius = rowH / 2
        if hexOnLeading {
            doneButton.frame = CGRect(x: innerLeft, y: rowY, width: rowH, height: rowH)
            cancelButton.frame = CGRect(x: doneButton.frame.maxX + 6, y: rowY, width: rowH, height: rowH)
        } else {
            doneButton.frame = CGRect(x: innerRight - rowH, y: rowY, width: rowH, height: rowH)
            cancelButton.frame = CGRect(x: doneButton.frame.minX - 6 - rowH, y: rowY, width: rowH, height: rowH)
        }
        doneSpinner.center = CGPoint(x: doneButton.frame.midX, y: doneButton.frame.midY)

        // No room for the style chip in the strip; the Hex key's menu sets the style.
        styleButton.isHidden = true
        let wellLeft = hexOnLeading ? cancelButton.frame.maxX + 6 : innerLeft
        let wellRight = hexOnLeading ? innerRight : cancelButton.frame.minX - 6
        well.frame = CGRect(x: wellLeft, y: rowY, width: max(24, wellRight - wellLeft), height: rowH)
        if let loggingMark, showsLoggingMark {
            // The scope stops short of the mark, so its dots never run under the text.
            let size = loggingMark.sizeThatFits(bounds.size)
            let markX = hexOnLeading ? well.frame.maxX - 14 - size.width : well.frame.minX + 14
            loggingMark.frame = CGRect(x: markX, y: well.frame.midY - size.height / 2, width: size.width, height: size.height)
            if hexOnLeading {
                let end = loggingMark.frame.minX - 6 - well.frame.minX
                scope.frame = CGRect(x: 0, y: 0, width: max(0, end), height: well.bounds.height)
            } else {
                let lead = loggingMark.frame.maxX + 6 - well.frame.minX
                scope.frame = CGRect(x: lead, y: 0, width: max(0, well.bounds.width - lead), height: well.bounds.height)
            }
        } else {
            scope.frame = well.bounds
        }
    }

    @objc private func done() { onDone?() }
    @objc private func cancel() { onCancel?() }
    @objc private func cycleStyle() { onCycleStyle?() }
}

/// A keycap-styled button for the recording panel.
@MainActor
final class PanelKeyButton: UIControl {
    var palette = FullKeyboardPalette(isDark: false) { didSet { setNeedsDisplay() } }
    var title: String? { didSet { setNeedsDisplay() } }
    var symbol: String? { didSet { setNeedsDisplay() } }
    var isPrimary = false { didSet { setNeedsDisplay() } }
    var cornerRadius: CGFloat = FullKeyboardPalette.cornerRadius { didSet { setNeedsDisplay() } }
    /// Draws its title and symbol as the letter keys' 123 and delete do (the emoji panel's
    /// ABC and delete), not the recording panel's bolder ones.
    var drawsLikeKeys = false { didSet { setNeedsDisplay() } }
    /// Only the title or symbol, with a keycap only while pressed (the emoji panel's ABC and
    /// delete, which the system emoji keyboard draws without keys).
    var isPlain = false { didSet { setNeedsDisplay() } }
    /// How far past its edges a touch still counts as on the key (the emoji panel's bar).
    var hitOutsets: UIEdgeInsets = .zero

    override init(frame: CGRect) {
        super.init(frame: frame)
        isOpaque = false
        backgroundColor = .clear
        contentMode = .redraw
        isAccessibilityElement = true
        accessibilityTraits = .button
        addTarget(self, action: #selector(redraw), for: [.touchDown, .touchDragEnter, .touchDragExit, .touchCancel, .touchUpInside, .touchUpOutside])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    override var isEnabled: Bool { didSet { setNeedsDisplay() } }
    @objc private func redraw() { setNeedsDisplay() }

    override func point(inside point: CGPoint, with event: UIEvent?) -> Bool {
        bounds.inset(by: UIEdgeInsets(top: -hitOutsets.top, left: -hitOutsets.left, bottom: -hitOutsets.bottom, right: -hitOutsets.right)).contains(point)
    }

    override func draw(_ rect: CGRect) {
        guard let context = UIGraphicsGetCurrentContext() else { return }
        if isHighlighted {
            context.fillKeycap(bounds, fill: palette.keyPressedFill, radius: cornerRadius)
        } else if !isPlain {
            context.fillKeycap(bounds, fill: isPrimary ? palette.keyFill : palette.functionFill, radius: cornerRadius)
        }
        let ink = isEnabled ? palette.textBase : palette.textMuted
        if let symbol {
            let config = drawsLikeKeys
                ? UIImage.SymbolConfiguration(pointSize: bounds.height < 34 ? 16 : 18, weight: .regular)
                : UIImage.SymbolConfiguration(pointSize: 16.5, weight: .medium)
            if let image = UIImage(systemName: symbol, withConfiguration: config)?
                .withTintColor(ink, renderingMode: .alwaysOriginal) {
                image.draw(at: CGPoint(x: bounds.midX - image.size.width / 2, y: bounds.midY - image.size.height / 2))
            }
        } else if let title {
            let font: UIFont = drawsLikeKeys
                ? .systemFont(ofSize: bounds.height < 34 ? 14.5 : 15.5)
                : .systemFont(ofSize: 16.5, weight: .semibold)
            let attributes: [NSAttributedString.Key: Any] = [
                .font: font,
                .foregroundColor: ink,
            ]
            let size = (title as NSString).size(withAttributes: attributes)
            (title as NSString).draw(
                at: CGPoint(x: bounds.midX - size.width / 2, y: bounds.midY - size.height / 2),
                withAttributes: attributes
            )
        }
    }
}

/// Holding the Hex key: dictation styles, then the dictation language when there are choices,
/// then Translate to <language> (on or off) and the target to translate into. The language
/// page takes a language off with a long press and Remove, and adds one from More languages….
@MainActor
final class HexStyleMenuView: UIView {
    enum Page { case styles, languages, moreLanguages, translation }

    var palette = FullKeyboardPalette(isDark: false) { didSet { rebuild() } }
    var style: DictationStyle = .polished { didSet { rebuild() } }
    /// Settings › Dictation language (nil for Automatic), what Automatic resolves to, and the
    /// quick choices; with no choices the menu has no Language row.
    var dictationLanguage: String? { didSet { if dictationLanguage != oldValue { rebuild() } } }
    var automaticLanguage: String? { didSet { if automaticLanguage != oldValue { rebuild() } } }
    var languageChoices: [String] = [] { didSet { if languageChoices != oldValue { rebuild() } } }
    /// Every other language Apple Speech has (`KeyboardViewModel.moreLanguages`).
    var moreLanguages: [String] = [] { didSet { if moreLanguages != oldValue { rebuild() } } }
    /// Translate what I say's target, on or off; nil hides its rows
    /// (`KeyboardViewModel.showsTranslationSwitch`).
    var translationTarget: String? { didSet { if translationTarget != oldValue { rebuild() } } }
    var isTranslating = false { didSet { if isTranslating != oldValue { rebuild() } } }
    /// The targets the Translate to page lists; with one or none it has no row.
    var translationTargets: [String] = [] { didSet { if translationTargets != oldValue { rebuild() } } }
    var onStyle: ((DictationStyle) -> Void)?
    var onLanguage: ((String?) -> Void)?
    var onRemoveLanguage: ((String) -> Void)?
    /// Translate to <language> switched on (true) or off.
    var onTranslationToggle: ((Bool) -> Void)?
    var onTranslationTarget: ((String) -> Void)?
    var onDismiss: (() -> Void)?
    var onHoverChanged: (() -> Void)?
    /// The Hex key's keycap, in this view's coordinates; the panel opens above it.
    var anchor: CGRect = .zero { didSet { setNeedsLayout() } }

    private var page: Page = .styles
    private let panel = UIView()
    /// More languages… scrolls its long list inside the panel.
    private let scroller = MenuScrollView()
    private var rows: [MenuRow] = []
    private var hoveredIndex: Int?
    /// The language a long press offered to remove, with Remove on its row.
    private var removingLanguage: String?
    private static let rowHeight: CGFloat = 36

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = .clear
        panel.layer.cornerRadius = 13
        panel.layer.cornerCurve = .continuous
        panel.layer.borderWidth = 1 / 3.0
        panel.layer.shadowOffset = CGSize(width: 0, height: 4)
        panel.layer.shadowRadius = 12
        panel.layer.shadowOpacity = 1
        addSubview(panel)
        scroller.showsVerticalScrollIndicator = false
        scroller.delaysContentTouches = false
        scroller.alwaysBounceVertical = true
        panel.addSubview(scroller)
        accessibilityViewIsModal = true
        rebuild()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    func show(page: Page) {
        self.page = page
        hoveredIndex = nil
        removingLanguage = nil
        scroller.contentOffset = .zero
        rebuild()
    }

    /// Fixtures: the language page with Remove on a language's row.
    func debugOfferRemove(_ language: String) {
        show(page: .languages)
        removingLanguage = language
        rebuild()
    }

    /// Updates row highlight while the user holds the Hex key and slides into the menu.
    func updateDrag(at point: CGPoint) {
        let nextIndex = rows.firstIndex { $0.bounds.contains(convert(point, to: $0)) }
        guard nextIndex != hoveredIndex else { return }
        hoveredIndex = nextIndex
        for (idx, row) in rows.enumerated() {
            row.isSlideHovered = (idx == nextIndex)
        }
        if nextIndex != nil {
            onHoverChanged?()
        }
    }

    /// Commits a hovered row if the hold-and-slide ended inside the panel; otherwise leaves the menu open for a tap.
    func finishDrag(at point: CGPoint) {
        if let idx = rows.firstIndex(where: { $0.bounds.contains(convert(point, to: $0)) }) {
            hoveredIndex = nil
            rows[idx].trigger()
        } else {
            hoveredIndex = nil
            rows.forEach { $0.isSlideHovered = false }
        }
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        // A tap outside the panel closes the menu.
        if let touch = touches.first, !panel.frame.contains(touch.location(in: self)) {
            onDismiss?()
        }
    }

    private func rebuild() {
        // Apple Speech's languages here, for labels: More languages holds every one the
        // switch doesn't, so the two together name a language's region where it has several.
        let supported = languageChoices + moreLanguages
        panel.backgroundColor = palette.popupFill
        panel.layer.borderColor = palette.popupBorder.cgColor
        panel.layer.borderWidth = 1 / max(1, traitCollection.displayScale)
        panel.layer.shadowColor = palette.popupShadow.cgColor
        rows.forEach { $0.removeFromSuperview() }
        rows = []
        switch page {
        case .styles:
            for option in DictationStyle.allCases {
                rows.append(makeRow(title: option.label, selected: option == style) { [weak self] in
                    self?.onStyle?(option)
                })
            }
            if !languageChoices.isEmpty || !moreLanguages.isEmpty {
                let current = (dictationLanguage ?? automaticLanguage).map {
                    DictationLanguageSwitch.nativeName(for: $0, supported: supported)
                }
                rows.append(makeRow(title: String(localized: "Language"), detail: current ?? String(localized: "Automatic"), chevron: true, selected: false, showsTopDivider: true) { [weak self] in
                    self?.show(page: .languages)
                })
            }
            if let translationTarget {
                // A switch: lit with a checkmark while on, like a picked style.
                let on = isTranslating
                let toggle = makeRow(title: TranslationPasteChoice.toggleTitle(target: translationTarget), selected: on, showsTopDivider: true) { [weak self] in
                    self?.onTranslationToggle?(!on)
                }
                toggle.accessibilityValue = on ? String(localized: "On") : String(localized: "Off")
                toggle.accessibilityTraits = on ? [.button, .selected] : .button
                rows.append(toggle)
                if translationTargets.count > 1 {
                    rows.append(makeRow(title: String(localized: "Translation language"), chevron: true, selected: false) { [weak self] in
                        self?.show(page: .translation)
                    })
                }
            }
        case .languages:
            let automatic = automaticLanguage.map {
                DictationLanguageSwitch.nativeName(for: $0, supported: supported)
            }
            rows.append(makeRow(title: String(localized: "Automatic"), detail: automatic, selected: dictationLanguage == nil) { [weak self] in
                self?.onLanguage?(nil)
            })
            let names = DictationLanguageSwitch.nativeNames(for: languageChoices, supported: supported)
            for id in languageChoices {
                let name = names[id] ?? id
                let removing = id == removingLanguage
                let row = makeRow(title: name, selected: id == dictationLanguage) { [weak self] in
                    guard let self else { return }
                    if removing {
                        // A tap on the row, beside Remove, keeps the language.
                        self.removingLanguage = nil
                        self.rebuild()
                    } else {
                        self.onLanguage?(id)
                    }
                }
                if removing {
                    row.addRemoveButton(title: String(localized: "Remove"), accessibilityLabel: String(localized: "Remove \(name)")) { [weak self] in
                        self?.removingLanguage = nil
                        self?.onRemoveLanguage?(id)
                    }
                }
                row.onLongPress = { [weak self] in
                    guard let self, self.removingLanguage != id else { return }
                    self.onHoverChanged?()
                    self.removingLanguage = id
                    self.rebuild()
                }
                row.accessibilityCustomActions = [
                    UIAccessibilityCustomAction(name: String(localized: "Remove")) { [weak self] _ in
                        self?.onRemoveLanguage?(id)
                        return true
                    },
                ]
                rows.append(row)
            }
            if !moreLanguages.isEmpty {
                rows.append(makeRow(title: String(localized: "More languages…"), chevron: true, selected: false, showsTopDivider: true) { [weak self] in
                    self?.show(page: .moreLanguages)
                })
            }
        case .moreLanguages:
            let names = DictationLanguageSwitch.nativeNames(for: languageChoices + moreLanguages, supported: supported)
            for id in moreLanguages {
                rows.append(makeRow(title: names[id] ?? id, selected: false) { [weak self] in
                    self?.onLanguage?(id)
                })
            }
        case .translation:
            // Picking a target turns translation on: that's why you'd pick one here.
            for id in translationTargets {
                let name = TranslationPasteChoice.name(of: id)
                let current = translationTarget.map { Locale.Language(identifier: $0).minimalIdentifier }
                rows.append(makeRow(title: name, selected: id == current) { [weak self] in
                    self?.onTranslationTarget?(id)
                })
            }
        }
        let container: UIView = page == .moreLanguages ? scroller : panel
        scroller.isHidden = page != .moreLanguages
        rows.forEach(container.addSubview)
        setNeedsLayout()
    }

    private func makeRow(
        title: String,
        detail: String? = nil,
        chevron: Bool = false,
        selected: Bool,
        showsTopDivider: Bool = false,
        action: @escaping () -> Void
    ) -> MenuRow {
        let row = MenuRow(
            palette: palette,
            title: title,
            detail: detail,
            chevron: chevron,
            selected: selected,
            showsTopDivider: showsTopDivider,
            onTrigger: action
        )
        return row
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        let width: CGFloat = 224
        let padding: CGFloat = 5
        // Opens away from the edge the Hex key sits on: down from the strip, up from the keys.
        let opensDown = anchor.midY < bounds.midY
        let available = opensDown ? max(80, bounds.height - anchor.maxY - 6 - 4) : max(80, anchor.minY - 6 - 4)
        // More languages… scrolls: its rows keep their height in a panel as tall as there is room.
        let scrolls = page == .moreLanguages
        let rowH = scrolls ? Self.rowHeight : min(Self.rowHeight, (available - 2 * padding) / CGFloat(max(1, rows.count)))
        let height = scrolls
            ? min(available, CGFloat(rows.count) * rowH + 2 * padding)
            : CGFloat(rows.count) * rowH + 2 * padding
        // Toward the screen's middle: from the key's right edge leftward on the right, from its
        // left edge rightward on the left.
        let towardRight = anchor.midX < bounds.midX
        let x = min(bounds.width - 6 - width, max(6, towardRight ? anchor.minX : anchor.maxX - width))
        let y = opensDown ? anchor.maxY + 6 : max(2, anchor.minY - 6 - height)
        panel.frame = CGRect(x: x, y: y, width: width, height: height)
        if scrolls {
            scroller.frame = panel.bounds
            scroller.layer.cornerRadius = panel.layer.cornerRadius
            scroller.contentSize = CGSize(width: width, height: CGFloat(rows.count) * rowH + 2 * padding)
        }
        for (i, row) in rows.enumerated() {
            row.frame = CGRect(x: padding, y: padding + CGFloat(i) * rowH, width: width - 2 * padding, height: rowH)
        }
    }

    /// Lets a drag that starts on a row scroll the list, as a table does.
    private final class MenuScrollView: UIScrollView {
        override func touchesShouldCancel(in view: UIView) -> Bool { true }
    }

    private final class MenuRow: UIControl {
        private let palette: FullKeyboardPalette
        private let title: String
        private let detail: String?
        private let chevron: Bool
        private let isChosen: Bool
        private let showsTopDivider: Bool
        private let onTrigger: () -> Void
        private var removeButton: UIButton?
        var isSlideHovered = false { didSet { if isSlideHovered != oldValue { setNeedsDisplay() } } }
        /// A long press on the row (a language's: Remove shows on it).
        var onLongPress: (() -> Void)? {
            didSet {
                guard onLongPress != nil, longPress == nil else { return }
                let recognizer = UILongPressGestureRecognizer(target: self, action: #selector(longPressed(_:)))
                recognizer.minimumPressDuration = 0.45
                addGestureRecognizer(recognizer)
                longPress = recognizer
            }
        }
        private var longPress: UILongPressGestureRecognizer?

        init(
            palette: FullKeyboardPalette,
            title: String,
            detail: String?,
            chevron: Bool,
            selected: Bool,
            showsTopDivider: Bool,
            onTrigger: @escaping () -> Void
        ) {
            self.palette = palette
            self.title = title
            self.detail = detail
            self.chevron = chevron
            self.isChosen = selected
            self.showsTopDivider = showsTopDivider
            self.onTrigger = onTrigger
            super.init(frame: .zero)
            isOpaque = false
            backgroundColor = .clear
            contentMode = .redraw
            isAccessibilityElement = true
            accessibilityLabel = detail.map { "\(title), \($0)" } ?? title
            accessibilityTraits = isChosen ? [.button, .selected] : .button
            addAction(UIAction { [weak self] _ in self?.onTrigger() }, for: .touchUpInside)
            addTarget(self, action: #selector(redraw), for: [.touchDown, .touchDragEnter, .touchDragExit, .touchCancel, .touchUpInside, .touchUpOutside])
        }

        @available(*, unavailable)
        required init?(coder: NSCoder) { fatalError() }

        func trigger() { onTrigger() }

        @objc private func longPressed(_ recognizer: UILongPressGestureRecognizer) {
            guard recognizer.state == .began else { return }
            onLongPress?()
        }

        /// Remove, at the row's trailing end, in place of its checkmark.
        func addRemoveButton(title: String, accessibilityLabel: String, action: @escaping () -> Void) {
            var config = UIButton.Configuration.plain()
            config.attributedTitle = AttributedString(title, attributes: AttributeContainer([
                .font: UIFont.systemFont(ofSize: 14.5, weight: .medium),
                .foregroundColor: UIColor(HexTheme.danger),
            ]))
            config.contentInsets = NSDirectionalEdgeInsets(top: 0, leading: 10, bottom: 0, trailing: 10)
            config.background.backgroundColor = palette.functionFill
            config.background.cornerRadius = 7
            let button = UIButton(configuration: config, primaryAction: UIAction { _ in action() })
            button.accessibilityLabel = accessibilityLabel
            addSubview(button)
            removeButton = button
            setNeedsLayout()
        }

        override func layoutSubviews() {
            super.layoutSubviews()
            guard let removeButton else { return }
            let size = removeButton.sizeThatFits(bounds.size)
            let height = min(bounds.height - 6, 28)
            removeButton.frame = CGRect(x: bounds.maxX - 4 - size.width, y: bounds.midY - height / 2, width: size.width, height: height)
        }

        @objc private func redraw() { setNeedsDisplay() }

        override var isHighlighted: Bool {
            didSet { if isHighlighted != oldValue { setNeedsDisplay() } }
        }

        override func draw(_ rect: CGRect) {
            if showsTopDivider {
                let hairline = 1 / max(1, traitCollection.displayScale)
                palette.separator.setFill()
                UIRectFill(CGRect(x: 10, y: 0, width: bounds.width - 20, height: hairline))
            }
            let activeHover = isHighlighted || isSlideHovered
            let lit = isChosen || activeHover
            if lit {
                let fill = activeHover ? palette.keyPressedFill : palette.selectFill
                fill.setFill()
                UIBezierPath(roundedRect: bounds, cornerRadius: 8.5).fill()
            }
            let ink = isChosen ? palette.selectText : palette.textBase
            let font = UIFont.systemFont(ofSize: 15.5, weight: isChosen ? .medium : .regular)
            let attributes: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: ink]
            let size = (title as NSString).size(withAttributes: attributes)
            (title as NSString).draw(at: CGPoint(x: 12, y: bounds.midY - size.height / 2), withAttributes: attributes)

            var trailing = bounds.maxX - 12
            if removeButton != nil { return }
            if isChosen || chevron {
                let config = UIImage.SymbolConfiguration(pointSize: 12, weight: .semibold)
                let name = chevron ? "chevron.right" : "checkmark"
                if let image = UIImage(systemName: name, withConfiguration: config)?
                    .withTintColor(chevron ? palette.textMuted : ink, renderingMode: .alwaysOriginal) {
                    trailing -= image.size.width
                    image.draw(at: CGPoint(x: trailing, y: bounds.midY - image.size.height / 2))
                    trailing -= 6
                }
            }
            if let detail {
                let detailAttributes: [NSAttributedString.Key: Any] = [
                    .font: UIFont.systemFont(ofSize: 14),
                    .foregroundColor: palette.textMuted,
                ]
                let detailSize = (detail as NSString).size(withAttributes: detailAttributes)
                guard trailing - detailSize.width > 12 + size.width + 8 else { return }
                (detail as NSString).draw(
                    at: CGPoint(x: trailing - detailSize.width, y: bounds.midY - detailSize.height / 2),
                    withAttributes: detailAttributes
                )
            }
        }
    }
}