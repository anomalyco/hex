import SwiftUI

/// What Learn how I type has gathered so far, with a way to erase it.
struct TypingInsightsView: View {
    @State private var snapshot = TypingJournalSnapshot()
    @State private var disabledFixes: Set<String> = []
    @State private var confirmingDelete = false

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(summary)
                .font(.system(size: 12))
                .foregroundStyle(HexTheme.textMuted)

            if !dictationFixes.isEmpty {
                section("Dictation fixes Hex learned") {
                    ForEach(dictationFixes) { fix in
                        Toggle(isOn: isApplied(fix)) {
                            HStack {
                                Text("\(fix.heard) → \(fix.meant)")
                                    .font(.system(size: 13))
                                    .foregroundStyle(HexTheme.textBase)
                                Spacer()
                                Text("\(fix.count)×")
                                    .font(.system(size: 12))
                                    .foregroundStyle(HexTheme.textMuted)
                            }
                        }
                        .tint(HexTheme.success)
                    }
                }
            }

            if !snapshot.dictationOutcomes.isEmpty {
                section("Dictation") {
                    row("Kept as is", detail: share(of: .kept))
                    row("Edited", detail: share(of: .edited))
                }
            }

            if !topFixes.isEmpty {
                section("Words you fix most") {
                    ForEach(topFixes, id: \.label) { fix in
                        row(fix.label, detail: "\(fix.count)×")
                    }
                }
            }

            if !missedKeys.isEmpty {
                section("Where you tap off-centre") {
                    ForEach(missedKeys, id: \.key) { item in
                        row(item.key.uppercased(), detail: item.direction)
                    }
                }
            }

            Button(role: .destructive) {
                confirmingDelete = true
            } label: {
                Text("Delete what Hex learned")
                    .font(.system(size: 13, weight: .medium))
            }
            .disabled(snapshot.isEmpty && disabledFixes.isEmpty)
            .confirmationDialog("Delete what Hex learned about your typing and dictation?", isPresented: $confirmingDelete) {
                Button("Delete", role: .destructive) {
                    TypingJournal.delete()
                    reload()
                }
            }
        }
        .onAppear(perform: reload)
    }

    private var summary: String {
        if snapshot.isEmpty {
            return "Type or dictate with the Hex keyboard and this fills in. It stays on this iPhone."
        }
        let dictations = snapshot.dictationOutcomes.count
        return "\(snapshot.tapCount) taps, \(snapshot.corrections.count) typing fixes, and \(dictations) "
            + (dictations == 1 ? "dictation" : "dictations") + " learned, kept only on this iPhone."
    }

    /// Learned fixes, most used first; each can be switched off.
    private var dictationFixes: [DictationLearnedFix] {
        Array(snapshot.learnedDictationFixes
            .sorted { $0.count == $1.count ? $0.id < $1.id : $0.count > $1.count }
            .prefix(12))
    }

    private func isApplied(_ fix: DictationLearnedFix) -> Binding<Bool> {
        Binding(
            get: { !disabledFixes.contains(fix.id) },
            set: { applied in
                if applied {
                    disabledFixes.remove(fix.id)
                } else {
                    disabledFixes.insert(fix.id)
                }
                TypingJournalSettings().disabledDictationFixes = disabledFixes
            }
        )
    }

    private func share(of outcome: DictationOutcome) -> String {
        let total = snapshot.dictationOutcomes.count
        guard total > 0 else { return "–" }
        let percent = (Double(snapshot.count(of: outcome)) / Double(total) * 100).rounded()
        return "\(Int(percent))%"
    }

    private var topFixes: [(label: String, count: Int)] {
        var counts: [String: Int] = [:]
        for correction in snapshot.corrections where correction.kind != .undone && correction.kind != .keptTyped {
            counts["\(correction.typed) → \(correction.final)", default: 0] += 1
        }
        return counts.sorted { $0.value == $1.value ? $0.key < $1.key : $0.value > $1.value }
            .prefix(5)
            .map { (label: $0.key, count: $0.value) }
    }

    private var missedKeys: [(key: String, direction: String)] {
        snapshot.keys
            .filter { $0.value.count >= 20 }
            .map { (key: $0.key, offset: $0.value.meanOffset) }
            .filter { hypot($0.offset.dx, $0.offset.dy) >= 0.12 }
            .sorted { hypot($0.offset.dx, $0.offset.dy) > hypot($1.offset.dx, $1.offset.dy) }
            .prefix(4)
            .map { item in
                let horizontal = abs(item.offset.dx) < 0.08 ? "" : (item.offset.dx > 0 ? "right" : "left")
                let vertical = abs(item.offset.dy) < 0.08 ? "" : (item.offset.dy > 0 ? "low" : "high")
                return (key: item.key, direction: [vertical, horizontal].filter { !$0.isEmpty }.joined(separator: " and "))
            }
    }

    private func reload() {
        snapshot = TypingJournal.load(from: FileManager.default
            .containerURL(forSecurityApplicationGroupIdentifier: KeyboardBridge.appGroup)?
            .appendingPathComponent(TypingJournal.fileName))
        disabledFixes = TypingJournalSettings().disabledDictationFixes
    }

    private func section<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(HexTheme.textMuted)
            content()
        }
    }

    private func row(_ title: String, detail: String) -> some View {
        HStack {
            Text(title)
                .font(.system(size: 13))
                .foregroundStyle(HexTheme.textBase)
            Spacer()
            Text(detail)
                .font(.system(size: 12))
                .foregroundStyle(HexTheme.textMuted)
        }
    }
}
