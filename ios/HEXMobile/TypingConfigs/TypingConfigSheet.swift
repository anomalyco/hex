import SwiftUI

/// The sheet a typing config opens in (`TypingConfigInbox`). It stays until closed and
/// never applies anything by itself: with Typing configs off it says where to turn them on; a
/// file Hex won't read says why; a config shows its review.
struct TypingConfigSheet: View {
    @Bindable var inbox: TypingConfigInbox
    @State private var enabled = TypingConfigSettings().isEnabled

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                if let arrival = inbox.current {
                    content(arrival)
                        .id(arrival.id)
                        .transition(.opacity)
                }
                if inbox.waiting > 0 {
                    Text(inbox.waiting == 1
                        ? String(localized: "1 more file waiting")
                        : String(localized: "\(inbox.waiting) more files waiting"))
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                        .frame(maxWidth: .infinity)
                }
            }
            .frame(maxWidth: 500)
            .padding(.horizontal, 20)
            .padding(.top, 24)
            .padding(.bottom, 20)
            .frame(maxWidth: .infinity)
            .onGeometryChange(for: CGFloat.self) { $0.size.height } action: {
                TypingConfigSheetPresenter.contentHeightChanged($0)
            }
        }
        .scrollBounceBehavior(.basedOnSize)
        .background(HexTheme.bgDeep.ignoresSafeArea())
        .preferredColorScheme(.dark)
        .animation(HexMotion.reducedFade, value: inbox.current?.id)
        .onAppear { enabled = TypingConfigSettings().isEnabled }
    }

    @ViewBuilder
    private func content(_ arrival: TypingConfigArrival) -> some View {
        switch arrival.content {
        case .tooLarge(let bytes):
            message(
                title: String(localized: "This file is too large"),
                body: String(localized: "A typing config is at most \(Self.size(TypingConfigLimits.maxBytes)), and “\(arrival.name)” is \(Self.size(bytes)). Hex didn't open it, and nothing has changed.")
            )
        case .unreadable:
            message(
                title: String(localized: "Hex couldn't read this file"),
                body: String(localized: "“\(arrival.name)” is empty or couldn't be opened. Nothing has changed.")
            )
        case .file(let data):
            if enabled {
                TypingConfigReviewView(
                    arrival: arrival,
                    data: data,
                    keyboards: TypingConfigSettings().keyboards,
                    fullAccessOn: KeyboardSetupMonitor().fullAccessOn,
                    done: inbox.finishCurrent
                )
            } else {
                experimentOff(arrival)
            }
        }
    }

    private func experimentOff(_ arrival: TypingConfigArrival) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            header(String(localized: "Typing configs are off"), close: inbox.closeAll)
            Text("“\(arrival.name)” is a typing config: words and key positions made for how you type. Hex opens typing configs only while Settings › Experiments › Typing configs is on. Nothing has changed.")
                .font(.system(size: 15))
                .foregroundStyle(HexTheme.textMuted)
                .fixedSize(horizontal: false, vertical: true)
            VStack(spacing: 4) {
                // Turns the experiment on here and shows the file's review at once.
                OnboardingPrimaryButton(title: "Turn on typing configs") {
                    TypingConfigSettings().isEnabled = true
                    enabled = true
                }
                OnboardingQuietButton(title: "Open Experiments") {
                    inbox.parkForExperiments()
                }
            }
            .padding(.top, 4)
        }
    }

    private func message(title: String, body: String) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            header(title, close: inbox.finishCurrent)
            Text(body)
                .font(.system(size: 15))
                .foregroundStyle(HexTheme.textMuted)
                .fixedSize(horizontal: false, vertical: true)
            OnboardingPrimaryButton(title: "Close", action: inbox.finishCurrent)
                .padding(.top, 4)
        }
    }

    static func size(_ bytes: Int) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .memory)
    }
}

/// The sheet's title row: the title and its close button, as the setup sheet's.
struct TypingConfigSheetHeader: View {
    let title: String
    let close: () -> Void

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            Text(title)
                .font(.system(size: 17, weight: .semibold))
                .tracking(-0.2)
                .foregroundStyle(HexTheme.textBase)
                .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 8)
            Button(action: close) {
                Image(systemName: "xmark")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(HexTheme.textMuted)
                    .frame(width: 30, height: 30)
                    .background(Color.white.opacity(0.07), in: Circle())
                    .frame(width: 44, height: 44, alignment: .trailing)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Close")
        }
    }
}

private extension TypingConfigSheet {
    func header(_ title: String, close: @escaping () -> Void) -> some View {
        TypingConfigSheetHeader(title: title, close: close)
    }
}
