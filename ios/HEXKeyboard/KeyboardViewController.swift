import Combine
import SwiftUI
import UIKit

@MainActor
private final class PrimaryActionModel: ObservableObject {
    @Published var title = "Open HEX"
    @Published var image = "arrow.up.forward.app.fill"
    @Published var enabled = true
    @Published var recording = false
    @Published var destination: URL?
    var action: (() -> Void)?
}

private struct PrimaryActionView: View {
    @ObservedObject var model: PrimaryActionModel

    var body: some View {
        Group {
            if let destination = model.destination {
                Link(destination: destination) {
                    label
                }
            } else {
                Button {
                    model.action?()
                } label: {
                    label
                }
                .disabled(!model.enabled)
            }
        }
        .buttonStyle(.plain)
    }

    private var label: some View {
        HStack(spacing: 7) {
            if !model.image.isEmpty {
                Image(systemName: model.image)
            }
            Text(model.title)
                .fontWeight(.semibold)
        }
        .foregroundStyle(model.recording ? Color.white : Color(uiColor: .label))
        .frame(maxWidth: .infinity, minHeight: 44)
        .background(
            model.recording ? Color(uiColor: .systemRed) : Color(uiColor: .secondarySystemFill),
            in: Capsule()
        )
        .opacity(model.enabled ? 1 : 0.5)
        .contentShape(Capsule())
    }
}

@MainActor
final class KeyboardViewController: UIInputViewController {
    private let bridge = KeyboardBridge()
    private let statusLabel = UILabel()
    private let detailLabel = UILabel()
    private let primaryActionModel = PrimaryActionModel()
    private let globeButton = UIButton(type: .system)
    private let spaceButton = UIButton(type: .system)
    private let deleteButton = UIButton(type: .system)
    private let returnButton = UIButton(type: .system)

    private var pollTimer: Timer?
    private var snapshot = KeyboardSnapshot.offline
    private var requestedJobID: String?

    private let lastInsertedResultKey = "keyboard.last-inserted-result"

    override func viewDidLoad() {
        super.viewDidLoad()
        configureView()
        poll()
    }

    override func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        pollTimer?.invalidate()
        pollTimer = Timer.scheduledTimer(withTimeInterval: 0.15, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.poll()
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
        updateAppearance()
    }

    private func configureView() {
        view.backgroundColor = .clear
        view.isOpaque = false
        preferredContentSize.height = 176

        statusLabel.font = .systemFont(ofSize: 15, weight: .semibold)
        statusLabel.textColor = .label
        statusLabel.textAlignment = .center

        detailLabel.font = .systemFont(ofSize: 12, weight: .regular)
        detailLabel.textColor = .secondaryLabel
        detailLabel.textAlignment = .center
        detailLabel.adjustsFontSizeToFitWidth = true
        detailLabel.minimumScaleFactor = 0.82

        primaryActionModel.action = { [weak self] in
            self?.performPrimaryAction()
        }
        let primaryActionController = UIHostingController(
            rootView: PrimaryActionView(model: primaryActionModel)
        )
        primaryActionController.view.backgroundColor = .clear
        addChild(primaryActionController)
        primaryActionController.didMove(toParent: self)
        primaryActionController.view.heightAnchor.constraint(equalToConstant: 44).isActive = true

        configureKey(globeButton, title: "", image: "globe")
        globeButton.addTarget(self, action: #selector(handleInputModeList(from:with:)), for: .allTouchEvents)

        configureKey(spaceButton, title: "space", image: nil)
        spaceButton.addTarget(self, action: #selector(insertSpace), for: .touchUpInside)

        configureKey(deleteButton, title: "", image: "delete.left")
        deleteButton.addTarget(self, action: #selector(deleteBackward), for: .touchUpInside)

        configureKey(returnButton, title: "", image: "return")
        returnButton.addTarget(self, action: #selector(insertReturn), for: .touchUpInside)

        let statusStack = UIStackView(arrangedSubviews: [statusLabel, detailLabel])
        statusStack.axis = .vertical
        statusStack.spacing = 2

        let utilityStack = UIStackView(arrangedSubviews: [globeButton, spaceButton, deleteButton, returnButton])
        utilityStack.axis = .horizontal
        utilityStack.spacing = 6
        utilityStack.distribution = .fill
        globeButton.widthAnchor.constraint(equalToConstant: 44).isActive = true
        deleteButton.widthAnchor.constraint(equalToConstant: 50).isActive = true
        returnButton.widthAnchor.constraint(equalToConstant: 50).isActive = true

        let stack = UIStackView(arrangedSubviews: [statusStack, primaryActionController.view, utilityStack])
        stack.translatesAutoresizingMaskIntoConstraints = false
        stack.axis = .vertical
        stack.spacing = 8
        view.addSubview(stack)

        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 10),
            stack.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -10),
            stack.topAnchor.constraint(equalTo: view.topAnchor, constant: 8),
            stack.bottomAnchor.constraint(equalTo: view.safeAreaLayoutGuide.bottomAnchor, constant: -6),
        ])
    }

