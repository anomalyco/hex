import Foundation
import ImageIO
import UIKit

/// Send feedback: one POST to Bellhop, Anomaly's feedback inbox (https://blhp.app/docs/feedback).
/// Only the Hex app sends it, only when the person taps Send; the keyboard has none of this.
nonisolated enum Feedback {
    static let endpoint = URL(string: "https://api.blhp.app/v1/feedback")!
    /// Info.plist's BellhopReportKey, from the BELLHOP_REPORT_KEY build setting
    /// (Feedback.xcconfig, set by a gitignored Feedback.local.xcconfig or Xcode Cloud).
    static let infoKey = "BellhopReportKey"

    static let messageLimit = 20_000
    static let subjectLimit = 200
    static let imageLimit = 5
    static let imageBytesLimit = 10 * 1024 * 1024
    static let imagesBytesLimit = 20 * 1024 * 1024
    /// Longest side of a picture as sent (the hard limit is 8,192 px).
    static let imageSide: CGFloat = 2048
    static let jpegQuality: CGFloat = 0.8

    /// The report key, or nil in a build without one (the form isn't offered then).
    static var reportKey: String? {
        guard let key = Bundle.main.object(forInfoDictionaryKey: infoKey) as? String else { return nil }
        let trimmed = key.trimmingCharacters(in: .whitespaces)
        return trimmed.hasPrefix("bh_pub_") ? trimmed : nil
    }

    static var isAvailable: Bool { reportKey != nil }

    enum Category: String, CaseIterable, Sendable {
        case bug, idea, question
    }

    /// The install id: a random UUID in Hex's own UserDefaults, made the first time it's needed.
    /// Never a hardware, vendor or advertising id.
    static func installID(defaults: UserDefaults = .standard) -> String {
        let key = "feedback.installID"
        if let id = defaults.string(forKey: key), !id.isEmpty { return id }
        let id = UUID().uuidString.lowercased()
        defaults.set(id, forKey: key)
        return id
    }

    /// The model identifier (`iPhone17,1`), the simulated one on a simulator.
    static var modelIdentifier: String {
        if let simulated = ProcessInfo.processInfo.environment["SIMULATOR_MODEL_IDENTIFIER"] { return simulated }
        var system = utsname()
        uname(&system)
        return withUnsafeBytes(of: &system.machine) { String(decoding: $0.prefix { $0 != 0 }, as: UTF8.self) }
    }
}

/// What a person typed and chose in the form.
nonisolated struct FeedbackDraft: Equatable, Sendable {
    var message = ""
    var subject = ""
    var category: Feedback.Category?
    var email = ""
    var anonymous = false
    /// JPEG data, already shrunk (`FeedbackImage.jpeg`).
    var images: [Data] = []

    enum Problem: Equatable, Sendable {
        case emptyMessage
        case messageTooLong
        case subjectTooLong
        case tooManyImages
        case imageTooLarge
        case imagesTooLarge
        case invalidEmail
    }

    var trimmedMessage: String { message.trimmingCharacters(in: .whitespacesAndNewlines) }
    var trimmedEmail: String { email.trimmingCharacters(in: .whitespacesAndNewlines) }

    /// What stops it being sent, checked before anything goes out.
    var problem: Problem? {
        if trimmedMessage.isEmpty { return .emptyMessage }
        if trimmedMessage.count > Feedback.messageLimit { return .messageTooLong }
        if subject.trimmingCharacters(in: .whitespacesAndNewlines).count > Feedback.subjectLimit { return .subjectTooLong }
        if images.count > Feedback.imageLimit { return .tooManyImages }
        if images.contains(where: { $0.count > Feedback.imageBytesLimit }) { return .imageTooLarge }
        if images.reduce(0, { $0 + $1.count }) > Feedback.imagesBytesLimit { return .imagesTooLarge }
        if !anonymous, !trimmedEmail.isEmpty, !Self.looksLikeEmail(trimmedEmail) { return .invalidEmail }
        return nil
    }

    static func looksLikeEmail(_ text: String) -> Bool {
        let parts = text.split(separator: "@", omittingEmptySubsequences: false)
        return parts.count == 2 && !parts[0].isEmpty && parts[1].contains(".") && !parts[1].hasPrefix(".")
            && !parts[1].hasSuffix(".") && !text.contains(" ") && text.count <= 254
    }
}

/// Everything about the app and iPhone a request carries, read once when the form opens.
nonisolated struct FeedbackContext: Equatable, Sendable {
    var appVersion: String
    var appBuild: String
    var os: String
    var model: String
    var locale: String
    /// Where the form was opened ("Home", "Settings › Experiments").
    var screen: String
    /// For the team, internal names: never shown in the form.
    var dictationLanguage: String?
    var speechModel: String?

    @MainActor
    static func current(screen: String, dictation: DictationController? = nil) -> FeedbackContext {
        let info = Bundle.main.infoDictionary
        return FeedbackContext(
            appVersion: info?["CFBundleShortVersionString"] as? String ?? "",
            appBuild: info?["CFBundleVersion"] as? String ?? "",
            os: "iOS \(UIDevice.current.systemVersion)",
            model: Feedback.modelIdentifier,
            locale: Locale.current.identifier(.bcp47),
            screen: screen,
            dictationLanguage: dictation?.transcriptionLanguageID,
            speechModel: dictation.map { String(describing: $0.engine) }
        )
    }
}

