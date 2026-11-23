import PhotosUI
import SwiftUI

/// Home's and Settings › Experiments' Send feedback row: draws Hex's own window first, then
/// opens the form with that picture. Absent in a build without a report key.
struct FeedbackRow: View {
    /// Where it was opened, for the team (`metadata.screen`).
    let screen: String
    var dictation: DictationController?
    var divider = false
    @State private var form: FeedbackFormStart?

    var body: some View {
        if Feedback.isAvailable {
            if divider { SettingsDivider(leading: 50) }
            Button {
                let picture = FeedbackImage.keyWindow().flatMap(FeedbackImage.jpeg)
                form = FeedbackFormStart(picture: picture, context: .current(screen: screen, dictation: dictation))
            } label: {
                SettingsLinkLabel(title: String(localized: "Send feedback"), icon: "bubble.left")
            }
            .buttonStyle(.plain)
            .sheet(item: $form) { start in
                FeedbackForm(start: start)
            }
        }
    }
}

struct FeedbackFormStart: Identifiable {
    let id = UUID()
    let picture: Data?
    let context: FeedbackContext
}

/// Send feedback: a message, optionally a subject and a kind, the pictures (Hex's window to
/// start with, removable, up to five with the photo picker), an optional email or Send
/// anonymously, and what is sent in plain words. Nothing goes out until Send.
struct FeedbackForm: View {
    let start: FeedbackFormStart
    @Environment(\.dismiss) private var dismiss
    @State private var draft = FeedbackDraft()
    @State private var picked: [PhotosPickerItem] = []
    @State private var sender = FeedbackSender()
    @State private var phase = Phase.editing
    @FocusState private var messageFocused: Bool
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    private static let sendID = "send"

    enum Phase: Equatable {
        case editing
        case sending
        case sent(number: Int, emailPending: Bool, email: String)
        case failed(FeedbackFailure)
    }

    /// The kind, sent as `category`; Other sends none.
    private enum Kind: CaseIterable {
        case bug, idea, question, other
        var category: Feedback.Category? {
            switch self { case .bug: .bug; case .idea: .idea; case .question: .question; case .other: nil }
        }
        var label: String {
            switch self {
            case .bug: String(localized: "Problem")
            case .idea: String(localized: "Idea")
            case .question: String(localized: "Question")
            case .other: String(localized: "Other")
            }
        }
    }

    init(start: FeedbackFormStart) {
        self.start = start
        var draft = FeedbackDraft()
        if let picture = start.picture { draft.images = [picture] }
        _draft = State(initialValue: draft)
    }

    var body: some View {
        ScrollViewReader { scroller in
            sheet
                .onChange(of: phase) { _, phase in
                    // The progress and any error sit just above Send: keep them on screen.
                    switch phase {
                    case .sending, .failed:
                        withAnimation(HexMotion.settleAnimation(reduceMotion: reduceMotion)) {
                            scroller.scrollTo(Self.sendID, anchor: .bottom)
                        }
                    case .editing, .sent:
                        break
                    }
                }
        }
    }

