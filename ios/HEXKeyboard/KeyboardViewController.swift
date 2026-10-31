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
        syncFromBridge(animated: false)
    }

    override func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        coldStartJobID = UUID().uuidString
        syncFromBridge(animated: false)

        pollTimer?.invalidate()
        pollTimer = Timer.scheduledTimer(withTimeInterval: 0.04, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.syncFromBridge(animated: true)
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
        view.backgroundColor = .clear
        preferredContentSize.height = 140

        viewModel.inputViewController = self
        viewModel.onDictateTap = { [weak self] in self?.startWarmDictation() }
        viewModel.onStopTap = { [weak self] in self?.stopWarmDictation() }
        viewModel.onCancelTap = { [weak self] in self?.cancelWarmDictation() }
        viewModel.onStyleSelected = { [weak self] style in
            guard let self else { return }
            self.bridge.style = style
            if self.viewModel.style != style {
                self.viewModel.style = style
            }
            self.refreshColdStartURL(now: Date().timeIntervalSince1970, force: true)
        }
        viewModel.onReturnAppSelected = { [weak self] appID in
            guard let self else { return }
            self.bridge.returnAppID = appID
            if self.viewModel.returnAppID != appID {
                self.viewModel.returnAppID = appID
            }
            self.refreshColdStartURL(now: Date().timeIntervalSince1970, force: true)
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

        let heightConstraint = view.heightAnchor.constraint(equalToConstant: 140)
        heightConstraint.priority = UILayoutPriority(999)

        NSLayoutConstraint.activate([
            heightConstraint,
            hosting.view.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            hosting.view.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            hosting.view.topAnchor.constraint(equalTo: view.topAnchor),
            hosting.view.bottomAnchor.constraint(equalTo: view.bottomAnchor),
        ])
    }

    private func syncFromBridge(animated: Bool) {
        let now = Date().timeIntervalSince1970
        updateKeyboardTraits()

        #if targetEnvironment(simulator)
        let simSnap = bridge.snapshot()
        let effectiveFullAccess = hasFullAccess || simSnap.isAvailable(at: now)
        #else
        let effectiveFullAccess = hasFullAccess
        #endif

        guard effectiveFullAccess else {
            viewModel.applyBridgeState(
                hasFullAccess: false,
                snapshot: .offline,
                isModelInstalled: false,
                style: viewModel.style,
                returnAppID: viewModel.returnAppID,
                now: now,
                clearPending: true,
                animated: animated
            )
            return
        }

        if now - lastFullAccessNote >= 5 {
            lastFullAccessNote = now
            bridge.fullAccessSeenAt = now
        }

        let snap = bridge.snapshot()
        var insertedResult = false

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
            insertedResult = true
            coldStartJobID = UUID().uuidString
        }

        viewModel.applyBridgeState(
            hasFullAccess: true,
            snapshot: snap,
            isModelInstalled: bridge.isModelInstalled,
            style: bridge.style,
            returnAppID: bridge.returnAppID,
            now: now,
            clearPending: insertedResult,
            animated: animated
        )

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
        let globe = needsInputModeSwitchKey
        if viewModel.needsGlobeKey != globe {
            viewModel.needsGlobeKey = globe
        }
        let nextReturn: String = switch textDocumentProxy.returnKeyType {
        case .send: "Send"
        case .search: "Search"
        case .go: "Go"
        case .done: "Done"
        default: "Return"
        }
        if viewModel.returnKeyLabel != nextReturn {
            viewModel.returnKeyLabel = nextReturn
        }
    }

    private func startWarmDictation() {
        let now = Date().timeIntervalSince1970
        guard viewModel.hasFullAccess, viewModel.currentSnapshot.isAvailable(at: now) else { return }
        let jobID = UUID().uuidString
        viewModel.setPendingCommand(
            KeyboardViewModel.PendingCommand(kind: .start, jobID: jobID, sentAt: now),
            now: now,
            animated: true
        )
        bridge.send(.start, jobID: jobID, style: viewModel.style, sentAt: now)
    }

    private func stopWarmDictation() {
        let now = Date().timeIntervalSince1970
        guard let jobID = viewModel.currentSnapshot.jobID ?? viewModel.pendingCommand?.jobID else { return }
        viewModel.setPendingCommand(
            KeyboardViewModel.PendingCommand(kind: .stop, jobID: jobID, sentAt: now),
            now: now,
            animated: true
        )
        bridge.send(.stop, jobID: jobID, style: viewModel.style, sentAt: now)
    }

    private func cancelWarmDictation() {
        let now = Date().timeIntervalSince1970
        guard let jobID = viewModel.currentSnapshot.jobID ?? viewModel.pendingCommand?.jobID else { return }
        viewModel.setPendingCommand(
            KeyboardViewModel.PendingCommand(kind: .cancel, jobID: jobID, sentAt: now),
            now: now,
            animated: true
        )
        bridge.send(.cancel, jobID: jobID, style: viewModel.style, sentAt: now)
    }
}
