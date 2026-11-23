import UIKit

extension UIColor {
    /// A fill no one can see (0.1% black) that makes the area under it take touches. iOS hands a
    /// keyboard extension only the touches that land on something its layers draw: on the emoji
    /// panel's clear background a touch between two emoji, between two rows or beside a category
    /// icon never reached the keyboard at all (iOS 27.2 simulator, iPhone 17 Pro and 15 Pro; a
    /// keyboard of opaque cells and an app's own grid lost none), so a swipe that started there
    /// did nothing and a glide couldn't be caught (TestFlight build 147: "the animation in the emoji
    /// section is not cancelable"). Any fill at all is enough.
    static let keyboardTouchFill = UIColor(white: 0, alpha: 0.001)
}

extension UIScrollView {
    /// iOS 26 softens and blurs a scroll view's edges where it meets the keyboard's glass.
    /// On a real keyboard the top and bottom effects blurred every emoji in the 44 pt rows,
    /// and on the grid the section names and the columns at either side (TestFlight build
    /// 32); the app's snapshot scenes, outside a keyboard, never showed it. Nothing sits over
    /// these scroll views, so they need no edge effect.
    func hideEdgeEffects() {
        guard #available(iOS 26.0, *) else { return }
        for edge in [topEdgeEffect, bottomEdgeEffect, leftEdgeEffect, rightEdgeEffect] {
            edge.isHidden = true
        }
    }
}

/// One emoji in the grid or a row, its bitmap from the panel's `EmojiGlyphRenderer`, with a
/// pressed fill.
@MainActor
final class EmojiCell: UICollectionViewCell {
    static let reuseID = "emoji"
    private let glyphLayer = CALayer()
    private var glyph: EmojiGlyph?
    private var pressedFill: UIColor = .clear
    /// What the cell shows, for tests.
    private(set) var emoji: String?

    override init(frame: CGRect) {
        super.init(frame: frame)
        glyphLayer.actions = ["contents": NSNull(), "bounds": NSNull(), "position": NSNull()]
        contentView.layer.addSublayer(glyphLayer)
        contentView.layer.cornerRadius = 8
        contentView.layer.cornerCurve = .continuous
        isAccessibilityElement = true
        accessibilityTraits = .keyboardKey
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    func configure(
        emoji: String,
        name: String,
        fontSize: CGFloat,
        palette: FullKeyboardPalette,
        renderer: EmojiGlyphRenderer,
        scale: CGFloat
    ) {
        self.emoji = emoji
        glyph = renderer.glyph(for: emoji, size: fontSize, scale: scale)
        glyphLayer.contents = glyph?.image
        glyphLayer.contentsScale = max(1, scale)
        accessibilityLabel = name
        pressedFill = palette.keyPressedFill
        contentView.backgroundColor = isHighlighted ? pressedFill : .clear
        setNeedsLayout()
    }

    override var isHighlighted: Bool {
        didSet { contentView.backgroundColor = isHighlighted ? pressedFill : .clear }
    }

    /// Off screen: drop the bitmap (the renderer's cache keeps the recent ones).
    func clear() {
        glyph = nil
        emoji = nil
        glyphLayer.contents = nil
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        guard let glyph else { return }
        // Where a centred label would draw it, on whole pixels.
        let scale = max(1, glyphLayer.contentsScale)
        let bounds = contentView.bounds
        let x = ((bounds.midX + glyph.rect.minX) * scale).rounded() / scale
        let y = ((bounds.midY + glyph.rect.minY) * scale).rounded() / scale
        glyphLayer.frame = CGRect(x: x, y: y, width: glyph.rect.width, height: glyph.rect.height)
    }
}

/// A section's name over the grid ("Frequently used", "Food and drink").
@MainActor
final class EmojiSectionHeader: UICollectionReusableView {
    static let reuseID = "header"
    let label = UILabel()
    /// How far the name has moved right to stay in view.
    var shift: CGFloat = 0 {
        didSet { if shift != oldValue { setNeedsLayout() } }
    }