    private var sheet: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                switch phase {
                case .sent(let number, let pending, let email):
                    thanks(number: number, emailPending: pending, email: email)
                default:
                    form
                }
            }
            .frame(maxWidth: 500)
            .padding(.horizontal, 20)
            .padding(.top, 24)
            .padding(.bottom, 20)
            .frame(maxWidth: .infinity)
        }
        .scrollBounceBehavior(.basedOnSize)
        .scrollDismissesKeyboard(.interactively)
        .background(HexTheme.bgDeep.ignoresSafeArea())
        .presentationBackground(HexTheme.bgDeep)
        .preferredColorScheme(.dark)
        .interactiveDismissDisabled(phase == .sending || (!isSent && !draft.trimmedMessage.isEmpty))
        .onChange(of: picked) { _, items in add(items) }
        .onChange(of: draft) { _, _ in
            // An edit after a failed send clears its message; the next Send is a new send.
            if case .failed = phase { phase = .editing }
        }
    }

    private var isSent: Bool {
        if case .sent = phase { return true }
        return false
    }

    // MARK: - The form

    @ViewBuilder
    private var form: some View {
        FeedbackHeader(title: String(localized: "Send feedback"), close: { dismiss() })

        SettingsCard {
            HStack {
                SettingsSegments(
                    items: Kind.allCases,
                    selected: Kind.allCases.first { $0.category == draft.category } ?? .other,
                    label: \.label,
                    onSelect: { kind in draft.category = kind.category }
                )
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 16)
            .frame(minHeight: 50)

            SettingsDivider()

            TextField(String(localized: "Subject (optional)"), text: $draft.subject)
                .font(.system(size: 15))
                .foregroundStyle(HexTheme.textBase)
                .padding(.horizontal, 16)
                .frame(minHeight: 50)
                .onChange(of: draft.subject) { _, s in
                    if s.count > Feedback.subjectLimit { draft.subject = String(s.prefix(Feedback.subjectLimit)) }
                }

            SettingsDivider()

            ZStack(alignment: .topLeading) {
                if draft.message.isEmpty {
                    Text("What happened, or what would you like?")
                        .font(.system(size: 15))
                        .foregroundStyle(HexTheme.textFaint)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 12)
                        .allowsHitTesting(false)
                }
                TextEditor(text: $draft.message)
                    .font(.system(size: 15))
                    .foregroundStyle(HexTheme.textBase)
                    .scrollContentBackground(.hidden)
                    .focused($messageFocused)
                    .padding(.horizontal, 11)
                    .padding(.vertical, 4)
                    .frame(minHeight: 140)
                    .accessibilityLabel("Message")
            }
            if draft.message.count > Feedback.messageLimit - 1000 {
                Text("\(draft.message.count.formatted()) of \(Feedback.messageLimit.formatted()) characters")
                    .font(.system(size: 12))
                    .monospacedDigit()
                    .foregroundStyle(draft.message.count > Feedback.messageLimit ? HexTheme.danger : HexTheme.textMuted)
                    .padding(.horizontal, 16)
                    .padding(.bottom, 10)
            }
        }

        SettingsSectionHeader(title: "Pictures")
        pictures

        SettingsCard {
            if !draft.anonymous {
                TextField(String(localized: "Email for a reply (optional)"), text: $draft.email)
                    .font(.system(size: 15))
                    .foregroundStyle(HexTheme.textBase)
                    .keyboardType(.emailAddress)
                    .textContentType(.emailAddress)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .padding(.horizontal, 16)
                    .frame(minHeight: 50)
                SettingsDivider()
            }
            SettingsToggleRow(title: String(localized: "Send anonymously"), isOn: $draft.anonymous)
        }

        Text(whatIsSent)
            .font(.system(size: 12))
            .foregroundStyle(HexTheme.textMuted)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 16)

        if let problem = shownProblem {
            Text(problem)
                .font(.system(size: 13))
                .foregroundStyle(HexTheme.danger)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.horizontal, 16)
        }

        if phase == .sending {
            VStack(alignment: .leading, spacing: 6) {
                ProgressView(value: sender.progress)
                    .tint(HexTheme.textBase)
                Text(sender.attempt > 1 ? String(localized: "Sending… trying again") : String(localized: "Sending…"))
                    .font(.system(size: 12))
                    .foregroundStyle(HexTheme.textMuted)
            }
            .padding(.horizontal, 16)
            .accessibilityElement(children: .combine)
        }

        OnboardingPrimaryButton(title: "Send", enabled: phase != .sending && draft.problem == nil, action: send)
            .padding(.top, 4)
            .id(Self.sendID)
    }

    private var pictures: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 10) {
                ForEach(Array(draft.images.enumerated()), id: \.offset) { n, data in
                    thumbnail(data, index: n)
                }
                if draft.images.count < Feedback.imageLimit {
                    PhotosPicker(selection: $picked, maxSelectionCount: Feedback.imageLimit - draft.images.count, matching: .images) {
                        VStack(spacing: 6) {
                            Image(systemName: "plus")
                                .font(.system(size: 18, weight: .medium))
                            Text("Add")
                                .font(.system(size: 12))
                        }
                        .foregroundStyle(HexTheme.textMuted)
                        .frame(width: 84, height: 120)
                        .background(HexTheme.bgBase, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                    }
                    .accessibilityLabel("Add a picture")
                }
            }
            .padding(.vertical, 2)
        }
        .scrollClipDisabled()
    }

    private func thumbnail(_ data: Data, index: Int) -> some View {
        ZStack(alignment: .topTrailing) {
            if let image = UIImage(data: data) {
                Image(uiImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
                    .frame(width: 84, height: 120)
                    .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                    .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(HexTheme.borderStrong))
            }
            Button {
                draft.images.remove(at: index)
            } label: {
                Image(systemName: "xmark")
                    .font(.system(size: 10, weight: .bold))
                    .foregroundStyle(HexTheme.textBase)
                    .frame(width: 24, height: 24)
                    .background(Color.black.opacity(0.7), in: Circle())
                    .frame(width: 44, height: 44, alignment: .topTrailing)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .offset(x: 6, y: -6)
            .accessibilityLabel("Remove picture \(index + 1)")
        }
    }

    /// The form's promise, in plain words: what goes out, to whom, and only on Send.
    private var whatIsSent: String {
        if draft.anonymous {
            return String(localized: "When you tap Send, Hex sends your message, the pictures above, Hex's version and the language and speech model it dictates with, your iPhone's model, iOS version and language, and the screen you opened this from to Anomaly's feedback inbox. No email and no install id. Anonymous feedback can't be answered.")
        }
        return draft.trimmedEmail.isEmpty
            ? String(localized: "When you tap Send, Hex sends your message, the pictures above, Hex's version and the language and speech model it dictates with, your iPhone's model, iOS version and language, the screen you opened this from, and a random install id that keeps your reports together, to Anomaly's feedback inbox. Nothing you dictate or type elsewhere is sent.")
            : String(localized: "When you tap Send, Hex sends your message, the pictures above, your email, Hex's version and the language and speech model it dictates with, your iPhone's model, iOS version and language, the screen you opened this from, and a random install id that keeps your reports together, to Anomaly's feedback inbox. Nothing you dictate or type elsewhere is sent.")
    }

    private var shownProblem: String? {
        if case .failed(let failure) = phase { return Self.words(failure) }
        switch draft.problem {
        case .messageTooLong?: return String(localized: "The message is too long. Keep it under 20,000 characters.")
        case .imageTooLarge?, .imagesTooLarge?: return String(localized: "The pictures are too large together. Remove one and try again.")
        case .invalidEmail?: return String(localized: "That email doesn't look right.")
        default: return nil
        }
    }

    static func words(_ failure: FeedbackFailure) -> String {
        switch failure {
        case .offline: String(localized: "Couldn't send: your iPhone seems to be offline. Your message is still here; try again when you're connected.")
        case .tooLarge: String(localized: "That's too large to send. Remove a picture, or add a smaller one, and try again.")
        case .unsupportedImage: String(localized: "One of the pictures couldn't be read. Remove it and try again.")
        case .tryLater: String(localized: "Too many reports right now. Try again in a little while.")
        case .unavailable: String(localized: "Feedback is unavailable in this version of Hex.")
        case .other: String(localized: "Couldn't send your feedback. Try again in a little while.")
        }
    }

    // MARK: - Pictures

    private func add(_ items: [PhotosPickerItem]) {
        guard !items.isEmpty else { return }
        picked = []
        Task {
            for item in items {
                guard draft.images.count < Feedback.imageLimit,
                      let data = try? await item.loadTransferable(type: Data.self) else { continue }
                let jpeg = await Task.detached(priority: .userInitiated) { FeedbackImage.jpeg(fromPicked: data) }.value
                if let jpeg { draft.images.append(jpeg) }
            }
        }
    }

    // MARK: - Sending

    private func send() {
        guard draft.problem == nil, let key = Feedback.reportKey else { return }
        messageFocused = false
        // Made here, at the tap, and reused for every retry of this send.
        var sent = draft
        #if DEBUG
        // Simulator proof of the 415 path: the last picture cut short, as a broken file would be.
        if ProcessInfo.processInfo.arguments.contains("--feedback-broken-image"), let last = sent.images.last {
            sent.images[sent.images.count - 1] = last.prefix(last.count / 2)
        }
        #endif
        let request = FeedbackRequest(draft: sent, context: start.context, installID: draft.anonymous ? "" : Feedback.installID())
        phase = .sending
        Task {
            switch await sender.send(request, key: key) {
            case .sent(let number, let pending):
                phase = .sent(number: number, emailPending: pending, email: draft.trimmedEmail)
            case .failed(let failure):
                phase = .failed(failure)
            }
        }
    }

    // MARK: - After

    @ViewBuilder
    private func thanks(number: Int, emailPending: Bool, email: String) -> some View {
        FeedbackHeader(title: String(localized: "Thanks for your feedback"), close: { dismiss() })
        Text(emailPending
            ? String(localized: "It's with the Hex team. If they reply, you'll get an email at \(email), and you'll need to confirm your address once from that email. Nothing has been emailed yet.")
            : draft.anonymous
                ? String(localized: "It's with the Hex team. It was sent anonymously, so it can't be answered.")
                : String(localized: "It's with the Hex team."))
            .font(.system(size: 15))
            .foregroundStyle(HexTheme.textMuted)
            .fixedSize(horizontal: false, vertical: true)
        OnboardingPrimaryButton(title: "Done") { dismiss() }
            .padding(.top, 4)
    }
}

/// The sheet's title and its close button, as the setup sheet draws them.
private struct FeedbackHeader: View {
    let title: String
    let close: () -> Void

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            Text(title)
                .font(.system(size: 17, weight: .semibold))
                .tracking(-0.2)
                .foregroundStyle(HexTheme.textBase)
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
