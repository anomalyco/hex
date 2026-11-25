import UIKit

/// The emoji panel (Settings › Keyboard › Emoji), in place of the strip and the keys: a row of
/// emoji for what you just typed, then a grid that scrolls sideways through what you use
/// most and every category, and a bar with ABC, the categories and delete. Holding an emoji
/// that has skin tones offers them; the one you pick is remembered for it.
@MainActor
final class EmojiPanelView: UIView, UICollectionViewDataSource, UICollectionViewDelegate {
    struct Section {
        let title: String
        let symbol: String
        let entries: [Int]
    }

    var palette = FullKeyboardPalette(isDark: false) {
        didSet {
            guard palette != oldValue else { return }
            applyPalette()
        }
    }
    var showsKeyPopups = true
    var metrics = KeyboardMetrics.portrait { didSet { if metrics != oldValue { setNeedsLayout() } } }
    var horizontalInset: CGFloat = 0 { didSet { if horizontalInset != oldValue { setNeedsLayout() } } }
    /// The full keyboard's current emoji key, in the panel's coordinates, when it has one: only
    /// its x and width are used. ABC takes both, under the thumb that just lifted off the key
    /// (Settings › Labs › ABC under the emoji key); nil keeps ABC at the bar's leading
    /// end, as on the dictation bar and in a panel a fixture shows alone.
    var emojiKeyFrame: CGRect? {
        didSet {
            guard emojiKeyFrame != oldValue else { return }
            setNeedsLayout()
        }
    }
    /// An emoji went in: the text to insert, its entry, and where it came from
    /// ("suggestion", "grid", "tone").
    var onInsert: ((String, Int, String) -> Void)?
    var onDelete: (() -> Void)?
    /// The delete key is still held: how long, for repeat and word delete.
    var onDeleteRepeat: ((TimeInterval) -> Void)?
    var onLetters: (() -> Void)?
    /// On the dictation bar, which has no letter keys, the ABC key goes back to the bar: it
    /// shows the bar's back chevron (More languages' "Back") instead of ABC.
    var returnsToBar = false {
        didSet {
            guard returnsToBar != oldValue else { return }
            lettersKey.title = returnsToBar ? nil : "ABC"
            lettersKey.symbol = returnsToBar ? "chevron.left" : nil
            lettersKey.accessibilityLabel = returnsToBar ? String(localized: "Back") : "Letters"
        }
    }
    var onSearch: (() -> Void)?
    /// The search button, whose search the letter keys type into: the dictation bar, which has
    /// none, hides it and the suggestions take its room.
    var showsSearch = true {
        didSet {
            guard showsSearch != oldValue else { return }
            searchButton.isHidden = !showsSearch
            setNeedsLayout()
        }
    }
    /// A key went down (click and haptic), or a tone was stepped over.
    var onKeyDown: (() -> Void)?
    var onSelectionStep: (() -> Void)?

    let library: EmojiLibrary
    let usage: EmojiUsageStore
    /// The panel's emoji bitmaps, shared with its suggestion row and the search's results,
    /// and gone with the panel.
    let glyphs = EmojiGlyphRenderer()
    private(set) var sections: [Section] = []
    private(set) var suggestions: [Int] = []

    let suggestionRow = EmojiRowView()
    private let searchButton = UIButton(type: .custom)
    private let grid: UICollectionView
    private let categoryBar: EmojiCategoryBar
    private let lettersKey = PanelKeyButton()
    private let deleteKey = PanelKeyButton()
    private let callout = KeyCalloutView()
    private let tonePicker = AccentCalloutView()
    /// A hairline between the suggestion row and the grid.
    private let rowSeparator = UIView()
    private var toneEntry: Int?
    /// An emoji without skin tones held down: typed when the finger lifts over it.
    private var heldEntry: (entry: Int, frame: CGRect)?
    private var layoutKey: GridGeometry?
    private var sectionStarts: [CGFloat] = []
    private var deleteTimer: Timer?
    private var deleteStartedAt: TimeInterval = 0

    struct GridGeometry: Equatable {
        let rows: Int
        let rowHeight: CGFloat
        let columnWidth: CGFloat
        let headerHeight: CGFloat
        let fontSize: CGFloat
        let counts: [Int]
    }

