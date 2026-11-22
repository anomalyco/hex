import CoreGraphics
import Foundation
import HexKeyboardCore

extension TypingDecoder {
    struct Keys: Equatable, Sendable {
        /// Each letter key's centre, shifted by your tap offsets when there are any.
        var centres: [Character: CGPoint]
        var pitch: CGSize
        /// The bottom letter row's centre, below which a tap may have been meant for space.
        var bottomRowY: CGFloat
        /// The space bar's keycap as on the unsplit keyboard (both bars on a split one), or
        /// nil when unknown: a bottom-row tap above it may have been meant for space.
        var space: CGRect?

        init(centres: [Character: CGPoint], pitch: CGSize, bottomRowY: CGFloat, space: CGRect? = nil) {
            self.centres = centres
            self.pitch = pitch
            self.bottomRowY = bottomRowY
            self.space = space
        }

        /// The letter keys of `layout` (nil off the letters page). `offsets` (in points, by
        /// letter) moves a centre to where you tend to tap it. A split layout's halves are
        /// pushed together, so a letter on both halves (G, V) has one centre, where the
        /// unsplit keyboard has it (`hk_layout_tap_keys`).
        init?(layout: KeyboardLayout, offsets: [Character: CGVector] = [:]) {
            guard layout.layer == .letters else { return nil }
            let m = layout.metrics
            let pitch = CGSize(
                width: layout.isSplit ? SplitGeometry.pitch : layout.letterWidth + m.gap,
                height: m.keyHeight + m.rowGap
            )
            let shifts = offsets.compactMap { letter, shift in
                Self.scalar(of: letter).map { HkKeyOffset(letter: $0, dx: Double(shift.dx), dy: Double(shift.dy)) }
            }
            let split = layout.split.map { [Double($0.middle), Double($0.shift)] } ?? []
            var centres = [HkKeyCentre](repeating: HkKeyCentre(), count: max(1, layout.keys.count))
            var made = HkTapKeys()
            let status = TypingDecoder.withLayoutKeys(layout) { keys in
                split.withUnsafeBufferPointer { split in
                    centres.withUnsafeMutableBufferPointer { buffer in
                        hk_layout_tap_keys(
                            keys.baseAddress, keys.count, split.isEmpty ? nil : split.baseAddress,
                            Double(pitch.width), Double(pitch.height), shifts, shifts.count,
                            buffer.baseAddress, buffer.count, &made
                        )
                    }
                }
            }
            guard status == 0 else { return nil }
            var byLetter: [Character: CGPoint] = [:]
            for centre in centres.prefix(made.keys.centre_count) {
                guard let scalar = Unicode.Scalar(centre.letter) else { continue }
                byLetter[Character(scalar)] = CGPoint(x: centre.x, y: centre.y)
            }
            let space = made.space
            self.init(
                centres: byLetter,
                pitch: pitch,
                bottomRowY: CGFloat(made.bottom_row_y),
                space: made.has_space != 0 ? CGRect(x: space.x, y: space.y, width: space.width, height: space.height) : nil
            )
        }

        /// The keys as the core reads them, for the length of `body`.
        func withCore<R>(_ body: (UnsafePointer<HkTapKeys>) -> R) -> R {
            let letters = centres.compactMap { letter, point in
                Self.scalar(of: letter).map { HkKeyCentre(letter: $0, x: Double(point.x), y: Double(point.y)) }
            }
            return letters.withUnsafeBufferPointer { buffer in
                let bar = space ?? .zero
                var keys = HkTapKeys(
                    keys: HkKeys(
                        centres: buffer.baseAddress, centre_count: buffer.count,
                        pitch_width: Double(pitch.width), pitch_height: Double(pitch.height)
                    ),
                    bottom_row_y: Double(bottomRowY),
                    has_space: space == nil ? 0 : 1,
                    space: HkRect(x: Double(bar.minX), y: Double(bar.minY), width: Double(bar.width), height: Double(bar.height))
                )
                return body(&keys)
            }
        }

        /// A letter's one Unicode scalar, composed; nil for a character that isn't one (no
        /// model letter is).
        static func scalar(of letter: Character) -> UInt32? {
            let scalars = letter.unicodeScalars.count == 1
                ? letter.unicodeScalars
                : String(letter).precomposedStringWithCanonicalMapping.unicodeScalars
            return scalars.count == 1 ? scalars.first?.value : nil
        }
    }

    /// Learn how I type's average tap offsets (in keycap widths and heights) as points, for
    /// letters with 20 taps or more, shrunk toward the centre while there are few
    /// (`hk_layout_offsets`).
    static func offsets(from stats: [String: TypingJournalKeyStats], layout: KeyboardLayout) -> [Character: CGVector] {
        let perKey = layout.keys.map { key -> HkKeyStats in
            guard let c = key.character, let s = stats[c] else { return HkKeyStats() }
            return HkKeyStats(present: 1, count: UInt64(max(0, s.count)), dx: s.dx, dy: s.dy)
        }
        var found = [HkKeyOffset](repeating: HkKeyOffset(), count: max(1, layout.keys.count))
        let count = withLayoutKeys(layout) { keys in
            hk_layout_offsets(keys.baseAddress, perKey, keys.count, &found, found.count)
        }
        var out: [Character: CGVector] = [:]
        for offset in found.prefix(count) {
            guard let scalar = Unicode.Scalar(offset.letter) else { continue }
            out[Character(scalar)] = CGVector(dx: offset.dx, dy: offset.dy)
        }
        return out
    }

    /// `layout`'s keys as the core reads them, for the length of `body`.
    fileprivate static func withLayoutKeys<R>(_ layout: KeyboardLayout, _ body: (UnsafeBufferPointer<HkLayoutKey>) -> R) -> R {
        let characters = layout.keys.map { $0.character.flatMap { strdup($0) } }
        defer { characters.forEach { free($0) } }
        let keys = zip(layout.keys, characters).map { key, character in
            let f = key.frame
            return HkLayoutKey(
                character: character.map { UnsafePointer($0) },
                space: key.kind == .space ? 1 : 0,
                frame: HkRect(x: Double(f.origin.x), y: Double(f.origin.y), width: Double(f.size.width), height: Double(f.size.height))
            )
        }
        return keys.withUnsafeBufferPointer(body)
    }
}
