import UIKit

/// The strip while a two-thumb selection is made: the text near the range with the selected
/// part highlighted, then, once both thumbs lift, Delete · Cut · Copy · Dictate · Cancel.
/// A keyboard can't draw in the host, so this is where the range shows.
@MainActor
final class TrackpadSelectionStripView: UIView {
    var palette = FullKeyboardPalette(isDark: false) {
        didSet { if palette != oldValue { applyPalette() } }
    }
    /// Copy and Cut write the general pasteboard, which a keyboard can only do with Full
    /// Access; without it they aren't offered, as the strip never offers the clipboard.
    var canUseClipboard = true {
        didSet { if canUseClipboard != oldValue { refresh() } }
    }
    var onAction: ((TrackpadSelectionSession.Action) -> Void)?

    private let textLabel = UILabel()
    private var buttons: [(action: TrackpadSelectionSession.Action, button: UIButton)] = []
    private var presentation: TrackpadSelectionSession.Presentation?

    override init(frame: CGRect) {
        super.init(frame: frame)
        isHidden = true
        isOpaque = false
        backgroundColor = .clear
        contentMode = .redraw
        isUserInteractionEnabled = false

        textLabel.font = .systemFont(ofSize: 13.5, weight: .regular)
        textLabel.lineBreakMode = .byTruncatingTail
        textLabel.isAccessibilityElement = true
        addSubview(textLabel)

        let actions: [(TrackpadSelectionSession.Action, String)] = [
            (.delete, String(localized: "Delete")),
            (.cut, String(localized: "Cut")),
            (.copy, String(localized: "Copy")),
            (.dictate, String(localized: "Dictate")),
            (.cancel, String(localized: "Cancel")),
        ]
        buttons = actions.map { item -> (action: TrackpadSelectionSession.Action, button: UIButton) in
            let button = UIButton(type: .system)
            button.setTitle(item.1, for: .normal)
            button.titleLabel?.font = .systemFont(ofSize: 13.5, weight: .medium)
            button.addTarget(self, action: #selector(tapped(_:)), for: .touchUpInside)
            addSubview(button)
            return (action: item.0, button: button)
        }
        applyPalette()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    /// Shows the range while both thumbs are down, or its actions once they lift; nil hides
    /// the strip and its text (the keyboard's own strip comes back).
    func update(_ next: TrackpadSelectionSession.Presentation?, canUseClipboard: Bool) {
        let changed = next != presentation
        presentation = next
        self.canUseClipboard = canUseClipboard
        if changed { refresh() }
    }

    @objc private func tapped(_ sender: UIButton) {
        guard let match = buttons.first(where: { $0.button === sender }) else { return }
        onAction?(match.action)
    }

    private func refresh() {
        guard let presentation else {
            isHidden = true
            isUserInteractionEnabled = false
            return
        }
        isHidden = false
        let actions = presentation.phase == .actions
        isUserInteractionEnabled = actions
        textLabel.isHidden = actions
        for (action, button) in buttons {
            button.isHidden = !actions || (action == .cut || action == .copy) && !canUseClipboard
        }
        if actions {
            setNeedsLayout()
        } else {
            textLabel.attributedText = attributed(presentation)
            textLabel.accessibilityLabel = presentation.hasSelection
                ? String(localized: "Selected: \(presentation.selected)")
                : String(localized: "No text selected")
            textLabel.frame = bounds.insetBy(dx: 12, dy: 0)
        }
        setNeedsLayout()
        setNeedsDisplay()
    }

    private func attributed(_ presentation: TrackpadSelectionSession.Presentation) -> NSAttributedString {
        let font = UIFont.systemFont(ofSize: 13.5, weight: .regular)
        let text = NSMutableAttributedString(string: presentation.before, attributes: [
            .font: font,
            .foregroundColor: palette.stripText,
        ])
        if !presentation.selected.isEmpty {
            text.append(NSAttributedString(string: presentation.selected, attributes: [
                .font: font,
                .foregroundColor: palette.textBase,
                .backgroundColor: palette.actionFill.withAlphaComponent(palette.isDark ? 0.42 : 0.28),
            ]))
        }
        text.append(NSAttributedString(string: presentation.after, attributes: [
            .font: font,
            .foregroundColor: palette.stripText,
        ]))
        return text
    }

    private var visibleButtons: [UIButton] {
        buttons
            .filter { !(($0.action == .cut || $0.action == .copy) && !canUseClipboard) }
            .map { $0.button }
    }

    private func applyPalette() {
        textLabel.attributedText = presentation.map(attributed)
        for (action, button) in buttons {
            button.setTitleColor(action == .delete ? .systemRed : palette.textBase, for: .normal)
        }
        setNeedsDisplay()
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        guard presentation?.phase == .actions else { return }
        let buttons = visibleButtons
        guard !buttons.isEmpty else { return }
        let width = bounds.width / CGFloat(buttons.count)
        for (index, button) in buttons.enumerated() {
            button.frame = CGRect(
                x: bounds.minX + CGFloat(index) * width,
                y: bounds.minY,
                width: width,
                height: bounds.height
            )
        }
    }

    override func draw(_ rect: CGRect) {
        super.draw(rect)
        guard presentation?.phase == .actions else { return }
        let buttons = visibleButtons
        guard buttons.count > 1 else { return }
        let scale = window?.screen.scale ?? traitCollection.displayScale
        let divider = 1 / max(1, scale)
        palette.stripDivider.setFill()
        for index in 1..<buttons.count {
            let x = buttons[index].frame.minX - divider / 2
            UIRectFill(CGRect(x: x, y: bounds.midY - 8, width: divider, height: 16))
        }
    }
}
