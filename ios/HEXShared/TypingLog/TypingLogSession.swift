import CoreGraphics
import Foundation
import UIKit

/// Experiment (Record typing logs): every tap Hex's keyboard sees, where it landed, the key
/// it typed and the text that resulted, plus dictations, written to day files in the App
/// Group for 14 days so they can be exported from the app. Off by default; nothing is sent.
/// Separate from Learn how I type, which keeps only word pairs.
nonisolated struct TypingLogSettings {
    static let enabledKey = "experiments.typingLogs"

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    var isEnabled: Bool {
        get { defaults?.bool(forKey: Self.enabledKey) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Self.enabledKey) }
    }

    /// Set once `turnOffOnce()` has run; a later version turns it off again under a new key.
    static let turnedOffKey = "migrations.typingLogsOff.v1"

    /// Once per App Group (Nov 2026): Record typing logs goes off for everyone who had it on,
    /// so the bar's Logging mark goes. The logs already recorded stay, and so does the switch
    /// in Settings › Privacy › Advanced, which turns it back on for good.
    func turnOffOnce() {
        guard let defaults, !defaults.bool(forKey: Self.turnedOffKey) else { return }
        if isEnabled { isEnabled = false }
        defaults.set(true, forKey: Self.turnedOffKey)
    }

    static let launchArgument = "--experiment-typing-logs"

    /// Simulator scenes pass `--experiment-typing-logs` to turn the experiment on.
    func applyLaunchArguments(_ arguments: [String] = ProcessInfo.processInfo.arguments) {
        if arguments.contains(Self.launchArgument) {
            isEnabled = true
        }
    }
}

/// The field's text input traits, as names, for `session_start` and the skip rule.
nonisolated struct TypingLogTraits: Equatable, Sendable {
    var keyboardType = "default"
    var returnKeyType = "default"
    var autocapitalization = "sentences"
    var autocorrection = "default"
    var spellChecking = "default"
    var smartQuotes = "default"
    var smartDashes = "default"
    var smartInsertDelete = "default"
    var inlinePrediction = "default"
    var keyboardAppearance = "default"
    var textContentType: String?
    var isSecure = false
    var enablesReturnKeyAutomatically = false
    /// `UITextDocumentProxy.documentIdentifier`: changes with the field, not the app.
    var document: String?

    /// Why nothing is logged in this field, or nil to log it.
    var skipReason: String? {
        TypingLogPrivacy.skipReason(isSecure: isSecure, textContentType: textContentType.map(UITextContentType.init(rawValue:)))
    }

    /// Changes when the keyboard moves to another field or the field's traits change.
    var signature: String {
        [
            keyboardType, returnKeyType, autocapitalization, autocorrection, spellChecking, smartQuotes,
            smartDashes, smartInsertDelete, inlinePrediction, keyboardAppearance, textContentType ?? "",
            isSecure ? "secure" : "", enablesReturnKeyAutomatically ? "auto-return" : "", document ?? "",
        ].joined(separator: "|")
    }

    var fields: [TypingLogField] {
        [
            TypingLogField("keyboardType", .string(keyboardType)),
            TypingLogField("returnKeyType", .string(returnKeyType)),
            TypingLogField("autocapitalization", .string(autocapitalization)),
            TypingLogField("autocorrection", .string(autocorrection)),
            TypingLogField("spellChecking", .string(spellChecking)),
            TypingLogField("smartQuotes", .string(smartQuotes)),
            TypingLogField("smartDashes", .string(smartDashes)),
            TypingLogField("smartInsertDelete", .string(smartInsertDelete)),
            TypingLogField("inlinePrediction", .string(inlinePrediction)),
            TypingLogField("keyboardAppearance", .string(keyboardAppearance)),
            TypingLogField("textContentType", .text(textContentType)),
            TypingLogField("secure", .bool(isSecure)),
            TypingLogField("enablesReturnKeyAutomatically", .bool(enablesReturnKeyAutomatically)),
        ]
    }
}

extension TypingLogTraits {
    @MainActor
    init(_ traits: UITextInputTraits, document: UUID? = nil) {
        keyboardType = Self.name(traits.keyboardType ?? .default)
        returnKeyType = Self.name(traits.returnKeyType ?? .default)
        autocapitalization = Self.name(traits.autocapitalizationType ?? .sentences)
        autocorrection = Self.name(traits.autocorrectionType ?? .default)
        spellChecking = Self.name(traits.spellCheckingType ?? .default)
        smartQuotes = Self.name(traits.smartQuotesType ?? .default)
        smartDashes = Self.name(traits.smartDashesType ?? .default)
        smartInsertDelete = Self.name(traits.smartInsertDeleteType ?? .default)
        inlinePrediction = Self.name(traits.inlinePredictionType ?? .default)
        keyboardAppearance = Self.name(traits.keyboardAppearance ?? .default)
        textContentType = traits.textContentType??.rawValue
        isSecure = traits.isSecureTextEntry ?? false
        enablesReturnKeyAutomatically = traits.enablesReturnKeyAutomatically ?? false
        self.document = document?.uuidString
    }

