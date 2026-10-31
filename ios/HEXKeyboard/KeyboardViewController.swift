import SwiftUI
import UIKit

@MainActor
final class KeyboardViewController: UIInputViewController {
    private let bridge = KeyboardBridge()
    private let viewModel = KeyboardViewModel()
    private var pollTimer: Timer?
    private var coldStartJobID = UUID().uuidString
    private var lastURLRefreshBucket: Int = 0
    private var lastFullAccessNote: TimeInterval = 0

    override func viewDidLoad() {
        super.viewDidLoad()
        configureView()
        syncFromBridge()
    }

    override func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        coldStartJobID = UUID().uuidString
        syncFromBridge()

        pollTimer?.invalidate()
        pollTimer = Timer.scheduledTimer(withTimeInterval: 0.04, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.syncFromBridge()
            }
        }
    }

    override func viewWillDisappear(_ animated: Bool) {
        super.viewWillDisappear(animated)
        pollTimer?.invalidate()
        pollTimer = nil
    }

    override func textDidChange(_ textInput: UITextInput?) {
        super.textDidChange(textInput)
        updateKeyboardTraits()
    }

    private func configureView() {
        view.backgroundColor = UIColor(red: 0x0A / 255.0, green: 0x0B / 255.0, blue: 0x0D / 255.0, alpha: 1)
        preferredContentSize.height = 148

        viewModel.inputViewController = self
        viewModel.onDictateTap = { [weak self] in self?.startWarmDictation() }
        viewModel.onStopTap = { [weak self] in self?.stopWarmDictation() }
        viewModel.onCancelTap = { [weak self] in self?.cancelWarmDictation() }
        viewModel.onStyleSelected = { [weak self] style in
            self?.bridge.style = style
            self?.viewModel.style = style
            self?.refreshColdStartURL(now: Date().timeIntervalSince1970, force: true)
        }
        viewModel.onReturnAppSelected = { [weak self] appID in
            self?.bridge.returnAppID = appID
            self?.viewModel.returnAppID = appID
            self?.refreshColdStartURL(now: Date().timeIntervalSince1970, force: true)
        }
        viewModel.onSpace = { [weak self] in self?.textDocumentProxy.insertText(" ") }
        viewModel.onDelete = { [weak self] in self?.textDocumentProxy.deleteBackward() }
        viewModel.onReturn = { [weak self] in self?.textDocumentProxy.insertText("\n") }
        viewModel.onOpenURL = { [weak self] url in
            guard let self else { return }
            ContainingAppOpener.open(url, from: self)
        }

        let hosting = UIHostingController(rootView: KeyboardSurfaceView(model: viewModel))
        hosting.view.translatesAutoresizingMaskIntoConstraints = false
        hosting.view.backgroundColor = .clear
        addChild(hosting)
        view.addSubview(hosting.view)
        hosting.didMove(toParent: self)

        let heightConstraint = view.heightAnchor.constraint(equalToConstant: 148)
        heightConstraint.priority = UILayoutPriority(999)

        NSLayoutConstraint.activate([
            heightConstraint,
            hosting.view.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            hosting.view.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            hosting.view.topAnchor.constraint(equalTo: view.topAnchor),
            hosting.view.bottomAnchor.constraint(equalTo: view.bottomAnchor),
        ])
    }

    private func syncFromBridge() {
        let now = Date().timeIntervalSince1970
        viewModel.now = now
        viewModel.hasFullAccess = hasFullAccess
        updateKeyboardTraits()

        guard hasFullAccess else { return }

        if now - lastFullAccessNote >= 5 {
            lastFullAccessNote = now
            bridge.fullAccessSeenAt = now
        }

        let snap = bridge.snapshot()
        viewModel.snapshot = snap
        viewModel.isModelInstalled = bridge.isModelInstalled
        viewModel.style = bridge.style
        viewModel.returnAppID = bridge.returnAppID

        if snap.hasDeliverableResult(at: now),
           let resultID = snap.resultID,
           let rawTranscript = snap.transcript,
           bridge.lastInsertedResultID != resultID {
            let insertion = TranscriptFormatter.insertionText(
                for: rawTranscript,
                beforeContext: textDocumentProxy.documentContextBeforeInput,
                afterContext: textDocumentProxy.documentContextAfterInput
            )
            textDocumentProxy.insertText(insertion)
            bridge.lastInsertedResultID = resultID
            viewModel.pendingCommand = nil
            coldStartJobID = UUID().uuidString
        }

        if let pending = viewModel.pendingCommand {
            let expired = now - pending.sentAt >= KeyboardViewModel.commandAckTimeout
            let acknowledged = (pending.kind == .start && snap.state == .recording)
                || (pending.kind == .stop && snap.state != .recording)
                || (pending.kind == .cancel && snap.state != .recording)
            if expired || acknowledged {
                viewModel.pendingCommand = nil
            }
        }

        refreshColdStartURL(now: now, force: false)
    }

    private func refreshColdStartURL(now: TimeInterval, force: Bool) {
        let bucket = Int(now / 2.0)
        guard force || bucket != lastURLRefreshBucket else { return }
        lastURLRefreshBucket = bucket

        let request: KeyboardLaunchRequest
        if bridge.isModelInstalled {
            request = .startRecording(
                jobID: coldStartJobID,
                requestedAt: now,
                style: bridge.style,
                returnAppID: bridge.returnAppID
            )
        } else {
            request = .startSession(
                requestedAt: now,
                returnAppID: bridge.returnAppID
            )
        }
        viewModel.coldStartURL = request.url
    }

    private func updateKeyboardTraits() {
        viewModel.needsGlobeKey = needsInputModeSwitchKey
        switch textDocumentProxy.returnKeyType {
        case .send: viewModel.returnKeyLabel = "Send"
        case .search: viewModel.returnKeyLabel = "Search"
        case .go: viewModel.returnKeyLabel = "Go"
        case .done: viewModel.returnKeyLabel = "Done"
        default: viewModel.returnKeyLabel = "Return"
        }
    }

    private func startWarmDictation() {
        let now = Date().timeIntervalSince1970
        guard hasFullAccess, viewModel.snapshot.isAvailable(at: now) else { return }
        let jobID = UUID().uuidString
        viewModel.pendingCommand = KeyboardViewModel.PendingCommand(
            kind: .start,
            jobID: jobID,
            sentAt: now
        )
        bridge.send(.start, jobID: jobID, style: viewModel.style, sentAt: now)
    }

    private func stopWarmDictation() {
        let now = Date().timeIntervalSince1970
        guard let jobID = viewModel.snapshot.jobID ?? viewModel.pendingCommand?.jobID else { return }
        viewModel.pendingCommand = KeyboardViewModel.PendingCommand(
            kind: .stop,
            jobID: jobID,
            sentAt: now
        )
        bridge.send(.stop, jobID: jobID, style: viewModel.style, sentAt: now)
    }

    private func cancelWarmDictation() {
        let now = Date().timeIntervalSince1970
        guard let jobID = viewModel.snapshot.jobID ?? viewModel.pendingCommand?.jobID else { return }
        viewModel.pendingCommand = KeyboardViewModel.PendingCommand(
            kind: .cancel,
            jobID: jobID,
            sentAt: now
        )
        bridge.send(.cancel, jobID: jobID, style: viewModel.style, sentAt: now)
    }
}
