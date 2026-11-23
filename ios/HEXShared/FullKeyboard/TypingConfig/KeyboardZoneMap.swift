import CoreGraphics

/// A typing config's zones compiled against one layout: the touch area of every key they
/// change, as disjoint rectangles, and what became of each zone. Every point of the keys'
/// area still belongs to exactly one key, and every key keeps its keycap's centre plus
/// `TypingConfigLimits.zoneKeepFromCentre` of its keycap each way.
nonisolated struct KeyboardZoneMap: Equatable, Sendable {
    /// Touch areas by index into `KeyboardLayout.keys`, only for keys whose area changed.
    var regions: [Int: [CGRect]] = [:]
    /// One per zone, in the order given: nil when it applied, else why it didn't.
    var skipped: [TypingConfigReview.Reason?] = []

    var isEmpty: Bool { regions.isEmpty }
}

nonisolated extension KeyboardLayout {
    /// `zones` against these keys (letter keycaps: `letterWidth` across, `metrics.keyHeight`
    /// down). Zones on an edge they may not move, or on a key this layout lacks, are skipped;
    /// so are both of two zones whose strips meet, and every zone whose strip reaches into a
    /// key that would keep too little.
    func zoneMap(_ zones: [KeyboardZone]) -> KeyboardZoneMap {
        var skipped = [TypingConfigReview.Reason?](repeating: nil, count: zones.count)
        let base = keys.map { [$0.hitFrame] + ($0.gapHitFrame.map { [$0] } ?? []) }
        var strips: [[(rect: CGRect, owner: Int)]] = Array(repeating: [], count: zones.count)
        for (n, zone) in zones.enumerated() {
            let found = zoneStrips(for: zone, base: base)
            strips[n] = found.strips
            skipped[n] = found.reason
        }
        // Two zones claiming the same point: neither applies.
        for a in zones.indices where skipped[a] == nil {
            for b in zones.indices where b > a && skipped[b] == nil {
                let meet = strips[a].contains { s in strips[b].contains { Self.overlaps(s.rect, $0.rect) } }
                if meet {
                    skipped[a] = .twoZonesClaimPoint
                    skipped[b] = .twoZonesClaimPoint
                }
            }
        }
        // Apply, then drop the zones that leave a key too little of its keycap, until none does.
        while true {
            var regions = base
            for n in zones.indices where skipped[n] == nil {
                for strip in strips[n] {
                    for k in regions.indices where k != strip.owner {
                        regions[k] = regions[k].flatMap { Self.subtract(strip.rect, from: $0) }
                    }
                    regions[strip.owner] += regions[strip.owner].reduce([strip.rect]) { pieces, own in
                        pieces.flatMap { Self.subtract(own, from: $0) }
                    }
                }
            }
            let small = keys.indices.filter { k in
                keys[k].row >= 0 && keys[k].frame.width > 0 && !Self.covers(regions[k], keepRect(keys[k]))
            }
            let culprits = zones.indices.filter { n in
                skipped[n] == nil && small.contains { k in strips[n].contains { Self.overlaps($0.rect, keepRect(keys[k])) } }
            }
            if small.isEmpty || culprits.isEmpty {
                var map = KeyboardZoneMap(skipped: skipped)
                for k in regions.indices where regions[k] != base[k] { map.regions[k] = regions[k] }
                return map
            }
            for n in culprits { skipped[n] = .keyTooSmall }
        }
    }

    /// The rectangle around a keycap's centre its key must keep.
    private func keepRect(_ key: KeyboardKey) -> CGRect {
        let keep = TypingConfigLimits.zoneKeepFromCentre
        return CGRect(x: key.frame.midX - keep * key.frame.width, y: key.frame.midY - keep * key.frame.height,
                      width: 2 * keep * key.frame.width, height: 2 * keep * key.frame.height)
    }

    /// The strips one zone moves, each with the key that gets it.
    private func zoneStrips(for zone: KeyboardZone, base: [[CGRect]]) -> (strips: [(rect: CGRect, owner: Int)], reason: TypingConfigReview.Reason?) {
        let letter = zone.key.count == 1 && zone.key.first?.isLetter == true
        guard let edges = TypingConfigLimits.zoneEdges[letter ? "letter" : zone.key], edges.contains(zone.edge) else {
            return ([], .edgeNotAllowed)
        }
        let targets = keys.indices.filter { keys[$0].row >= 0 && !keys[$0].isNumberRow && TypingLogger.keyID(keys[$0].kind) == zone.key }
        guard !targets.isEmpty else { return ([], .keyNotOnKeyboard) }
        let move = max(-TypingConfigLimits.zoneMaxMove, min(TypingConfigLimits.zoneMaxMove, zone.move))
        let vertical = zone.edge == .top || zone.edge == .bottom
        let d = abs(move) * (vertical ? metrics.keyHeight : letterWidth)
        guard d > 0.01 else { return ([], nil) }
        let area = touchAreaFrame
        var out: [(rect: CGRect, owner: Int)] = []
        for i in targets {
            let rects = base[i]
            let bounds = rects.dropFirst().reduce(rects[0]) { $0.union($1) }
            let line: CGFloat = switch zone.edge {
            case .top: bounds.minY
            case .bottom: bounds.maxY
            case .left: bounds.minX
            case .right: bounds.maxX
            }
            let outer = switch zone.edge {
            case .top: line <= area.minY + 0.01
            case .bottom: line >= area.maxY - 0.01
            case .left: line <= area.minX + 0.01
            case .right: line >= area.maxX - 0.01
            }
            if outer { return ([], .edgeNotAllowed) }
            // The key's own stretches of that edge.
            let segments = rects.filter { r in
                switch zone.edge {
                case .top: abs(r.minY - line) < 0.01
                case .bottom: abs(r.maxY - line) < 0.01
                case .left: abs(r.minX - line) < 0.01
                case .right: abs(r.maxX - line) < 0.01
                }
            }
            for s in segments {
                if move > 0 {
                    let grown: CGRect = switch zone.edge {
                    case .top: CGRect(x: s.minX, y: line - d, width: s.width, height: d)
                    case .bottom: CGRect(x: s.minX, y: line, width: s.width, height: d)
                    case .left: CGRect(x: line - d, y: s.minY, width: d, height: s.height)
                    case .right: CGRect(x: line, y: s.minY, width: d, height: s.height)
                    }
                    let clipped = grown.intersection(area)
                    if !clipped.isNull, clipped.width > 0.01, clipped.height > 0.01 { out.append((clipped, i)) }
                } else {
                    // A strip inside the key goes to the key across the edge at each point.
                    let strip: CGRect = switch zone.edge {
                    case .top: CGRect(x: s.minX, y: line, width: s.width, height: d)
                    case .bottom: CGRect(x: s.minX, y: line - d, width: s.width, height: d)
                    case .left: CGRect(x: line, y: s.minY, width: d, height: s.height)
                    case .right: CGRect(x: line - d, y: s.minY, width: d, height: s.height)
                    }
                    for j in base.indices where j != i && keys[j].row >= 0 {
                        for r in base[j] {
                            let touching = switch zone.edge {
                            case .top: abs(r.maxY - line) < 0.01
                            case .bottom: abs(r.minY - line) < 0.01
                            case .left: abs(r.maxX - line) < 0.01
                            case .right: abs(r.minX - line) < 0.01
                            }
                            guard touching else { continue }
                            let piece = vertical
                                ? CGRect(x: max(strip.minX, r.minX), y: strip.minY, width: min(strip.maxX, r.maxX) - max(strip.minX, r.minX), height: strip.height)
                                : CGRect(x: strip.minX, y: max(strip.minY, r.minY), width: strip.width, height: min(strip.maxY, r.maxY) - max(strip.minY, r.minY))
                            if piece.width > 0.01, piece.height > 0.01 { out.append((piece, j)) }
                        }
                    }
                }
            }
        }
        return (out, nil)
    }

    static func overlaps(_ a: CGRect, _ b: CGRect) -> Bool {
        let i = a.intersection(b)
        return !i.isNull && i.width > 0.01 && i.height > 0.01
    }

    /// `rect` minus `hole`: up to four rectangles.
    static func subtract(_ hole: CGRect, from rect: CGRect) -> [CGRect] {
        let i = rect.intersection(hole)
        guard !i.isNull, i.width > 0.01, i.height > 0.01 else { return [rect] }
        let pieces = [
            CGRect(x: rect.minX, y: rect.minY, width: rect.width, height: i.minY - rect.minY),
            CGRect(x: rect.minX, y: i.maxY, width: rect.width, height: rect.maxY - i.maxY),
            CGRect(x: rect.minX, y: i.minY, width: i.minX - rect.minX, height: i.height),
            CGRect(x: i.maxX, y: i.minY, width: rect.maxX - i.maxX, height: i.height),
        ]
        return pieces.filter { $0.width > 0.01 && $0.height > 0.01 }
    }

    /// Whether disjoint `rects` cover `target`.
    static func covers(_ rects: [CGRect], _ target: CGRect) -> Bool {
        let area = rects.reduce(0) { sum, r in
            let i = r.intersection(target)
            return i.isNull ? sum : sum + i.width * i.height
        }
        return area >= target.width * target.height - 0.5
    }
}

nonisolated extension KeyboardLayout {
    /// These keys with `map`'s touch areas.
    func applying(_ map: KeyboardZoneMap) -> KeyboardLayout {
        guard !map.isEmpty else { return self }
        var out = self
        for (k, rects) in map.regions where k < out.keys.count { out.keys[k].zoneRects = rects }
        return out
    }
}
