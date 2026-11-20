import SwiftUI
import UIKit

/// Record typing logs: each day's file, to share (AirDrop it to a Mac) or delete.
struct TypingLogsView: View {
    @Environment(\.dismiss) private var dismiss
    @State private var days: [TypingLogDay] = []
    /// Event counts by day and size, counted off the main thread when a row shows.
    @State private var counts: [String: Int] = [:]
    @State private var confirmingDelete = false

    var body: some View {
        NavigationStack {
            List {
                Section {
                    if days.isEmpty {
                        Text("Type with Hex's keyboard and each day's log shows up here.")
                            .foregroundStyle(.secondary)
                    }
                    ForEach(days) { day in
                        row(day)
                            .task(id: countKey(day)) { await count(day) }
                    }
                } footer: {
                    Text("One file per day, kept 14 days, one event per line. Password, code and card fields are never logged.")
                }

                if !days.isEmpty {
                    Section {
                        Button("Delete all", role: .destructive) {
                            confirmingDelete = true
                        }
                    }
                }
            }
            .navigationTitle("Typing logs")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
                ToolbarItem(placement: .primaryAction) {
                    Button("Share all") {
                        ShareSheet.present(days.map(\.url))
                    }
                    .disabled(days.isEmpty)
                }
            }
            .confirmationDialog("Delete all typing logs?", isPresented: $confirmingDelete, titleVisibility: .visible) {
                Button("Delete all", role: .destructive) {
                    TypingLogStore.deleteAll()
                    reload()
                }
            } message: {
                Text("Every day's log is removed from this iPhone.")
            }
            .refreshable { reload() }
        }
        .preferredColorScheme(.dark)
        .onAppear(perform: reload)
    }

    private func row(_ day: TypingLogDay) -> some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(title(day))
                Text(detail(day))
                    .font(.caption)
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Button {
                ShareSheet.present([day.url])
            } label: {
                Image(systemName: "square.and.arrow.up")
            }
            .buttonStyle(.borderless)
            .accessibilityLabel("Share \(title(day))")
        }
    }

    private func title(_ day: TypingLogDay) -> String {
        guard let date = day.date else { return day.day }
        if Calendar.current.isDateInToday(date) { return String(localized: "Today") }
        if Calendar.current.isDateInYesterday(date) { return String(localized: "Yesterday") }
        return date.formatted(.dateTime.weekday(.wide).month(.abbreviated).day())
    }

    private func detail(_ day: TypingLogDay) -> String {
        let size = ByteCountFormatter.string(fromByteCount: Int64(day.bytes), countStyle: .file)
        guard let count = counts[countKey(day)] else { return size }
        return count == 1
            ? String(localized: "\(size) · 1 event")
            : String(localized: "\(size) · \(count.formatted()) events")
    }

    private func countKey(_ day: TypingLogDay) -> String {
        "\(day.day)|\(day.bytes)"
    }

    private func count(_ day: TypingLogDay) async {
        let key = countKey(day)
        guard counts[key] == nil else { return }
        let url = day.url
        let count = await Task.detached(priority: .utility) { TypingLogStore.countEvents(in: url) }.value
        counts[key] = count
    }

    private func reload() {
        TypingLogStore.prune(in: TypingLogStore.defaultDirectory)
        days = TypingLogStore.days()
    }
}

/// The system share sheet for files, over whatever is showing.
@MainActor
enum ShareSheet {
    static func present(_ urls: [URL]) {
        guard !urls.isEmpty else { return }
        let scene = UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .first { $0.activationState == .foregroundActive }
        guard var top = scene?.keyWindow?.rootViewController else { return }
        while let presented = top.presentedViewController, !presented.isBeingDismissed {
            top = presented
        }
        let controller = UIActivityViewController(activityItems: urls, applicationActivities: nil)
        controller.popoverPresentationController?.sourceView = top.view
        controller.popoverPresentationController?.sourceRect = CGRect(x: top.view.bounds.midX, y: 60, width: 1, height: 1)
        top.present(controller, animated: true)
    }
}

/// `--snap-state experiments` (Settings › Experiments) and `--snap-state typing-logs`
/// (the Typing logs screen) on Simulator; pass `--experiment-typing-logs` to show it on.
struct ExperimentsSnapshotStage: View {
    let scene: String

    static var requestedScene: String? {
        let args = ProcessInfo.processInfo.arguments
        guard let idx = args.firstIndex(of: "--snap-state"), idx + 1 < args.count else { return nil }
        let value = args[idx + 1]
        return value == "experiments" || value == "typing-logs" ? value : nil
    }

    var body: some View {
        if scene == "typing-logs" {
            TypingLogsView()
        } else {
            NavigationStack {
                ExperimentsSettingsPage()
            }
            .tint(HexTheme.textBase)
            .preferredColorScheme(.dark)
        }
    }
}
