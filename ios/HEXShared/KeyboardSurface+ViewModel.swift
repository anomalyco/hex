import Combine
import SwiftUI
import UIKit

/// Which dictation starts when a finger lands on Dictate or the Hex key.
/// Every surface (dictation bar, full keyboard, selection dictation, auto-start, touch-down
/// arming) switches on this single resolution.
nonisolated enum DictationRoute: Equatable, Sendable {
    /// Full Access is off: open Hex's setup sheet or show the Full Access hint.
    case setUp
    /// Full Access is on, but Hex's speech model isn't installed yet and the keyboard can't
    /// dictate by itself: open Hex to finish setup.
    case needsModel
    /// Hex's warm session takes the dictation (including warm retry after a failure).
    case warm
    /// In-keyboard dictation (iOS 26+) runs in the keyboard's own process.
    case inKeyboard
    /// Opens Hex to dictate (cold start / handoff). The optional reason records why in-keyboard
    /// dictation handed off (`memory`, `speechModel`, `multilingual`), or nil where in-keyboard
    /// dictation isn't supported.
    case opensHex(KeyboardSelfDictation.Reason?)

    static func resolve(
        hasFullAccess: Bool,
        isModelInstalled: Bool,
        isWarmAvailable: Bool,
        localAvailable: Bool,
        opensHexReason: KeyboardSelfDictation.Reason?
    ) -> DictationRoute {
        guard hasFullAccess else { return .setUp }
        // In-keyboard dictation uses the keyboard's own speech model, not Hex's, so it still
        // dictates while Hex's isn't installed (a first download, Continue without a model, a
        // failed preparation), as 1.0.4 did. A warm session needs Hex's model.
        guard isModelInstalled else { return localAvailable ? .inKeyboard : .needsModel }
        if isWarmAvailable { return .warm }
        if localAvailable { return .inKeyboard }
        return .opensHex(opensHexReason)
    }

    /// Whether dictation starts without leaving the host app (warm session or in-keyboard).
    var dictatesInPlace: Bool {
        self == .warm || self == .inKeyboard
    }
}

/// The dictation bar's state: its phase, the style and language trays, the message and
/// transcript, and the clock the surface reads. Kept beside the surface view.
@MainActor
final class KeyboardViewModel: ObservableObject {
    enum PickerTray: Equatable {
        case none
        case style
        case language
        /// More languages…: every other language Apple Speech has, in place of the keys.
        case moreLanguages
    }

    enum StagePhase: Equatable {
        case idle
        case starting
        case recording
        case transcribing
        case failed
    }

    /// Mutually exclusive status shown in the dictation bar's leading slot and read by the full
    /// keyboard's strip for failures.
    enum StatusSlot: Equatable {
        case none
        case noAccess
        case starting
        case recording
        case message(String, isFailure: Bool)
        case translationNote(TranslationFailure)
        case warm
    }

    struct PendingCommand: Equatable {
        let kind: KeyboardCommandKind
        let jobID: String
        let sentAt: TimeInterval
    }

    static let commandAckTimeout: TimeInterval = 1.6

    let clock = KeyboardClockModel()
    /// The words so far while the keyboard records with live words on (`LiveDictationPreviewHost`).
    let liveText = LiveDictationText()

    @Published var hasFullAccess = true
    @Published var needsGlobeKey = true
    /// The bar's return key for the field's type, as the full keyboard's, and whether it's on.
    @Published var returnKey = ReturnKeyStyle(.default)
    @Published var returnKeyEnabled = true
    @Published var isModelInstalled = false
    @Published private(set) var isWarmAvailable = false
    @Published private(set) var stagePhase: StagePhase = .idle
    @Published private(set) var statusMessage: String?
    /// Why Dictate opens Hex when in-keyboard dictation can't take it, fed by `KeyboardViewController`.
    var opensHexReason: KeyboardSelfDictation.Reason?

    /// The single precedence table for what happens when a finger lands to dictate.
    var route: DictationRoute {
        DictationRoute.resolve(
            hasFullAccess: hasFullAccess,
            isModelInstalled: isModelInstalled,
            isWarmAvailable: isWarmAvailable,
            localAvailable: onLocalDictateTap != nil,
            opensHexReason: opensHexReason
        )
    }

