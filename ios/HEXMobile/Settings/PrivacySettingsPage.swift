import SwiftUI

/// Settings › Privacy: what Hex keeps about how you type, all of it on this iPhone, and
/// under Advanced the typing logs you can record to export.
struct PrivacySettingsPage: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var learnTyping = TypingJournalSettings().isEnabled
    @State private var typingLogs = TypingLogSettings().isEnabled
    @State private var showTypingLogs = false
    @State private var confirmingTypingLogs = SettingsSnapshotFixture.confirmsTypingLogs

    var body: some View {
        SettingsPageScaffold(title: "Privacy") {
            HStack(spacing: 7) {
                Image(systemName: "lock.fill")
                    .font(.system(size: 11, weight: .medium))
                Text("Everything stays on this iPhone")
                    .font(.system(size: 13))
            }
            .foregroundStyle(HexTheme.textMuted)
            .padding(.horizontal, 16)
            .padding(.bottom, 2)

            SettingsCard {
                // Not only for the full keyboard: it also learns from dictations the
                // dictation bar inserts. On until turned off (`TypingJournalSettings`).
                SettingsToggleRow(
                    title: String(localized: "Learn how I type"),
                    detail: String(localized: "Learns on this iPhone from your taps and the words you fix in Hex"),
                    isOn: $learnTyping
                )

                if learnTyping {
                    SettingsDivider()
                    TypingInsightsView()
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 12)
                }
            }

            SettingsSectionHeader(title: "Advanced")

            SettingsCard {
                // Apart from Learn how I type: this keeps everything, to export and train on.
                // Turning it on asks first (App Review 2.5.14); turning it off doesn't.
                SettingsToggleRow(
                    title: String(localized: "Record typing logs"),
                    detail: String(localized: "Keeps 14 days of taps and typed text on this iPhone, to export"),
                    isOn: Binding(
                        get: { typingLogs },
                        set: { on in
                            if on {
                                confirmingTypingLogs = true
                            } else {
                                typingLogs = false
                            }
                        }
                    )
                )
                .alert("Record typing logs?", isPresented: $confirmingTypingLogs) {
                    Button("Cancel", role: .cancel) {}
                    Button("Turn On") {
                        typingLogs = true
                    }
                } message: {
                    Text("The Hex keyboard will record each tap and the text you type or dictate, including text around the cursor, for 14 days. Logs stay on this iPhone. Password, code and card fields are never recorded.")
                }

                if typingLogs {
                    SettingsDivider()
                    Button {
                        showTypingLogs = true
                    } label: {
                        SettingsLinkLabel(title: String(localized: "Typing logs"))
                    }
                    .buttonStyle(.plain)
                }
            }

            // What Send feedback sends, and when (FeedbackForm; the website's policy says the same).
            if Feedback.isAvailable {
                SettingsSectionHeader(title: "Feedback")
                Text("Hex sends nothing unless you tap Send in Send feedback. Then it sends your message and the pictures you kept, your email if you add one, and details about Hex and this iPhone to Anomaly's feedback inbox. With an email, Hex also sends a random install ID so your reports stay together. Your dictations, what you type, and what Hex learned are never included.")
                    .font(.system(size: 12))
                    .foregroundStyle(HexTheme.textMuted)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.horizontal, 16)
            }
        }
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: learnTyping)
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: typingLogs)
        .sheet(isPresented: $showTypingLogs) {
            TypingLogsView()
        }
        .onChange(of: learnTyping) { _, enabled in
            TypingJournalSettings().isEnabled = enabled
        }
        .onChange(of: typingLogs) { _, enabled in
            TypingLogSettings().isEnabled = enabled
        }
        .onAppear {
            learnTyping = TypingJournalSettings().isEnabled
            typingLogs = TypingLogSettings().isEnabled
        }
    }
}