    /// `UITextDocumentProxy.documentIdentifier`, or nil while UIKit has none. UIKit declares it
    /// non-optional, but the keyboard's proxy returns nil early on (in `viewDidLoad`, and at
    /// times later), and reading that nil as Swift's `UUID` traps (Debug keyboard crashes in
    /// `viewDidLoad` and on a field change). Read through key-value coding instead.
    static func documentIdentifier(of proxy: AnyObject) -> UUID? {
        guard let object = proxy as? NSObject,
              object.responds(to: NSSelectorFromString("documentIdentifier")) else { return nil }
        return object.value(forKey: "documentIdentifier") as? UUID
    }

    /// Whether `next` is another field, or this field with other traits. A document UIKit
    /// couldn't name yet (nil) matches any, so a field isn't split in two when it's named.
    func isOtherField(than next: TypingLogTraits) -> Bool {
        var current = self
        var next = next
        if current.document == nil || next.document == nil {
            current.document = nil
            next.document = nil
        }
        return current.signature != next.signature
    }

    static func name(_ type: UIKeyboardType) -> String {
        switch type {
        case .default: "default"
        case .asciiCapable: "asciiCapable"
        case .numbersAndPunctuation: "numbersAndPunctuation"
        case .URL: "URL"
        case .numberPad: "numberPad"
        case .phonePad: "phonePad"
        case .namePhonePad: "namePhonePad"
        case .emailAddress: "emailAddress"
        case .decimalPad: "decimalPad"
        case .twitter: "twitter"
        case .webSearch: "webSearch"
        case .asciiCapableNumberPad: "asciiCapableNumberPad"
        @unknown default: "raw:\(type.rawValue)"
        }
    }

    static func name(_ type: UIReturnKeyType) -> String {
        switch type {
        case .default: "default"
        case .go: "go"
        case .google: "google"
        case .join: "join"
        case .next: "next"
        case .route: "route"
        case .search: "search"
        case .send: "send"
        case .yahoo: "yahoo"
        case .done: "done"
        case .emergencyCall: "emergencyCall"
        case .continue: "continue"
        @unknown default: "raw:\(type.rawValue)"
        }
    }

    static func name(_ type: UITextAutocapitalizationType) -> String {
        switch type {
        case .none: "none"
        case .words: "words"
        case .sentences: "sentences"
        case .allCharacters: "allCharacters"
        @unknown default: "raw:\(type.rawValue)"
        }
    }

    static func name(_ type: UITextAutocorrectionType) -> String {
        switch type {
        case .default: "default"
        case .no: "no"
        case .yes: "yes"
        @unknown default: "raw:\(type.rawValue)"
        }
    }

    static func name(_ type: UITextSpellCheckingType) -> String {
        switch type {
        case .default: "default"
        case .no: "no"
        case .yes: "yes"
        @unknown default: "raw:\(type.rawValue)"
        }
    }

    static func name(_ type: UITextSmartQuotesType) -> String {
        switch type {
        case .default: "default"
        case .no: "no"
        case .yes: "yes"
        @unknown default: "raw:\(type.rawValue)"
        }
    }

    static func name(_ type: UITextSmartDashesType) -> String {
        switch type {
        case .default: "default"
        case .no: "no"
        case .yes: "yes"
        @unknown default: "raw:\(type.rawValue)"
        }
    }

    static func name(_ type: UITextSmartInsertDeleteType) -> String {
        switch type {
        case .default: "default"
        case .no: "no"
        case .yes: "yes"
        @unknown default: "raw:\(type.rawValue)"
        }
    }

    static func name(_ type: UITextInlinePredictionType) -> String {
        switch type {
        case .default: "default"
        case .no: "no"
        case .yes: "yes"
        @unknown default: "raw:\(type.rawValue)"
        }
    }

    static func name(_ type: UIKeyboardAppearance) -> String {
        switch type {
        case .default: "default"
        case .dark: "dark"
        case .light: "light"
        @unknown default: "raw:\(type.rawValue)"
        }
    }
}

