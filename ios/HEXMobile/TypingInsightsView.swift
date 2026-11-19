import SwiftUI

/// What Learn how I type has gathered so far, with a way to erase it.
struct TypingInsightsView: View {
    @State private var snapshot = TypingJournalSnapshot()
    @State private var confirmingDelete = false

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(summary)
                .font(.system(size: 12))
                .foregroundStyle(HexTheme.textMuted)

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
            .disabled(snapshot.tapCount == 0 && snapshot.corrections.isEmpty)
            .confirmationDialog("Delete what Hex learned about your typing?", isPresented: $confirmingDelete) {
                Button("Delete", role: .destructive) {
                    TypingJournal.delete()
                    reload()
                }
            }
        }
        .onAppear(perform: reload)
    }

    private var summary: String {
        if snapshot.tapCount == 0 && snapshot.corrections.isEmpty {
            return "Type with the Hex keyboard and this fills in. It stays on this iPhone."
        }
        return "\(snapshot.tapCount) taps and \(snapshot.corrections.count) fixes learned, kept only on this iPhone."
    }

    private var topFixes: [(label: String, count: Int)] {
        var counts: [String: Int] = [:]
        for correction in snapshot.corrections where correction.kind != .undone {
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
