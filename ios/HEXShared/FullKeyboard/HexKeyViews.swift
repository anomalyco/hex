import UIKit

/// The Hex key right of space. Warm session: an orange core, and a tap records here.
/// Otherwise the mark is faded with ↗, and a tap leaves for Hex to start one.
/// Holding it opens the style menu, and sliding up selects a row on release.
@MainActor
final class HexKeyControl: UIControl {
    var palette = FullKeyboardPalette(isDark: false) { didSet { if palette != oldValue { setNeedsDisplay() } } }
    var isWarm = false { didSet { if isWarm != oldValue { setNeedsDisplay(); updateAccessibility() } } }
    var cornerRadius: CGFloat = FullKeyboardPalette.cornerRadius { didSet { if cornerRadius != oldValue { setNeedsDisplay() } } }
    /// The drawn keycap inside the control's (larger) touch target.
    var keycapFrame: CGRect = .zero { didSet { if keycapFrame != oldValue { setNeedsDisplay() } } }

    var onTap: (() -> Void)?
    var onLongPress: (() -> Void)?
    /// Finger location in `superview` coordinates during a hold-and-slide gesture.
    var onLongPressDrag: ((CGPoint) -> Void)?
    var onLongPressEnd: ((CGPoint) -> Void)?
    private var longPressFired = false