/// The request as Bellhop's checklist asks for it: a `payload` JSON part and up to five
/// `screenshot` JPEG parts, with an Idempotency-Key made when the person tapped Send.
nonisolated struct FeedbackRequest: Sendable {
    let payload: Data
    let body: Data
    let boundary: String
    let idempotencyKey: String
    /// Each screenshot part's bytes and pixel size, for the simulator's request log.
    let pictures: [[String: Int]]

    /// The payload, with nothing Hex keeps on the phone: no typing logs, dictations, vocabulary
    /// or learned words, no `logs` and no `user` (Hex has no accounts). Anonymous: no email
    /// and no install id, and metadata holds nothing identifying.
    static func payload(_ draft: FeedbackDraft, context: FeedbackContext, installID: String) -> [String: Any] {
        var out: [String: Any] = [
            "message": draft.trimmedMessage,
            "app": ["name": "Hex", "version": context.appVersion, "build": context.appBuild],
            "device": ["platform": "ios", "os": context.os, "model": context.model, "locale": context.locale],
        ]
        let subject = draft.subject.trimmingCharacters(in: .whitespacesAndNewlines)
        if !subject.isEmpty { out["subject"] = subject }
        if let category = draft.category { out["category"] = category.rawValue }
        var metadata: [String: String] = ["screen": context.screen]
        if let language = context.dictationLanguage, !language.isEmpty { metadata["dictation_language"] = language }
        if let model = context.speechModel, !model.isEmpty { metadata["speech_model"] = model }
        out["metadata"] = metadata
        if draft.anonymous {
            out["anonymous"] = true
        } else {
            if !draft.trimmedEmail.isEmpty { out["email"] = draft.trimmedEmail }
            out["install_id"] = installID
        }
        return out
    }

    init(draft: FeedbackDraft, context: FeedbackContext, installID: String, idempotencyKey: String = UUID().uuidString,
         boundary: String = "hex-feedback-\(UUID().uuidString)") {
        let object = Self.payload(draft, context: context, installID: installID)
        payload = (try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])) ?? Data()
        self.boundary = boundary
        self.idempotencyKey = idempotencyKey
        pictures = draft.images.map { data in
            let source = CGImageSourceCreateWithData(data as CFData, nil)
            let properties = source.flatMap { CGImageSourceCopyPropertiesAtIndex($0, 0, nil) } as? [CFString: Any]
            return [
                "bytes": data.count,
                "width": properties?[kCGImagePropertyPixelWidth] as? Int ?? 0,
                "height": properties?[kCGImagePropertyPixelHeight] as? Int ?? 0,
            ]
        }
        var body = Data()
        func line(_ s: String) { body.append(Data((s + "\r\n").utf8)) }
        line("--\(boundary)")
        line("Content-Disposition: form-data; name=\"payload\"")
        line("Content-Type: application/json")
        line("")
        body.append(payload)
        line("")
        for (n, image) in draft.images.enumerated() {
            line("--\(boundary)")
            line("Content-Disposition: form-data; name=\"screenshot\"; filename=\"hex-\(n + 1).jpg\"")
            line("Content-Type: image/jpeg")
            line("")
            body.append(image)
            line("")
        }
        line("--\(boundary)--")
        self.body = body
    }

    func urlRequest(key: String, endpoint: URL = Feedback.endpoint) -> URLRequest {
        var request = URLRequest(url: endpoint, timeoutInterval: 60)
        request.httpMethod = "POST"
        request.setValue("Bearer \(key)", forHTTPHeaderField: "Authorization")
        request.setValue("multipart/form-data; boundary=\(boundary)", forHTTPHeaderField: "Content-Type")
        request.setValue(idempotencyKey, forHTTPHeaderField: "Idempotency-Key")
        return request
    }
}

/// What came back, and whether to try again.
nonisolated enum FeedbackOutcome: Equatable, Sendable {
    /// 201: the issue's number, and whether an email waits for the team's first reply.
    case sent(number: Int, emailPending: Bool)
    case failed(FeedbackFailure)
}

nonisolated enum FeedbackFailure: Equatable, Sendable {
    /// No connection, or it dropped: retried, then this.
    case offline
    /// 413: a picture or the whole is too large.
    case tooLarge
    /// 415: a picture isn't a complete PNG, JPEG or WebP image.
    case unsupportedImage
    /// 429 (after its retries).
    case tryLater
    /// 401, 403 feedback_disabled: the app's setup, not the person's.
    case unavailable
    /// 400, 422 and anything else.
    case other(status: Int, code: String?)
}

/// When to send again: network failures and 5xx a few times with backoff; 409 request_pending
/// and 429 after Retry-After; no other 4xx.
nonisolated enum FeedbackRetry {
    static let attempts = 4

    /// Seconds to wait before attempt `attempt` (2, 3, 4) after a failure, or nil to stop.
    static func delay(afterStatus status: Int?, code: String?, retryAfter: String?, attempt: Int) -> Double? {
        guard attempt < attempts else { return nil }
        let backoff = pow(2.0, Double(attempt - 1))
        switch status {
        case nil:
            return backoff
        case 409 where code == "request_pending", 429:
            let seconds = retryAfter.flatMap(Double.init) ?? backoff
            // A wait past a minute isn't worth holding the sheet for.
            return seconds <= 60 ? max(seconds, 0.5) : nil
        case let s? where s >= 500:
            return backoff
        default:
            return nil
        }
    }

    static func failure(status: Int, code: String?) -> FeedbackFailure {
        switch status {
        case 413: .tooLarge
        case 415: .unsupportedImage
        case 429: .tryLater
        case 401: .unavailable
        case 403: code == "blocked" ? .other(status: 403, code: code) : .unavailable
        default: .other(status: status, code: code)
        }
    }
}