    /// A tap on Dictate or the Hex key records through Hex's warm session (including retrying
    /// after a warm-session failure).
    var dictatesThroughHex: Bool {
        route == .warm
    }

    /// Push-to-talk is available when idle or after a failure and dictation stays in-place.
    var canPushToTalk: Bool {
        route.dictatesInPlace && (stagePhase == .idle || stagePhase == .failed)
    }

    /// Single mutually exclusive status for the leading bar slot and full-keyboard failure strip.
    var statusSlot: StatusSlot {
        guard hasFullAccess else { return .noAccess }
        switch stagePhase {
        case .starting:
            return .starting
        case .recording:
            return .recording
        case .transcribing:
            return .none
        case .failed:
            if let statusMessage { return .message(statusMessage, isFailure: true) }
            if let translationNote { return .translationNote(translationNote) }
            return isWarmAvailable ? .warm : .none
        case .idle:
            if let translationNote { return .translationNote(translationNote) }
            if let statusMessage { return .message(statusMessage, isFailure: false) }
            return isWarmAvailable ? .warm : .none
        }
    }

    /// Failure message shown consistently by both the dictation bar and the full keyboard.
    var failureMessage: String? {
        if case .message(let text, isFailure: true) = statusSlot { return text }
        return nil
    }
    @Published var style: DictationStyle = .polished
    @Published var activeTray: PickerTray = .none
    /// Settings › Dictation language (nil for Automatic), what Automatic resolves to, and the
    /// languages the keyboard's quick switch offers. Empty choices hide the switch.
    @Published var dictationLanguage: String?
    @Published var automaticLanguage: String?
    @Published var languageChoices: [String] = []
    /// Every other language Apple Speech has, for More languages… (`DictationLanguageSwitch.more`).
    @Published var moreLanguages: [String] = []
    /// The language a long press on its chip offered to remove: Remove shows beside it.
    @Published var removingLanguage: String?
    /// Translate what I say's target (BCP 47) while it is on, else nil.
    @Published var translationTarget: String?
    /// The last dictation went in as spoken while Translate what I say was on: both keyboards
    /// say so for a few seconds, with the fix, which opens Hex's Settings.
    @Published var translationNote: TranslationFailure?
    /// Translate what I say's target whether it is on or off, for the keyboards' Translate to
    /// <language> switch; nil hides the switch (no Apple translation before iOS 26).
    @Published var translationLanguage: String?
    /// The targets the full keyboard's Hex key menu offers (`DictationTranslationSettings.targetChoices`).
    @Published var translationTargetChoices: [String] = []
    /// The dictation bar's emoji key: Settings › Keyboard › Emoji, in a field that takes emoji.
    @Published var showsEmojiKey = true
    /// A finished dictation the keyboard didn't insert by itself (it missed its moment, or
    /// the keyboard came back in another field): the full keyboard's strip and the dictation
    /// bar's top row offer Paste dictation, which calls `onPasteDictation`.
    @Published var offeredDictation: String?

    // Non-published live telemetry read directly by `DotScopeHostView` / button tap handlers
    // so 25–30 Hz polling never invalidates SwiftUI views mid-spring.
    var liveLoudnessDB: Float = -62.0
    var currentTimestamp: TimeInterval = Date().timeIntervalSince1970
    private(set) var currentSnapshot: KeyboardSnapshot = .offline
    var coldStartURL: URL = KeyboardLaunchRequest.startSession(requestedAt: nil).url
    private(set) var pendingCommand: PendingCommand?