    override init(frame: CGRect) {
        super.init(frame: frame)
        label.font = .systemFont(ofSize: 13, weight: .semibold)
        label.isAccessibilityElement = false
        addSubview(label)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    override func layoutSubviews() {
        super.layoutSubviews()
        label.frame = CGRect(x: 10 + shift, y: 0, width: max(0, bounds.width - 10 - shift), height: bounds.height)
    }

    override func prepareForReuse() {
        super.prepareForReuse()
        shift = 0
    }
}

/// A horizontal row of emoji: the panel's suggestions, or the search's results.
@MainActor
final class EmojiRowView: UIView, UICollectionViewDataSource, UICollectionViewDelegate {
    var palette = FullKeyboardPalette(isDark: false) {
        didSet {
            guard palette != oldValue else { return }
            applyPalette()
        }
    }
    /// What the row says when it has nothing to show.
    var emptyText: String? {
        didSet { refreshEmpty() }
    }
    var onPick: ((Int) -> Void)?
    var onHighlight: ((Int?) -> Void)?
    var fontSize: CGFloat = 27 {
        didSet { if fontSize != oldValue { collection.reloadData() } }
    }

    private(set) var items: [(emoji: String, name: String)] = []
    /// Draws the emoji; the panel shares its own.
    var renderer: EmojiGlyphRenderer?
    private lazy var ownRenderer = EmojiGlyphRenderer()
    private let layout = UICollectionViewFlowLayout()
    private let collection: UICollectionView
    private let emptyLabel = UILabel()

    override init(frame: CGRect) {
        layout.scrollDirection = .horizontal
        layout.minimumLineSpacing = 0
        layout.minimumInteritemSpacing = 0
        collection = UICollectionView(frame: .zero, collectionViewLayout: layout)
        super.init(frame: frame)
        // The search's results sit outside the panel: their gaps take touches too.
        backgroundColor = .keyboardTouchFill
        collection.backgroundColor = .clear
        collection.showsHorizontalScrollIndicator = false
        collection.delaysContentTouches = false
        collection.isPrefetchingEnabled = false
        collection.alwaysBounceHorizontal = false
        collection.hideEdgeEffects()
        collection.dataSource = self
        collection.delegate = self
        collection.register(EmojiCell.self, forCellWithReuseIdentifier: EmojiCell.reuseID)
        addSubview(collection)
        emptyLabel.font = .systemFont(ofSize: 14)
        emptyLabel.textAlignment = .center
        emptyLabel.isHidden = true
        addSubview(emptyLabel)
        applyPalette()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    func setItems(_ items: [(emoji: String, name: String)]) {
        self.items = items
        collection.reloadData()
        collection.setContentOffset(.zero, animated: false)
        refreshEmpty()
    }

    private func refreshEmpty() {
        emptyLabel.text = emptyText
        emptyLabel.isHidden = !items.isEmpty || emptyText == nil
    }

    private func applyPalette() {
        emptyLabel.textColor = palette.textMuted
        collection.reloadData()
    }

    /// The cell width that fits whole emoji across the row, about 42 pt.
    var cellWidth: CGFloat {
        let count = max(1, (bounds.width / 42).rounded(.down))
        return bounds.width / count
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        collection.frame = bounds
        emptyLabel.frame = bounds
        let size = CGSize(width: cellWidth, height: max(1, bounds.height - 8))
        if layout.itemSize != size {
            layout.itemSize = size
            layout.sectionInset = UIEdgeInsets(top: 4, left: 0, bottom: 4, right: 0)
            layout.invalidateLayout()
        }
    }

    func collectionView(_ collectionView: UICollectionView, numberOfItemsInSection section: Int) -> Int {
        items.count
    }

    func collectionView(_ collectionView: UICollectionView, cellForItemAt indexPath: IndexPath) -> UICollectionViewCell {
        let cell = collectionView.dequeueReusableCell(withReuseIdentifier: EmojiCell.reuseID, for: indexPath)
        if let cell = cell as? EmojiCell {
            let item = items[indexPath.item]
            cell.configure(
                emoji: item.emoji,
                name: item.name,
                fontSize: fontSize,
                palette: palette,
                renderer: renderer ?? ownRenderer,
                scale: collectionView.traitCollection.displayScale
            )
        }
        return cell
    }

    func collectionView(_ collectionView: UICollectionView, didSelectItemAt indexPath: IndexPath) {
        collectionView.deselectItem(at: indexPath, animated: false)
        onPick?(indexPath.item)
    }

    func collectionView(_ collectionView: UICollectionView, didEndDisplaying cell: UICollectionViewCell, forItemAt indexPath: IndexPath) {
        (cell as? EmojiCell)?.clear()
    }

    func collectionView(_ collectionView: UICollectionView, didHighlightItemAt indexPath: IndexPath) {
        onHighlight?(indexPath.item)
    }

    func collectionView(_ collectionView: UICollectionView, didUnhighlightItemAt indexPath: IndexPath) {
        onHighlight?(nil)
    }

    /// Where an item sits, in this view's coordinates.
    func frame(ofItem index: Int) -> CGRect? {
        guard let attributes = collection.layoutAttributesForItem(at: IndexPath(item: index, section: 0)) else { return nil }
        return collection.convert(attributes.frame, to: self)
    }
}

/// The bar under the grid: one icon per section, the current one on a chip. It tracks the
/// finger itself, as the system emoji keyboard's does (black box, iOS 27.2 simulator): a tap
/// on a category jumps the grid to its start when the finger lifts, and a finger that slides
/// along the bar scrubs the grid live, each icon's slot running through its own section from
/// start to end. The panel sends it every touch in the band under the grid.
@MainActor
final class EmojiCategoryBar: UIView {
    var palette = FullKeyboardPalette(isDark: false) { didSet { if palette != oldValue { refresh() } } }
    /// A tap picked the category.
    var onSelect: ((Int) -> Void)?
    /// A slide is over the category, this far (0 to 1) across its slot.
    var onScrub: ((Int, CGFloat) -> Void)?
    /// A slide moved onto another category (a light tick).
    var onScrubStep: (() -> Void)?
    var selected = 0 {
        didSet { if selected != oldValue { refresh() } }
    }

