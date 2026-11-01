import SwiftUI

/// Chooses which apps the keyboard offers to jump back to, in order.
struct ReturnAppsSheet: View {
    let onSave: ([HostApp]) -> Void
    @State private var chosen: [HostApp]
    @Environment(\.dismiss) private var dismiss

    init(chosen: [HostApp], onSave: @escaping ([HostApp]) -> Void) {
        self.onSave = onSave
        _chosen = State(initialValue: chosen)
    }

    var body: some View {
        NavigationStack {
            List {
                Section("In the keyboard") {
                    ForEach(chosen) { app in
                        row(app)
                    }
                    .onMove { chosen.move(fromOffsets: $0, toOffset: $1) }
                }
                Section("More apps") {
                    ForEach(HostAppCatalog.all.filter { !chosen.contains($0) }) { app in
                        row(app)
                    }
                }
            }
            .environment(\.editMode, .constant(.active))
            .navigationTitle("Return apps")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") {
                        onSave(chosen)
                        dismiss()
                    }
                }
            }
        }
        .preferredColorScheme(.dark)
    }

    private func row(_ app: HostApp) -> some View {
        let isChosen = chosen.contains(app)
        return Button {
            if isChosen {
                chosen.removeAll { $0 == app }
            } else {
                chosen.append(app)
            }
        } label: {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(app.name)
                    Text(app.schemeURL)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                if isChosen {
                    Image(systemName: "checkmark")
                        .foregroundStyle(.tint)
                }
            }
            .contentShape(Rectangle())
        }
        .foregroundStyle(.primary)
    }
}