    var onDictateTap: (() -> Void)?
    var onLocalDictateTap: (() -> Void)?
    /// The finger landed on the full keyboard's Hex key (wall clock), before it is a tap,
    /// a hold or a slide.
    var onHexKeyPress: ((TimeInterval) -> Void)?
    /// That press became a slide (the style menu) or was cancelled before it was a tap or a hold.
    var onHexKeyAbandon: (() -> Void)?
    var onStopTap: (() -> Void)?
    var onCancelTap: (() -> Void)?
    var onStyleSelected: ((DictationStyle) -> Void)?
    /// A language picked in the keyboard's switch; nil is Automatic.
    var onLanguageSelected: ((String?) -> Void)?
    /// A language taken off the keyboard's switch (a long press on it, then Remove).
    var onLanguageRemoved: ((String) -> Void)?
    /// Translate to <language> switched on (true) or off where you dictate.
    var onTranslationToggled: ((Bool) -> Void)?
    /// A target picked in the Hex key menu; it also turns translation on.
    var onTranslationTargetSelected: ((String) -> Void)?
    var onSpace: (() -> Void)?
    /// The dictation bar's space bar slid as a trackpad: move the cursor by this many characters.
    var onMoveCursor: ((Int) -> Void)?
    /// A trackpad step or a long press that opened Remove, for the selection haptic.
    var onSelectionStep: (() -> Void)?
    /// A punctuation key on the dictation bar.
    var onInsertText: ((String) -> Void)?
    /// The dictation bar's emoji key: the full keyboard's emoji panel opens in its place.
    var onEmojiTap: (() -> Void)?
    var onDelete: (() -> Void)?
    var onReturn: (() -> Void)?
    var onOpenURL: ((URL) -> Void)?
    var onPasteDictation: (() -> Void)?
    /// The dictation bar's light haptic for each press; the keyboard allows it with Full
    /// Access and Experiments › Haptics, as the full keyboard's keys.
    var onKeyFeedback: (() -> Void)?
    weak var inputViewController: UIInputViewController?

    /// The language dictation uses now, when known.
    var effectiveLanguage: String? {
        dictationLanguage ?? automaticLanguage
    }

    /// Its own short name ("Norsk", "English"), for the switch and the space bar.
    var languageLabel: String? {
        effectiveLanguage.map(DictationLanguageSwitch.nativeName(for:))
    }

    var showsLanguageSwitch: Bool {
        hasFullAccess && !(languageChoices.isEmpty && moreLanguages.isEmpty)
    }

    /// "Paste in English" or "Paste in Spanish", when Translate what I say is on and you
    /// dictate in another language than its target.
    var pasteChoice: TranslationPasteChoice? {
        guard hasFullAccess, let translationTarget else { return nil }
        return TranslationPasteChoice(target: translationTarget, source: effectiveLanguage)
    }

    /// The keyboards' Translate to <language> switch can show: Full Access and a target.
    var showsTranslationSwitch: Bool {
        hasFullAccess && translationLanguage != nil
    }

    /// Dictations paste translated: Translate what I say is on. The Hex key wears the target's
    /// badge ("EN") the whole time.
    var isTranslating: Bool {
        hasFullAccess && translationTarget != nil
    }

    /// The target while `isTranslating`, for the badge beside the Hex key ("EN").
    var translatingTo: String? {
        isTranslating ? translationTarget : nil
    }

    /// The language label with where dictations go while they paste translated:
    /// "Español → English".
    var languageSummary: String? {
        guard let languageLabel else { return nil }
        guard let target = pasteChoice?.target else { return languageLabel }
        return "\(languageLabel) → \(DictationLanguageSwitch.nativeName(for: target))"
    }

    var dotScopePhase: DotScopePhase {
        switch stagePhase {
        case .idle:
            return .idle
        case .starting:
            return .starting
        case .recording:
            return .recording
        case .transcribing:
            return .processing
        case .failed:
            return .failed
        }
    }

    func setActiveTray(_ tray: PickerTray, reduceMotion: Bool) {
        guard activeTray != tray else { return }
        withAnimation(HexMotion.trayAnimation(reduceMotion: reduceMotion)) {
            activeTray = tray
            removingLanguage = nil
        }
    }

    func setPendingCommand(_ command: PendingCommand?, now: TimeInterval, animated: Bool) {
        currentTimestamp = now
        pendingCommand = command
        reconcileVisualState(
            hasFullAccess: hasFullAccess,
            snapshot: currentSnapshot,
            isModelInstalled: isModelInstalled,
            style: style,
            now: now,
            animated: animated
        )
    }