    init(library: EmojiLibrary, usage: EmojiUsageStore) {
        self.library = library
        self.usage = usage
        grid = UICollectionView(frame: .zero, collectionViewLayout: UICollectionViewFlowLayout())
        categoryBar = EmojiCategoryBar(items: [("clock", "Frequently used")] + EmojiCategory.allCases.map { ($0.symbol, $0.title) })
        super.init(frame: .zero)
        // Clear to the eye, but every point of the panel takes touches (`keyboardTouchFill`).
        backgroundColor = .keyboardTouchFill
        isOpaque = false

        searchButton.setImage(
            UIImage(systemName: "magnifyingglass", withConfiguration: UIImage.SymbolConfiguration(pointSize: 15, weight: .medium)),
            for: .normal
        )
        searchButton.accessibilityLabel = "Search emoji"
        searchButton.layer.cornerCurve = .continuous
        searchButton.addTarget(self, action: #selector(searchTapped), for: .touchUpInside)
        addSubview(searchButton)

        suggestionRow.renderer = glyphs
        suggestionRow.onPick = { [weak self] index in self?.pickSuggestion(index) }
        addSubview(suggestionRow)
        rowSeparator.isUserInteractionEnabled = false
        addSubview(rowSeparator)

        grid.backgroundColor = .clear
        grid.showsHorizontalScrollIndicator = false
        grid.delaysContentTouches = false
        // Cells draw their emoji when they're configured; only the visible ones should.
        grid.isPrefetchingEnabled = false
        grid.hideEdgeEffects()
        grid.dataSource = self
        grid.delegate = self
        grid.register(EmojiCell.self, forCellWithReuseIdentifier: EmojiCell.reuseID)
        grid.register(
            EmojiSectionHeader.self,
            forSupplementaryViewOfKind: UICollectionView.elementKindSectionHeader,
            withReuseIdentifier: EmojiSectionHeader.reuseID
        )
        let hold = UILongPressGestureRecognizer(target: self, action: #selector(held(_:)))
        hold.minimumPressDuration = FullKeyboardEngine.accentHoldDelay
        grid.addGestureRecognizer(hold)
        addSubview(grid)

        categoryBar.onSelect = { [weak self] index in self?.jump(toSection: index) }
        categoryBar.onScrub = { [weak self] index, fraction in self?.scrub(toSection: index, fraction: fraction) }
        categoryBar.onScrubStep = { [weak self] in self?.onSelectionStep?() }
        addSubview(categoryBar)

        lettersKey.title = "ABC"
        lettersKey.isPrimary = true
        lettersKey.drawsLikeKeys = true
        lettersKey.isPlain = true
        lettersKey.accessibilityLabel = "Letters"
        lettersKey.addTarget(self, action: #selector(lettersTapped), for: .touchUpInside)
        addSubview(lettersKey)

        deleteKey.symbol = "delete.left"
        deleteKey.isPrimary = true
        deleteKey.drawsLikeKeys = true
        deleteKey.isPlain = true
        deleteKey.accessibilityLabel = "Delete"
        deleteKey.addTarget(self, action: #selector(deleteDown), for: .touchDown)
        deleteKey.addTarget(self, action: #selector(deleteUp), for: [.touchUpInside, .touchUpOutside, .touchCancel])
        addSubview(deleteKey)

        callout.isUserInteractionEnabled = false
        addSubview(callout)
        tonePicker.isHidden = true
        addSubview(tonePicker)
        applyPalette()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    // MARK: - Content

    /// Fills the panel for the text before the cursor and your use: the row's suggestions, and
    /// a full block of Frequently used, as the system keyboard's (your emoji, then common
    /// ones, then the first smileys).
    func fill(before: String?) {
        var scores: [Int: Float] = [:]
        var frequent: [Int] = []
        let capacity = frequentCapacity
        for (emoji, score) in usage.ranked(limit: capacity) {
            guard let entry = library.entryByEmoji[emoji], scores[entry] == nil else { continue }
            scores[entry] = score
            frequent.append(entry)
        }
        let common = EmojiSuggester.defaults.compactMap { library.entryByEmoji[$0] }
            + library.entries.indices.lazy.filter { self.library.entries[$0].category == .smileysPeople }.prefix(capacity)
        for entry in common where frequent.count < capacity && !frequent.contains(entry) {
            frequent.append(entry)
        }
        let context = EmojiContext(before: before, stopwords: library.stopwords)
        show(suggestions: EmojiSuggester.suggestions(context: context, library: library, usage: scores, limit: 10), frequent: frequent)
    }

    /// Fills the panel: `suggestions` for the row, `frequent` for the grid's first section.
    func show(suggestions: [Int], frequent: [Int]) {
        self.suggestions = suggestions
        suggestionRow.setItems(suggestions.map { (display(for: $0), library.name(of: $0)) })
        var sections = [Section(title: "Frequently used", symbol: "clock", entries: frequent)]
        var byCategory = Array(repeating: [Int](), count: EmojiCategory.allCases.count)
        for (index, entry) in library.entries.enumerated() {
            byCategory[entry.category.rawValue].append(index)
        }
        for category in EmojiCategory.allCases {
            sections.append(Section(title: category.title, symbol: category.symbol, entries: byCategory[category.rawValue]))
        }
        self.sections = sections
        layoutKey = nil
        sectionStarts = []
        grid.reloadData()
        setNeedsLayout()
    }

    /// The emoji with the skin tone you last picked for it.
    func display(for entry: Int) -> String {
        let item = library.entries[entry]
        let tone = item.hasTones ? usage.tone(for: item.emoji) : 0
        return tone > 0 && tone <= item.tones.count ? item.tones[tone - 1] : item.emoji
    }

    private func pickSuggestion(_ index: Int) {
        guard index < suggestions.count else { return }
        onKeyDown?()
        let entry = suggestions[index]
        onInsert?(display(for: entry), entry, "suggestion")
    }

    // MARK: - Layout

    private var rowFrame: CGRect {
        CGRect(x: horizontalInset, y: metrics.stripTop, width: bounds.width - 2 * horizontalInset, height: metrics.stripHeight)
    }

    /// The row's band around the strip's words' line: the search button and suggestions centre
    /// in it, where the strip's words were.
    private var rowLineFrame: CGRect {
        let band = metrics.stripLineBand
        return CGRect(x: rowFrame.minX, y: metrics.stripTop + band.top, width: rowFrame.width, height: band.height)
    }

    /// The grid's rows: four on roomy keyboards, three on compact ones.
    var gridRows: Int {
        metrics.keyHeight >= 40 ? 4 : 3
    }

    /// How many emoji Frequently used holds: eight columns of the grid's rows.
    var frequentCapacity: Int {
        gridRows * 8
    }

    /// The suggestion row's and the search results' emoji size.
    var rowFontSize: CGFloat {
        metrics.keyHeight >= 40 ? 27 : 23
    }

    private var barKeyHeight: CGFloat {
        metrics.keyHeight >= 40 ? 38 : metrics.keyHeight
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        let row = rowFrame
        let line = rowLineFrame
        let side = metrics.side
        let buttonSide = min(36, line.height - 8)
        searchButton.frame = CGRect(x: row.minX + side, y: line.midY - buttonSide / 2, width: buttonSide + 4, height: buttonSide)
        searchButton.layer.cornerRadius = 10
        let rowStart = showsSearch ? searchButton.frame.maxX + 4 : row.minX + side
        suggestionRow.frame = CGRect(x: rowStart, y: line.minY, width: row.maxX - side - rowStart, height: line.height)
        let hairline = 1 / max(1, traitCollection.displayScale)
        rowSeparator.frame = CGRect(x: row.minX + side, y: row.maxY - hairline, width: row.width - 2 * side, height: hairline)
        suggestionRow.fontSize = rowFontSize

        let barY = bounds.height - metrics.keysBottom - barKeyHeight
        // ABC and delete are drawn without keys, so they take only their label's room and the
        // categories get the rest.
        let keyWidth = min(44, KeyboardLayout.letterWidth(innerWidth: bounds.width - 2 * horizontalInset, metrics: metrics) * KeyboardLayout.bottomSwitchWidthRatio)
        let lettersFrame = emojiKeyFrame.map {
            CGRect(x: $0.minX, y: barY, width: $0.width, height: barKeyHeight)
        } ?? CGRect(x: horizontalInset + side, y: barY, width: keyWidth, height: barKeyHeight)
        lettersKey.frame = lettersFrame
        deleteKey.frame = CGRect(x: bounds.width - horizontalInset - side - keyWidth, y: barY, width: keyWidth, height: barKeyHeight)
        lettersKey.cornerRadius = metrics.cornerRadius
        deleteKey.cornerRadius = metrics.cornerRadius
        categoryBar.frame = CGRect(x: lettersKey.frame.maxX + 4, y: barY, width: max(0, deleteKey.frame.minX - lettersKey.frame.maxX - 8), height: barKeyHeight)

        let gridTop = row.maxY
        let gridFrame = CGRect(x: horizontalInset, y: gridTop, width: bounds.width - 2 * horizontalInset, height: barY - 4 - gridTop)
        grid.frame = gridFrame
        let band = UIEdgeInsets(top: barY - gridFrame.maxY, left: 0, bottom: max(0, bounds.height - barY - barKeyHeight), right: 0)
        lettersKey.hitOutsets = UIEdgeInsets(top: band.top, left: lettersKey.frame.minX, bottom: band.bottom, right: categoryBar.frame.minX - lettersKey.frame.maxX)
        deleteKey.hitOutsets = UIEdgeInsets(top: band.top, left: deleteKey.frame.minX - categoryBar.frame.maxX, bottom: band.bottom, right: bounds.width - deleteKey.frame.maxX)
        callout.frame = bounds
        tonePicker.frame = bounds
        applyGridLayout(height: gridFrame.height)
    }

    /// The bar's keys take the whole band under the grid: the gap above them and the margin
    /// down to the keyboard's edge, where a thumb aiming at a small icon often lands. Taps
    /// there used to hit nothing, so a category could take two taps. Between ABC and delete
    /// the bar takes it, each category the full width of its slot, so a finger can also slide
    /// along it.
    override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? {
        guard !isHidden, isUserInteractionEnabled, alpha > 0.01, self.point(inside: point, with: event) else { return nil }
        if tonePicker.menu == nil, point.y >= grid.frame.maxY {
            if point.x < categoryBar.frame.minX { return lettersKey }
            if point.x >= categoryBar.frame.maxX { return deleteKey }
            return categoryBar
        }
        return super.hitTest(point, with: event)
    }

    private func applyGridLayout(height: CGFloat) {
        let isRoomy = metrics.keyHeight >= 40
        let headerHeight: CGFloat = isRoomy ? 20 : 16
        let rows = gridRows
        let rowHeight = max(20, (height - headerHeight) / CGFloat(rows))
        let geometry = GridGeometry(
            rows: rows,
            rowHeight: rowHeight,
            // The system emoji keyboard's pitch and emoji size, measured from screenshots.
            columnWidth: isRoomy ? 46 : 42,
            headerHeight: headerHeight,
            fontSize: isRoomy ? 32 : 26,
            counts: sections.map(\.entries.count)
        )
        guard geometry != layoutKey, height > 0 else { return }
        layoutKey = geometry
        sectionStarts = []
        grid.setCollectionViewLayout(Self.makeLayout(geometry), animated: false)
    }

    private static func makeLayout(_ g: GridGeometry) -> UICollectionViewCompositionalLayout {
        let config = UICollectionViewCompositionalLayoutConfiguration()
        config.scrollDirection = .horizontal
        config.interSectionSpacing = 10
        let counts = g.counts
        return UICollectionViewCompositionalLayout(sectionProvider: { index, _ in
            let item = NSCollectionLayoutItem(layoutSize: NSCollectionLayoutSize(
                widthDimension: .fractionalWidth(1),
                heightDimension: .absolute(g.rowHeight)
            ))
            let group = NSCollectionLayoutGroup.vertical(
                layoutSize: NSCollectionLayoutSize(
                    widthDimension: .absolute(g.columnWidth),
                    heightDimension: .absolute(g.rowHeight * CGFloat(g.rows))
                ),
                repeatingSubitem: item,
                count: g.rows
            )
            let section = NSCollectionLayoutSection(group: group)
            let count = counts[safe: index] ?? 0
            let columns = max(1, (count + g.rows - 1) / g.rows)
            // Above the section and as wide as it; scrolling keeps its name in view
            // (keepHeadersInView), as a pinned header would.
            let header = NSCollectionLayoutBoundarySupplementaryItem(
                layoutSize: NSCollectionLayoutSize(
                    widthDimension: .absolute(CGFloat(columns) * g.columnWidth),
                    heightDimension: .absolute(g.headerHeight)
                ),
                elementKind: UICollectionView.elementKindSectionHeader,
                alignment: .top
            )
            section.boundarySupplementaryItems = [header]
            // A horizontal layout doesn't move the items below a top header; the inset does,
            // and the header ignores it.
            section.contentInsets = NSDirectionalEdgeInsets(top: g.headerHeight, leading: 2, bottom: 0, trailing: 2)
            section.supplementaryContentInsetsReference = .none
            return section
        }, configuration: config)
    }

    private func applyPalette() {
        searchButton.backgroundColor = palette.chipFill
        searchButton.tintColor = palette.textMuted
        rowSeparator.backgroundColor = palette.separator
        suggestionRow.palette = palette
        categoryBar.palette = palette
        lettersKey.palette = palette
        deleteKey.palette = palette
        callout.palette = palette
        tonePicker.palette = palette
        grid.reloadData()
    }

    // MARK: - Grid

    func numberOfSections(in collectionView: UICollectionView) -> Int {
        sections.count
    }

    func collectionView(_ collectionView: UICollectionView, numberOfItemsInSection section: Int) -> Int {
        sections[section].entries.count
    }

    func collectionView(_ collectionView: UICollectionView, cellForItemAt indexPath: IndexPath) -> UICollectionViewCell {
        let cell = collectionView.dequeueReusableCell(withReuseIdentifier: EmojiCell.reuseID, for: indexPath)
        if let cell = cell as? EmojiCell {
            let entry = sections[indexPath.section].entries[indexPath.item]
            cell.configure(
                emoji: display(for: entry),
                name: library.name(of: entry),
                fontSize: layoutKey?.fontSize ?? 32,
                palette: palette,
                renderer: glyphs,
                scale: collectionView.traitCollection.displayScale
            )
        }
        return cell
    }

    func collectionView(
        _ collectionView: UICollectionView,
        viewForSupplementaryElementOfKind kind: String,
        at indexPath: IndexPath
    ) -> UICollectionReusableView {
        let view = collectionView.dequeueReusableSupplementaryView(
            ofKind: kind,
            withReuseIdentifier: EmojiSectionHeader.reuseID,
            for: indexPath
        )
        if let header = view as? EmojiSectionHeader {
            header.label.text = sections[indexPath.section].title
            header.label.textColor = palette.textMuted
        }
        return view
    }

    func collectionView(_ collectionView: UICollectionView, didEndDisplaying cell: UICollectionViewCell, forItemAt indexPath: IndexPath) {
        (cell as? EmojiCell)?.clear()
    }

    func collectionView(_ collectionView: UICollectionView, didHighlightItemAt indexPath: IndexPath) {
        // No click here: a swipe through the grid starts on a cell too.
        guard showsKeyPopups, let frame = cellFrame(indexPath) else { return }
        let entry = sections[indexPath.section].entries[indexPath.item]
        callout.show(balloon(over: frame), label: display(for: entry))
    }

    func collectionView(_ collectionView: UICollectionView, didUnhighlightItemAt indexPath: IndexPath) {
        callout.show(nil, label: "")
    }

    func collectionView(_ collectionView: UICollectionView, didSelectItemAt indexPath: IndexPath) {
        collectionView.deselectItem(at: indexPath, animated: false)
        onKeyDown?()
        let entry = sections[indexPath.section].entries[indexPath.item]
        onInsert?(display(for: entry), entry, "grid")
    }

    func scrollViewDidScroll(_ scrollView: UIScrollView) {
        callout.show(nil, label: "")
        keepHeadersInView()
        // While a finger slides along the bar, the bar leads and the grid follows it.
        guard !categoryBar.isScrubbing else { return }
        if sectionStarts.isEmpty { measureSections() }
        let x = scrollView.contentOffset.x + 24
        let current = sectionStarts.lastIndex { $0 <= x } ?? 0
        categoryBar.selected = current
    }

    func collectionView(
        _ collectionView: UICollectionView,
        willDisplaySupplementaryView view: UICollectionReusableView,
        forElementKind elementKind: String,
        at indexPath: IndexPath
    ) {
        keepHeadersInView()
    }

    /// A section's name stays at the grid's left edge while the section scrolls past, and
    /// the next section's name pushes it out.
    private func keepHeadersInView() {
        let left = grid.contentOffset.x
        for case let header as EmojiSectionHeader in grid.visibleSupplementaryViews(ofKind: UICollectionView.elementKindSectionHeader) {
            let room = max(0, header.frame.width - header.label.intrinsicContentSize.width - 16)
            header.shift = min(room, max(0, left - header.frame.minX))
        }
    }

    private func measureSections() {
        sectionStarts = sections.indices.map { section in
            guard !sections[section].entries.isEmpty,
                  let attributes = grid.layoutAttributesForItem(at: IndexPath(item: 0, section: section)) else {
                return .greatestFiniteMagnitude
            }
            return attributes.frame.minX
        }
    }

    private func jump(toSection section: Int) {
        guard section < sections.count else { return }
        onKeyDown?()
        grid.layoutIfNeeded()
        if sectionStarts.isEmpty { measureSections() }
        let start = sectionStarts[section] == .greatestFiniteMagnitude ? 0 : sectionStarts[section]
        let maxX = max(0, grid.contentSize.width - grid.bounds.width)
        grid.setContentOffset(CGPoint(x: min(maxX, max(0, start - 2)), y: 0), animated: false)
        categoryBar.selected = section
    }

    /// A finger sliding along the bar: the grid shows `section` `fraction` of the way
    /// through, live, so the bar scrubs through every emoji.
    private func scrub(toSection section: Int, fraction: CGFloat) {
        guard section < sections.count else { return }
        grid.layoutIfNeeded()
        if sectionStarts.isEmpty { measureSections() }
        let start = sectionStart(section)
        let end = section + 1 < sections.count ? sectionStart(section + 1) : grid.contentSize.width
        let maxX = max(0, grid.contentSize.width - grid.bounds.width)
        let x = start + min(1, max(0, fraction)) * max(0, end - start)
        grid.setContentOffset(CGPoint(x: min(maxX, max(0, x - 2)), y: 0), animated: false)
    }

    /// Where a section's first emoji sits in the grid; an empty one starts where the next does.
    private func sectionStart(_ section: Int) -> CGFloat {
        sectionStarts[section...].first { $0 != .greatestFiniteMagnitude } ?? grid.contentSize.width
    }

    private func cellFrame(_ indexPath: IndexPath) -> CGRect? {
        guard let attributes = grid.layoutAttributesForItem(at: indexPath) else { return nil }
        return grid.convert(attributes.frame, to: self)
    }

    private func balloon(over cell: CGRect) -> KeyBalloon {
        let width = cell.width + KeyBalloon.extraWidth
        let x = max(2, min(bounds.width - 2 - width, cell.midX - width / 2))
        let bubble = CGRect(x: x, y: max(0, cell.minY - KeyBalloon.neck - KeyBalloon.bubbleHeight), width: width, height: KeyBalloon.bubbleHeight)
        return KeyBalloon(keyFrame: cell, bubble: bubble)
    }

    // MARK: - Skin tones

    @objc private func held(_ gesture: UILongPressGestureRecognizer) {
        let point = gesture.location(in: self)
        switch gesture.state {
        case .began:
            callout.show(nil, label: "")
            guard let indexPath = grid.indexPathForItem(at: gesture.location(in: grid)),
                  let frame = cellFrame(indexPath) else { return }
            let entry = sections[indexPath.section].entries[indexPath.item]
            let item = library.entries[entry]
            guard item.hasTones else {
                // Without tones a hold is a slow tap: typed if the finger lifts over it.
                heldEntry = (entry, frame)
                if showsKeyPopups { callout.show(balloon(over: frame), label: display(for: entry)) }
                return
            }
            showTones(for: entry, over: frame)
        case .changed:
            guard var menu = tonePicker.menu else { return }
            let index = menu.index(for: point.x)
            if index != menu.selectedIndex {
                menu.selectedIndex = index
                tonePicker.show(menu)
                onSelectionStep?()
            }
        case .ended:
            callout.show(nil, label: "")
            if let held = heldEntry {
                heldEntry = nil
                if held.frame.insetBy(dx: -4, dy: -4).contains(point) {
                    onKeyDown?()
                    onInsert?(display(for: held.entry), held.entry, "grid")
                }
                return
            }
            guard let menu = tonePicker.menu, let entry = toneEntry else { return }
            let chosen = menu.options[menu.index(for: point.x)]
            pickTone(chosen, for: entry)
        default:
            callout.show(nil, label: "")
            heldEntry = nil
            tonePicker.show(nil)
            toneEntry = nil
        }
    }

    /// Opens the skin tone picker over a cell, the remembered tone selected.
    func showTones(for entry: Int, over frame: CGRect) {
        let item = library.entries[entry]
        // As the system keyboard's: emoji about its grid's size, the untoned one set apart.
        var menu = AccentMenu.build(
            for: frame,
            rawOptions: [item.emoji] + item.tones,
            in: bounds.width,
            horizontalInset: horizontalInset,
            cellWidth: 44
        )
        tonePicker.optionFontSize = 28
        tonePicker.separatedOption = item.emoji
        if let index = menu.options.firstIndex(of: display(for: entry)) { menu.selectedIndex = index }
        toneEntry = entry
        tonePicker.show(menu)
        onSelectionStep?()
    }

    private func pickTone(_ chosen: String, for entry: Int) {
        let item = library.entries[entry]
        let tone = item.tones.firstIndex(of: chosen).map { $0 + 1 } ?? 0
        usage.setTone(tone, for: item.emoji)
        tonePicker.show(nil)
        toneEntry = nil
        onInsert?(chosen, entry, "tone")
        // Every cell showing it takes the new tone.
        grid.reloadData()
        suggestionRow.setItems(suggestions.map { (display(for: $0), library.name(of: $0)) })
    }

    /// Fixtures: hold an emoji in the grid until its tones open.
    func debugShowTones(for emoji: String, selecting tone: Int) {
        grid.layoutIfNeeded()
        guard let entry = library.entryByEmoji[emoji] else { return }
        for (section, content) in sections.enumerated() {
            guard let item = content.entries.firstIndex(of: entry) else { continue }
            let indexPath = IndexPath(item: item, section: section)
            if cellFrame(indexPath).map({ !grid.frame.contains(CGPoint(x: $0.midX, y: $0.midY)) }) ?? true {
                grid.scrollToItem(at: indexPath, at: .centeredHorizontally, animated: false)
                grid.layoutIfNeeded()
            }
            guard let frame = cellFrame(indexPath) else { return }
            showTones(for: entry, over: frame)
            if var menu = tonePicker.menu, let index = menu.options.firstIndex(of: library.entries[entry].tones[tone - 1]) {
                menu.selectedIndex = index
                tonePicker.show(menu)
            }
            return
        }
    }

    /// Fixtures and probes: the section the grid shows first.
    func debugJump(toSection section: Int) {
        jump(toSection: section)
    }

    /// Probes: the grid scrolled to its `page`th screen (clamped to the end).
    func debugScroll(toPage page: Int) {
        grid.layoutIfNeeded()
        let maxX = max(0, grid.contentSize.width - grid.bounds.width)
        grid.setContentOffset(CGPoint(x: min(maxX, CGFloat(page) * grid.bounds.width), y: 0), animated: false)
        grid.layoutIfNeeded()
    }

    /// Probes: the grid scrolled `dx` points on; false once it's at the end.
    func debugScroll(by dx: CGFloat) -> Bool {
        let maxX = max(0, grid.contentSize.width - grid.bounds.width)
        guard grid.contentOffset.x < maxX else { return false }
        grid.setContentOffset(CGPoint(x: min(maxX, grid.contentOffset.x + dx), y: 0), animated: false)
        grid.layoutIfNeeded()
        return true
    }

    // MARK: - Keys

    @objc private func searchTapped() {
        onKeyDown?()
        onSearch?()
    }

    @objc private func lettersTapped() {
        onKeyDown?()
        onLetters?()
    }

    @objc private func deleteDown() {
        onKeyDown?()
        onDelete?()
        deleteTimer?.invalidate()
        deleteStartedAt = CACurrentMediaTime()
        let timer = Timer(timeInterval: FullKeyboardEngine.repeatInterval, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                let held = CACurrentMediaTime() - self.deleteStartedAt
                guard held >= FullKeyboardEngine.repeatDelay else { return }
                self.onDeleteRepeat?(held)
                self.onKeyDown?()
            }
        }
        RunLoop.main.add(timer, forMode: .common)
        deleteTimer = timer
    }

    @objc private func deleteUp() {
        deleteTimer?.invalidate()
        deleteTimer = nil
    }

    func tearDown() {
        deleteUp()
        tonePicker.show(nil)
        callout.show(nil, label: "")
    }
}

private extension Array {
    subscript(safe index: Int) -> Element? {
        indices.contains(index) ? self[index] : nil
    }
}