    /// Whether a finger is sliding along the bar: the grid follows it, not the other way.
    private(set) var isScrubbing = false
    /// How far (points) a finger moves along the bar before a tap becomes a slide.
    static let scrubThreshold: CGFloat = 8
    private var touchStartX: CGFloat?

    private final class BarButton: UIButton {
        var activate: (() -> Void)?

        override func accessibilityActivate() -> Bool {
            activate?()
            return true
        }
    }

    private var buttons: [BarButton] = []
    private let chip = UIView()
    /// A category's slot narrower than this shrinks its icon.
    static let narrowSlot: CGFloat = 21
    private var iconPointSize: CGFloat = 17

    init(items: [(symbol: String, title: String)]) {
        super.init(frame: .zero)
        isMultipleTouchEnabled = false
        chip.isUserInteractionEnabled = false
        chip.layer.cornerCurve = .continuous
        addSubview(chip)
        for (index, item) in items.enumerated() {
            let button = BarButton(type: .custom)
            button.tag = index
            button.accessibilityLabel = item.title
            button.setImage(
                UIImage(systemName: item.symbol, withConfiguration: UIImage.SymbolConfiguration(pointSize: 17, weight: .medium)),
                for: .normal
            )
            // The bar takes the touches; the buttons draw and speak (VoiceOver activates them).
            button.isUserInteractionEnabled = false
            button.activate = { [weak self] in self?.onSelect?(index) }
            addSubview(button)
            buttons.append(button)
        }
        refresh()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    /// The section whose slot is under `x` (the bar's coordinates; outside it, the nearest),
    /// and how far across that slot `x` is.
    func slot(atX x: CGFloat) -> (index: Int, fraction: CGFloat) {
        guard !buttons.isEmpty, bounds.width > 0 else { return (0, 0) }
        let width = bounds.width / CGFloat(buttons.count)
        let position = min(max(0, x), bounds.width - 0.001) / width
        let index = min(buttons.count - 1, Int(position))
        return (index, position - CGFloat(index))
    }

    // MARK: - Touches (UIKit's, or a test's)

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let touch = touches.first else { return }
        touchBegan(atX: touch.location(in: self).x)
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let touch = touches.first else { return }
        touchMoved(toX: touch.location(in: self).x)
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let touch = touches.first else { return }
        touchEnded(atX: touch.location(in: self).x)
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        touchCancelled()
    }

    func touchBegan(atX x: CGFloat) {
        touchStartX = x
        isScrubbing = false
    }

    func touchMoved(toX x: CGFloat) {
        guard let start = touchStartX else { return }
        if !isScrubbing {
            guard abs(x - start) > Self.scrubThreshold else { return }
            isScrubbing = true
        }
        let (index, fraction) = slot(atX: x)
        if index != selected {
            selected = index
            onScrubStep?()
        }
        onScrub?(index, fraction)
    }

    func touchEnded(atX x: CGFloat) {
        defer { touchCancelled() }
        guard touchStartX != nil, !isScrubbing else { return }
        onSelect?(slot(atX: x).index)
    }

    func touchCancelled() {
        touchStartX = nil
        isScrubbing = false
    }