    private func configureKey(_ button: UIButton, title: String, image: String?) {
        var configuration = UIButton.Configuration.filled()
        configuration.title = title
        if let image {
            configuration.image = UIImage(systemName: image)
        }
        configuration.cornerStyle = .medium
        configuration.baseBackgroundColor = .secondarySystemFill
        configuration.baseForegroundColor = .label
        configuration.contentInsets = NSDirectionalEdgeInsets(top: 9, leading: 12, bottom: 9, trailing: 12)
        button.configuration = configuration
    }

    private func poll() {
        snapshot = bridge.snapshot()

        if let resultID = snapshot.resultID,
           let transcript = snapshot.transcript,
           !transcript.isEmpty,
           UserDefaults.standard.string(forKey: lastInsertedResultKey) != resultID {
            textDocumentProxy.insertText(transcript)
            UserDefaults.standard.set(resultID, forKey: lastInsertedResultKey)
            requestedJobID = nil
        }

        updateAppearance()
    }

    private func updateAppearance() {
        globeButton.isHidden = !needsInputModeSwitchKey

        guard hasFullAccess else {
            statusLabel.text = "Full Access required"
            detailLabel.text = "Open HEX to finish keyboard setup"
            setPrimaryButton(
                title: "Open HEX",
                image: "arrow.up.forward.app.fill",
                enabled: true,
                launchRequest: .startSession
            )
            return
        }

        guard snapshot.isAvailable else {
            statusLabel.text = "HEX is offline"
            if bridge.isModelInstalled {
                detailLabel.text = "Open HEX to connect"
            } else {
                detailLabel.text = "Open HEX to prepare on-device speech"
            }
            let jobID = requestedJobID ?? UUID().uuidString
            requestedJobID = jobID
            setPrimaryButton(
                title: "Open HEX",
                image: "arrow.up.forward.app.fill",
                enabled: true,
                launchRequest: .startRecording(jobID: jobID)
            )
            return
        }

        switch snapshot.state {
        case .offline:
            statusLabel.text = "Connecting to HEX"
            detailLabel.text = "The app is preparing your keyboard session"
            setPrimaryButton(title: "Connecting...", image: nil, enabled: false)
        case .ready:
            statusLabel.text = snapshot.message ?? "Ready"
            detailLabel.text = "Tap to dictate into this field"
            setPrimaryButton(title: "Record", image: "mic.fill", enabled: true)
        case .recording:
            statusLabel.text = "Listening"
            detailLabel.text = "Tap when you are finished"
            setPrimaryButton(title: "Stop and insert", image: "stop.fill", enabled: true, recording: true)
        case .transcribing:
            statusLabel.text = "Transcribing"
            detailLabel.text = "Running on-device speech recognition"
            setPrimaryButton(title: "Working...", image: nil, enabled: false)
        case .failed:
            statusLabel.text = "HEX needs attention"
            detailLabel.text = snapshot.message ?? "Open the app to recover"
            setPrimaryButton(
                title: "Open HEX",
                image: "arrow.up.forward.app.fill",
                enabled: true,
                launchRequest: .startSession
            )
        }
    }

    private func setPrimaryButton(
        title: String,
        image: String?,
        enabled: Bool,
        recording: Bool = false,
        launchRequest: KeyboardLaunchRequest? = nil
    ) {
        primaryActionModel.title = title
        primaryActionModel.image = image ?? ""
        primaryActionModel.enabled = enabled
        primaryActionModel.recording = recording
        primaryActionModel.destination = launchRequest?.url
    }

    @objc private func performPrimaryAction() {
        guard hasFullAccess, snapshot.isAvailable else {
            return
        }

        switch snapshot.state {
        case .ready:
            let jobID = UUID().uuidString
            requestedJobID = jobID
            bridge.send(.start, jobID: jobID)
            statusLabel.text = "Starting"
            setPrimaryButton(title: "Starting...", image: "mic.fill", enabled: false)
        case .recording:
            guard let jobID = snapshot.jobID ?? requestedJobID else { return }
            bridge.send(.stop, jobID: jobID)
            setPrimaryButton(title: "Finishing...", image: "stop.fill", enabled: false)
        case .failed:
            break
        case .offline, .transcribing:
            break
        }
    }

    @objc private func insertSpace() {
        textDocumentProxy.insertText(" ")
    }

    @objc private func deleteBackward() {
        textDocumentProxy.deleteBackward()
    }

    @objc private func insertReturn() {
        textDocumentProxy.insertText("\n")
    }
}