/// Fields nothing is logged in. iOS never shows a third-party keyboard in a secure field,
/// but a field can still ask for a password, a code, or a card number without being secure.
nonisolated enum TypingLogPrivacy {
    static let skippedContentTypes: Set<UITextContentType> = [
        .password, .newPassword, .oneTimeCode,
        .creditCardNumber, .creditCardSecurityCode, .creditCardType,
        .creditCardExpiration, .creditCardExpirationMonth, .creditCardExpirationYear,
        .creditCardName, .creditCardGivenName, .creditCardMiddleName, .creditCardFamilyName,
    ]

    static func skipReason(isSecure: Bool, textContentType: UITextContentType?) -> String? {
        if isSecure { return "secureTextEntry" }
        if let type = textContentType, skippedContentTypes.contains(type) {
            return "textContentType:\(type.rawValue)"
        }
        return nil
    }
}

/// What a session line says about the keyboard and the iPhone. The host app can't be
/// known from a keyboard extension, so it is never guessed.
nonisolated struct TypingLogSessionInfo: Sendable {
    /// `appear`, `field` (another field or new traits), or `enabled` (turned on while shown).
    var trigger: String
    /// `full` (Hex's keys) or `bar` (the dictation bar).
    var surface: String
    var traits: TypingLogTraits
    var bounds: CGSize
    var safeArea: UIEdgeInsets
    var orientation: String
    var appearance: String
    var device: String
    var system: String
    var version: String
    var build: String
    var locale: String
    var languages: [String]
    /// The keyboard's own language (`primaryLanguage`) and the field's input mode.
    var keyboardLanguage: String?
    var inputMode: String?
    var fullAccess: Bool
    /// Other switches that change what the keys do.
    var settings: [TypingLogField]

    var fields: [TypingLogField] {
        [
            TypingLogField("trigger", .string(trigger)),
            TypingLogField("surface", .string(surface)),
            TypingLogField("document", .text(traits.document)),
            TypingLogField("traits", .object(traits.fields)),
            TypingLogField("bounds", .array([.pt(bounds.width), .pt(bounds.height)])),
            TypingLogField("safeArea", Self.insets(safeArea)),
            TypingLogField("orientation", .string(orientation)),
            TypingLogField("appearance", .string(appearance)),
            TypingLogField("device", .string(device)),
            TypingLogField("system", .string(system)),
            TypingLogField("version", .string(version)),
            TypingLogField("build", .string(build)),
            TypingLogField("locale", .string(locale)),
            TypingLogField("languages", .strings(languages)),
            TypingLogField("keyboardLanguage", .text(keyboardLanguage)),
            TypingLogField("inputMode", .text(inputMode)),
            TypingLogField("fullAccess", .bool(fullAccess)),
            TypingLogField("settings", .object(settings)),
        ]
    }

    /// `[top, left, bottom, right]` in points.
    static func insets(_ insets: UIEdgeInsets) -> TypingLogValue {
        .array([.pt(insets.top), .pt(insets.left), .pt(insets.bottom), .pt(insets.right)])
    }

    /// The hardware model (`iPhone17,1`); the simulated one in the simulator.
    static var deviceModel: String {
        if let simulated = ProcessInfo.processInfo.environment["SIMULATOR_MODEL_IDENTIFIER"] {
            return "Simulator:\(simulated)"
        }
        var system = utsname()
        uname(&system)
        return withUnsafeBytes(of: &system.machine) { raw in
            String(decoding: raw.prefix { $0 != 0 }, as: UTF8.self)
        }
    }
}

/// One `UITouch` sample, copied so tests can feed the same path.
nonisolated struct TypingLogTouchSample: Equatable, Sendable {
    var point: CGPoint
    /// `UITouch.timestamp`, seconds on the `CACurrentMediaTime` clock.
    var timestamp: TimeInterval
    var majorRadius: CGFloat = 0
    var majorRadiusTolerance: CGFloat = 0
    var force: CGFloat = 0
    var maximumPossibleForce: CGFloat = 0
}

extension TypingLogTouchSample {
    @MainActor
    init(_ touch: UITouch, in view: UIView) {
        point = touch.location(in: view)
        timestamp = touch.timestamp
        majorRadius = touch.majorRadius
        majorRadiusTolerance = touch.majorRadiusTolerance
        force = touch.force
        maximumPossibleForce = touch.maximumPossibleForce
    }
}

nonisolated enum TypingLogTouchPhase: String, Sendable {
    case began, moved, ended, cancelled
}

/// The text around the cursor, as the host shows it.
nonisolated struct TypingLogContext: Equatable, Sendable {
    var before: String?
    var after: String?
    var hasText: Bool
}
