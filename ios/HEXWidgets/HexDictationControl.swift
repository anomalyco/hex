import AppIntents
import SwiftUI
import WidgetKit

@available(iOS 18.0, *)
struct HexDictationControl: ControlWidget {
    static let kind = "com.kitlangton.hex.mobile.control.dictate"

    var body: some ControlWidgetConfiguration {
        StaticControlConfiguration(kind: Self.kind) {
            ControlWidgetButton(action: ToggleHexControlIntent()) {
                Label("Dictate", systemImage: "mic.fill")
                Text("Hex")
            }
        }
        .displayName("Dictate with Hex")
        .description("Starts or finishes on-device dictation and copies the text.")
    }
}
