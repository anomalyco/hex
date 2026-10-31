import SwiftUI
import WidgetKit

@main
struct HEXWidgetsBundle: WidgetBundle {
    var body: some Widget {
        HexDictationLiveActivity()
        if #available(iOS 18.0, *) {
            HexDictationControl()
        }
    }
}