    func applyBridgeState(
        hasFullAccess newFullAccess: Bool,
        snapshot newSnapshot: KeyboardSnapshot,
        isModelInstalled newModelInstalled: Bool,
        style newStyle: DictationStyle,
        now: TimeInterval,
        clearPending: Bool = false,
        animated: Bool
    ) {
        // A job Hex stopped heartbeating for is over (Hex ended, crashed or was suspended
        // mid-dictation): without this its last snapshot held every keyboard in recording or
        // transcribing for good, with Done and Cancel sent to no one.
        // A failure Hex wrote a while ago is over too (`KeyboardSnapshot.hasStaleMessage`).
        let newSnapshot = newSnapshot.droppingOrphanedJob(at: now).droppingStaleMessage(at: now)
        currentTimestamp = now
        currentSnapshot = newSnapshot
        liveLoudnessDB = newSnapshot.state == .recording ? newSnapshot.effectiveLoudnessDB : -62.0

        if clearPending {
            pendingCommand = nil
        } else if let pending = pendingCommand {
            let expired = now - pending.sentAt >= Self.commandAckTimeout
            let acknowledged = (pending.kind == .start && (newSnapshot.state == .starting || newSnapshot.state == .recording))
                || (pending.kind == .stop && newSnapshot.state != .starting && newSnapshot.state != .recording)
                || (pending.kind == .cancel && newSnapshot.state != .starting && newSnapshot.state != .recording)
            if expired || acknowledged {
                pendingCommand = nil
            }
        }

        let recordingElapsed: TimeInterval? = if newSnapshot.state == .recording {
            newSnapshot.recordingElapsed(at: now)
        } else {
            nil
        }
        let warmRemaining: TimeInterval? = newSnapshot.isAvailable(at: now)
            ? max(0, newSnapshot.expiresAt - now)
            : nil
        clock.update(recordingElapsed: recordingElapsed, warmRemaining: warmRemaining)

        reconcileVisualState(
            hasFullAccess: newFullAccess,
            snapshot: newSnapshot,
            isModelInstalled: newModelInstalled,
            style: newStyle,
            now: now,
            animated: animated
        )
    }

    /// Convenience for deterministic preview/snapshot fixtures (`KeyboardPreviewHost`).
    func configureFixture(
        snapshot: KeyboardSnapshot,
        now: TimeInterval,
        hasFullAccess: Bool = true,
        isModelInstalled: Bool = true,
        style: DictationStyle = .polished,
        activeTray: PickerTray = .none
    ) {
        self.activeTray = activeTray
        applyBridgeState(
            hasFullAccess: hasFullAccess,
            snapshot: snapshot,
            isModelInstalled: isModelInstalled,
            style: style,
            now: now,
            clearPending: true,
            animated: false
        )
    }

    private func reconcileVisualState(
        hasFullAccess newFullAccess: Bool,
        snapshot snap: KeyboardSnapshot,
        isModelInstalled newModelInstalled: Bool,
        style newStyle: DictationStyle,
        now: TimeInterval,
        animated: Bool
    ) {
        let nextWarm = newFullAccess && snap.isAvailable(at: now)
        let nextStage = computeStagePhase(snapshot: snap, now: now, hasFullAccess: newFullAccess)
        let trimmedMsg = snap.message?.trimmingCharacters(in: .whitespacesAndNewlines)
        let nextMessage = (trimmedMsg?.isEmpty == false) ? trimmedMsg : nil

        let changed = hasFullAccess != newFullAccess
            || isWarmAvailable != nextWarm
            || isModelInstalled != newModelInstalled
            || stagePhase != nextStage
            || statusMessage != nextMessage
            || style != newStyle

        guard changed else { return }

        let commit = {
            self.hasFullAccess = newFullAccess
            self.isWarmAvailable = nextWarm
            self.isModelInstalled = newModelInstalled
            self.stagePhase = nextStage
            self.statusMessage = nextMessage
            self.style = newStyle
        }

        if animated {
            withAnimation(HexMotion.stageAnimation(reduceMotion: UIAccessibility.isReduceMotionEnabled)) {
                commit()
            }
        } else {
            commit()
        }
    }

    private func computeStagePhase(
        snapshot snap: KeyboardSnapshot,
        now: TimeInterval,
        hasFullAccess: Bool
    ) -> StagePhase {
        guard hasFullAccess else { return .idle }

        if let pending = pendingCommand, now - pending.sentAt < Self.commandAckTimeout {
            switch pending.kind {
            case .cancel:
                return .idle
            case .stop where snap.state == .recording:
                return .transcribing
            case .stop where snap.state == .starting:
                return .idle
            case .start where snap.state != .recording:
                return .starting
            default:
                break
            }
        }

        switch snap.state {
        case .starting:
            return .starting
        case .recording:
            return .recording
        case .transcribing:
            return .transcribing
        case .failed:
            return .failed
        case .offline, .ready:
            return .idle
        }
    }
}
