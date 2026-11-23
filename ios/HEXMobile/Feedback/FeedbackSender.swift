import Foundation
import Observation
import os

/// Sends one feedback request, with its upload's progress, retrying as `FeedbackRetry` says
/// with the same body and Idempotency-Key, so a retry can never make a second issue.
@MainActor
@Observable
final class FeedbackSender {
    /// 0…1 while the body uploads.
    private(set) var progress: Double = 0
    private(set) var attempt = 0
    private let log = Logger(subsystem: "anomaly.hex.mobile", category: "feedback")

    func send(_ request: FeedbackRequest, key: String) async -> FeedbackOutcome {
        let key = Self.override("--feedback-key") ?? key
        let endpoint = Self.override("--feedback-endpoint").flatMap(URL.init(string:)) ?? Feedback.endpoint
        Self.record(request)
        attempt = 0
        while true {
            attempt += 1
            progress = 0
            var status: Int?
            var code: String?
            var retryAfter: String?
            do {
                let delegate = FeedbackUploadProgress { [weak self] fraction in
                    Task { @MainActor in self?.progress = fraction }
                }
                let (data, response) = try await URLSession.shared.upload(for: request.urlRequest(key: key, endpoint: endpoint), from: request.body, delegate: delegate)
                let http = response as? HTTPURLResponse
                Self.recordAnswer(status: http?.statusCode ?? 0, body: data, attempt: attempt)
                if Self.dropsResponse(attempt: attempt) { throw URLError(.networkConnectionLost) }
                status = http?.statusCode
                retryAfter = http?.value(forHTTPHeaderField: "Retry-After")
                let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
                if status == 201, let number = json?["number"] as? Int {
                    progress = 1
                    let email = json?["email"] as? String
                    return .sent(number: number, emailPending: email == "pending")
                }
                code = (json?["error"] as? [String: Any])?["code"] as? String
                log.error("feedback answered \(status ?? 0, privacy: .public) \(code ?? "", privacy: .public)")
            } catch {
                log.error("feedback attempt \(self.attempt, privacy: .public) failed: \(error.localizedDescription, privacy: .public)")
                Self.recordError(error, attempt: attempt)
            }
            if let wait = FeedbackRetry.delay(afterStatus: status, code: code, retryAfter: retryAfter, attempt: attempt) {
                try? await Task.sleep(for: .seconds(wait))
                continue
            }
            guard let status else { return .failed(.offline) }
            let failure = FeedbackRetry.failure(status: status, code: code)
            if failure == .unavailable {
                // The report key's setup (revoked, or feedback not allowed): the team's to fix.
                log.fault("feedback is unavailable: \(status, privacy: .public) \(code ?? "", privacy: .public)")
            }
            return .failed(failure)
        }
    }

    // MARK: - Simulator proof (debug builds only)

    /// `--feedback-key`, `--feedback-endpoint`: a wrong key, an unreachable address.
    private static func override(_ name: String) -> String? {
        #if DEBUG
        let args = ProcessInfo.processInfo.arguments
        guard let i = args.firstIndex(of: name), i + 1 < args.count else { return nil }
        return args[i + 1]
        #else
        return nil
        #endif
    }

    /// `--feedback-drop-first-response`: the first answer is lost as a dropped connection is.
    private static func dropsResponse(attempt: Int) -> Bool {
        #if DEBUG
        return attempt == 1 && ProcessInfo.processInfo.arguments.contains("--feedback-drop-first-response")
        #else
        return false
        #endif
    }

    /// `--feedback-log-request`: what was sent, without the key, in Documents/feedback-requests.
    private static func record(_ request: FeedbackRequest) {
        #if DEBUG
        guard ProcessInfo.processInfo.arguments.contains("--feedback-log-request") else { return }
        let headers = request.urlRequest(key: "<report key>").allHTTPHeaderFields ?? [:]
        let summary: [String: Any] = [
            "headers": headers,
            "payload": (try? JSONSerialization.jsonObject(with: request.payload)) ?? [:],
            "bodyBytes": request.body.count,
            "screenshots": request.pictures,
        ]
        write(summary, suffix: "request")
        #endif
    }

    private static func recordAnswer(status: Int, body: Data, attempt: Int) {
        #if DEBUG
        guard ProcessInfo.processInfo.arguments.contains("--feedback-log-request") else { return }
        let answer = (try? JSONSerialization.jsonObject(with: body)) ?? String(decoding: body, as: UTF8.self)
        write(["attempt": attempt, "status": status, "body": answer, "dropped": dropsResponse(attempt: attempt)], suffix: "answer")
        #endif
    }

    private static func recordError(_ error: Error, attempt: Int) {
        #if DEBUG
        guard ProcessInfo.processInfo.arguments.contains("--feedback-log-request") else { return }
        write(["attempt": attempt, "error": String(describing: error)], suffix: "error")
        #endif
    }

    #if DEBUG
    private static func write(_ object: [String: Any], suffix: String) {
        let folder = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent("feedback-requests")
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let name = "\(Int(Date().timeIntervalSince1970 * 1000))-\(suffix).json"
        if let data = try? JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys]) {
            try? data.write(to: folder.appendingPathComponent(name))
        }
    }
    #endif
}

/// The upload's progress, from URLSession's task delegate.
nonisolated final class FeedbackUploadProgress: NSObject, URLSessionTaskDelegate, Sendable {
    private let report: @Sendable (Double) -> Void

    init(report: @escaping @Sendable (Double) -> Void) {
        self.report = report
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didSendBodyData bytesSent: Int64,
                    totalBytesSent: Int64, totalBytesExpectedToSend: Int64) {
        guard totalBytesExpectedToSend > 0 else { return }
        report(Double(totalBytesSent) / Double(totalBytesExpectedToSend))
    }
}