    override init(frame: CGRect) {
        super.init(frame: frame)
        isOpaque = false
        backgroundColor = .clear
        contentMode = .redraw
        isAccessibilityElement = true
        accessibilityTraits = .button
        updateAccessibility()

        let longPress = UILongPressGestureRecognizer(target: self, action: #selector(handleLongPress(_:)))
        longPress.minimumPressDuration = 0.35
        longPress.cancelsTouchesInView = true
        addGestureRecognizer(longPress)
        addTarget(self, action: #selector(handleTouchUp), for: .touchUpInside)
        addTarget(self, action: #selector(redraw), for: [.touchDown, .touchDragEnter, .touchDragExit, .touchCancel, .touchUpOutside])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    override var isHighlighted: Bool {
        didSet { if isHighlighted != oldValue { setNeedsDisplay() } }
    }

    private func updateAccessibility() {
        accessibilityLabel = isWarm ? "Dictate with Hex" : "Open Hex to dictate"
        accessibilityHint = "Hold for dictation styles"
    }

    @objc private func redraw() { setNeedsDisplay() }

    @objc private func handleTouchUp() {
        setNeedsDisplay()
        guard !longPressFired else {
            longPressFired = false
            return
        }
        onTap?()
    }

    @objc private func handleLongPress(_ gesture: UILongPressGestureRecognizer) {
        let pointInParent = superview.map { gesture.location(in: $0) } ?? gesture.location(in: self)
        switch gesture.state {
        case .began:
            longPressFired = true
            isHighlighted = false
            onLongPress?()
        case .changed:
            onLongPressDrag?(pointInParent)
        case .ended:
            onLongPressEnd?(pointInParent)
            DispatchQueue.main.async { self.longPressFired = false }
        case .cancelled, .failed:
            DispatchQueue.main.async { self.longPressFired = false }
        default:
            break
        }
    }

    override func draw(_ rect: CGRect) {
        guard let context = UIGraphicsGetCurrentContext() else { return }
        let cap = keycapFrame == .zero ? bounds : keycapFrame
        context.fillKeycap(
            cap,
            fill: isHighlighted ? palette.keyPressedFill : palette.keyFill,
            radius: cornerRadius
        )
        let markSize: CGFloat = min(19, cap.height * 0.56)
        let markRect = CGRect(x: cap.midX - markSize / 2, y: cap.midY - markSize / 2, width: markSize, height: markSize)
        if isWarm {
            HexMarkPath.draw(in: context, rect: markRect, color: palette.textBase, coreColor: palette.warmCore)
        } else {
            HexMarkPath.draw(in: context, rect: markRect, color: palette.textMuted, coreColor: nil)
            let config = UIImage.SymbolConfiguration(pointSize: 8.5, weight: .bold)
            if let arrow = UIImage(systemName: "arrow.up.forward", withConfiguration: config)?
                .withTintColor(palette.textMuted, renderingMode: .alwaysOriginal) {
                arrow.draw(at: CGPoint(x: cap.maxX - 5 - arrow.size.width, y: cap.minY + 4))
            }
        }
    }
}

/// While recording, this replaces the strip and keys at the same height: the clock and
/// style, a wide Dot Scope well, then ✕ and Done where the bottom row was.
@MainActor
final class HexKeyRecordingPanel: UIView {
    var palette = FullKeyboardPalette(isDark: false) { didSet { if palette != oldValue { applyPalette() } } }
    var onDone: (() -> Void)?
    var onCancel: (() -> Void)?
    var onCycleStyle: (() -> Void)?

    let scope: DotScopeHostView
    private let dot = UIView()
    private let clockLabel = UILabel()
    private let styleButton = UIButton(type: .custom)
    private let well = UIView()
    private let cancelButton = PanelKeyButton()
    private let doneButton = PanelKeyButton()
    private var isTranscribing = false
    /// Width of one modifier key, so ✕ lines up with 123 + space's start.
    var sideKeyWidth: CGFloat = 50 { didSet { setNeedsLayout() } }
    var horizontalInset: CGFloat = 0 { didSet { setNeedsLayout() } }
    var metrics = KeyboardMetrics.portrait { didSet { setNeedsLayout() } }

    init(loudness: @escaping () -> Float) {
        var tuning = DotScopeTuning.bar(leadingInset: 0, trailingInset: 0, fade: 36, spacing: 6.5)
        tuning.height = 120
        // Cap drawables at 2 inside the keyboard extension to save ~2.2 MB of IOSurface RAM.
        scope = DotScopeHostView(tuning: tuning, maximumDrawableCount: 2)
        super.init(frame: .zero)
        scope.drawPlate = false
        scope.loudnessProvider = loudness
        scope.reduceMotion = UIAccessibility.isReduceMotionEnabled
        scope.useDemoVoice = ProcessInfo.processInfo.arguments.contains("--snap-state")

        dot.layer.cornerRadius = 3.5
        clockLabel.font = .monospacedDigitSystemFont(ofSize: 15, weight: .medium)
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
        doneButton.title = "Done"
        doneButton.isPrimary = true
        doneButton.accessibilityLabel = "Done"
        doneButton.addTarget(self, action: #selector(done), for: .touchUpInside)

        for view in [dot, clockLabel, styleButton, well, cancelButton, doneButton] {
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

    func update(phase: DotScopePhase, clock: String, style: DictationStyle) {
        isTranscribing = phase == .processing
        scope.phase = phase
        dot.isHidden = isTranscribing
        clockLabel.text = isTranscribing ? "Transcribing" : clock
        clockLabel.textColor = isTranscribing ? palette.textMuted : palette.textBase
        doneButton.title = isTranscribing ? "Inserting…" : "Done"
        doneButton.isPrimary = !isTranscribing
        doneButton.isEnabled = !isTranscribing
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
        dot.backgroundColor = palette.recordingDot
        clockLabel.textColor = isTranscribing ? palette.textMuted : palette.textBase
        styleButton.setTitleColor(palette.textMuted, for: .normal)
        well.backgroundColor = palette.wellFill
        scope.isDark = palette.isDark
        cancelButton.palette = palette
        doneButton.palette = palette
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        let innerLeft = horizontalInset + metrics.side
        let innerRight = bounds.width - horizontalInset - metrics.side
        let innerW = max(120, innerRight - innerLeft)
        let headerTop = metrics.stripTop
        let headerH = metrics.stripHeight
        var x = innerLeft + 8
        if !dot.isHidden {
            dot.frame = CGRect(x: x, y: headerTop + headerH / 2 - 3.5, width: 7, height: 7)
            x += 7 + 6
        }
        clockLabel.sizeToFit()
        clockLabel.frame = CGRect(x: x, y: headerTop, width: clockLabel.bounds.width, height: headerH)
        styleButton.sizeToFit()
        let styleW = styleButton.bounds.width + 12
        styleButton.frame = CGRect(x: innerRight - styleW, y: headerTop, width: styleW, height: headerH)

        let keyH = metrics.keyHeight
        let bottomY = bounds.height - metrics.keysBottom - keyH
        let cancelW = sideKeyWidth * 2 + metrics.gap
        cancelButton.cornerRadius = metrics.cornerRadius
        doneButton.cornerRadius = metrics.cornerRadius
        cancelButton.frame = CGRect(x: innerLeft, y: bottomY, width: cancelW, height: keyH)
        doneButton.frame = CGRect(
            x: innerLeft + cancelW + metrics.gap,
            y: bottomY,
            width: innerW - cancelW - metrics.gap,
            height: keyH
        )

        let wellTop = headerTop + headerH + 6
        well.frame = CGRect(x: innerLeft, y: wellTop, width: innerW, height: max(24, bottomY - 8 - wellTop))
        scope.frame = well.bounds
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

    override func draw(_ rect: CGRect) {
        guard let context = UIGraphicsGetCurrentContext() else { return }
        let fill: UIColor = isHighlighted
            ? palette.keyPressedFill
            : (isPrimary ? palette.keyFill : palette.functionFill)
        context.fillKeycap(bounds, fill: fill, radius: cornerRadius)
        let ink = isEnabled ? palette.textBase : palette.textMuted
        if let symbol {
            let config = UIImage.SymbolConfiguration(pointSize: 16.5, weight: .medium)
            if let image = UIImage(systemName: symbol, withConfiguration: config)?
                .withTintColor(ink, renderingMode: .alwaysOriginal) {
                image.draw(at: CGPoint(x: bounds.midX - image.size.width / 2, y: bounds.midY - image.size.height / 2))
            }
        } else if let title {
            let attributes: [NSAttributedString.Key: Any] = [
                .font: UIFont.systemFont(ofSize: 16.5, weight: .semibold),
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

/// Holding the Hex key: dictation styles, then "Return to" for the app Hex goes back to.
@MainActor
final class HexStyleMenuView: UIView {
    enum Page { case styles, returnApps }

    var palette = FullKeyboardPalette(isDark: false) { didSet { rebuild() } }
    var style: DictationStyle = .polished { didSet { rebuild() } }
    var returnAppID: String? { didSet { rebuild() } }
    var quickReturnApps: [HostApp] = HostAppCatalog.quickChips { didSet { if quickReturnApps != oldValue { rebuild() } } }
    var onStyle: ((DictationStyle) -> Void)?
    var onReturnApp: ((String?) -> Void)?
    var onDismiss: (() -> Void)?
    var onHoverChanged: (() -> Void)?
    /// The Hex key's keycap, in this view's coordinates; the panel opens above it.
    var anchor: CGRect = .zero { didSet { setNeedsLayout() } }

    private var page: Page = .styles
    private let panel = UIView()
    private var rows: [MenuRow] = []
    private var hoveredIndex: Int?

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
        accessibilityViewIsModal = true
        rebuild()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    func show(page: Page) {
        self.page = page
        hoveredIndex = nil
        rebuild()
    }

    /// Updates row highlight while the user holds the Hex key and slides into the menu.
    func updateDrag(at point: CGPoint) {
        let local = convert(point, to: panel)
        let nextIndex = rows.firstIndex { $0.frame.contains(local) }
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
        let local = convert(point, to: panel)
        if let idx = rows.firstIndex(where: { $0.frame.contains(local) }) {
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
            let appName = HostAppCatalog.app(for: returnAppID)?.name ?? "None"
            rows.append(makeRow(title: "Return to", detail: appName, chevron: true, selected: false, showsTopDivider: true) { [weak self] in
                self?.show(page: .returnApps)
            })
        case .returnApps:
            rows.append(makeRow(title: "None", selected: returnAppID == nil) { [weak self] in
                self?.onReturnApp?(nil)
            })
            var apps = quickReturnApps.isEmpty ? HostAppCatalog.quickChips : quickReturnApps
            if let current = HostAppCatalog.app(for: returnAppID), !apps.contains(current) {
                apps.insert(current, at: 0)
            }
            for app in apps {
                rows.append(makeRow(title: app.name, selected: app.id == returnAppID) { [weak self] in
                    self?.onReturnApp?(app.id)
                })
            }
        }
        rows.forEach(panel.addSubview)
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
        let available = max(80, anchor.minY - 6 - 4)
        let rowH = min(36, (available - 2 * padding) / CGFloat(max(1, rows.count)))
        let height = CGFloat(rows.count) * rowH + 2 * padding
        // Opens toward the middle of the keyboard: the Hex key sits right of space.
        let x = min(bounds.width - 6 - width, max(6, anchor.maxX - width))
        panel.frame = CGRect(x: x, y: max(2, anchor.minY - 6 - height), width: width, height: height)
        for (i, row) in rows.enumerated() {
            row.frame = CGRect(x: padding, y: padding + CGFloat(i) * rowH, width: width - 2 * padding, height: rowH)
        }
    }

    private final class MenuRow: UIControl {
        private let palette: FullKeyboardPalette
        private let title: String
        private let detail: String?
        private let chevron: Bool
        private let isChosen: Bool
        private let showsTopDivider: Bool
        private let onTrigger: () -> Void
        var isSlideHovered = false { didSet { if isSlideHovered != oldValue { setNeedsDisplay() } } }

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

        @objc private func redraw() { setNeedsDisplay() }

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