    private func refresh() {
        chip.backgroundColor = palette.chipFill
        for (index, button) in buttons.enumerated() {
            let isSelected = index == selected
            // The chip marks the section; the icons stay one grey, as the system keyboard's.
            button.tintColor = palette.textMuted
            button.accessibilityTraits = isSelected ? [.button, .selected] : .button
        }
        setNeedsLayout()
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        guard !buttons.isEmpty else { return }
        let width = bounds.width / CGFloat(buttons.count)
        // The icons are 17 pt in a slot of 22 pt or more (every iPhone); in a narrower one (an
        // iPad's 320 pt floating keyboard, 18.6 pt slots) they shrink with it so they don't touch.
        let pointSize = width < Self.narrowSlot ? max(12, (17 * width / 22 * 2).rounded(.down) / 2) : 17
        let resize = pointSize != iconPointSize
        iconPointSize = pointSize
        for (index, button) in buttons.enumerated() {
            button.frame = CGRect(x: CGFloat(index) * width, y: 0, width: width, height: bounds.height)
            if resize {
                button.setPreferredSymbolConfiguration(UIImage.SymbolConfiguration(pointSize: pointSize, weight: .medium), forImageIn: .normal)
            }
        }
        let side = min(30, bounds.height - 4, width - 2)
        let selectedFrame = buttons[min(selected, buttons.count - 1)].frame
        chip.frame = CGRect(x: selectedFrame.midX - side / 2, y: selectedFrame.midY - side / 2, width: side, height: side)
        chip.layer.cornerRadius = side / 2
    }
}

/// The search field that takes the suggestion strip's place while you search emoji: a
/// magnifier, what you've typed (or "Search emoji"), a caret, and a button that ends the search.
@MainActor
final class EmojiSearchField: UIView {
    var palette = FullKeyboardPalette(isDark: false) { didSet { if palette != oldValue { refresh() } } }
    var query = "" { didSet { if query != oldValue { refresh() } } }
    var onClose: (() -> Void)?

    private let well = UIView()
    private let icon = UIImageView()
    private let label = UILabel()
    private let caret = UIView()
    private let closeButton = UIButton(type: .custom)

    override init(frame: CGRect) {
        super.init(frame: frame)
        well.layer.cornerCurve = .continuous
        addSubview(well)
        icon.image = UIImage(systemName: "magnifyingglass", withConfiguration: UIImage.SymbolConfiguration(pointSize: 14, weight: .medium))
        icon.contentMode = .center
        addSubview(icon)
        label.font = .systemFont(ofSize: 16)
        label.lineBreakMode = .byTruncatingHead
        addSubview(label)
        addSubview(caret)
        closeButton.setImage(
            UIImage(systemName: "xmark.circle.fill", withConfiguration: UIImage.SymbolConfiguration(pointSize: 17, weight: .regular)),
            for: .normal
        )
        closeButton.accessibilityLabel = "Close search"
        closeButton.addTarget(self, action: #selector(close), for: .touchUpInside)
        addSubview(closeButton)
        isAccessibilityElement = false
        label.accessibilityTraits = .searchField
        refresh()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    @objc private func close() {
        onClose?()
    }

    private func refresh() {
        well.backgroundColor = palette.chipFill
        icon.tintColor = palette.textMuted
        closeButton.tintColor = palette.textMuted
        caret.backgroundColor = palette.actionFill
        label.text = query.isEmpty ? "Search emoji" : query
        label.textColor = query.isEmpty ? palette.textMuted : palette.textBase
        label.accessibilityLabel = "Search emoji"
        label.accessibilityValue = query
        setNeedsLayout()
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        let height = min(36, bounds.height - 8)
        let frame = CGRect(x: 6, y: (bounds.height - height) / 2, width: bounds.width - 12, height: height)
        well.frame = frame
        well.layer.cornerRadius = height / 2
        icon.frame = CGRect(x: frame.minX + 6, y: frame.minY, width: 24, height: height)
        closeButton.frame = CGRect(x: frame.maxX - 40, y: frame.minY, width: 40, height: height)
        let textX = icon.frame.maxX + 4
        let maxWidth = closeButton.frame.minX - textX - 6
        let size = label.sizeThatFits(CGSize(width: maxWidth, height: height))
        // The caret sits after the query, or at the start of the placeholder.
        let textWidth = min(maxWidth, size.width)
        label.frame = CGRect(x: query.isEmpty ? textX + 3 : textX, y: frame.minY, width: textWidth, height: height)
        let caretX = query.isEmpty ? textX : label.frame.maxX + 1
        caret.frame = CGRect(x: caretX, y: frame.midY - 10, width: 2, height: 20)
    }
}
