import CoreGraphics

/// The keyboard a typing config's key block is for, read from a live layout, and how two
/// compare (`reference/typing-config.md`, "Context").
nonisolated extension TypingConfigFile.Context {
    init(layout: KeyboardLayout) {
        let m = layout.metrics
        self.init(
            width: Double(layout.width),
            orientation: layout.orientation == .portrait ? "portrait" : "landscape",
            posture: Self.name(layout.posture),
            language: layout.language.rawValue,
            letters: layout.letters.rawValue,
            split: layout.isSplit,
            hexPlacement: layout.hexPlacement.rawValue,
            numberRow: layout.hasNumberRow,
            keyHeight: Double(m.keyHeight),
            rowPitch: Double(m.keyHeight + m.rowGap),
            letterWidth: Double(layout.letterWidth),
            hitModel: KeyboardZone.hitModel
        )
    }

    static func name(_ posture: KeyboardPosture) -> String {
        switch posture {
        case .phone: "phone"
        case .cover: "cover"
        case .unfoldedWide: "unfoldedWide"
        case .unfoldedTall: "unfoldedTall"
        }
    }

    /// The fields that keep this block off `live`, in the order the context lists them; empty
    /// when it applies there. `hitModel` is `TypingConfigReview.ContextCheck.hitModelMatches`.
    func differences(from live: Self) -> [String] {
        let round = Double(TypingConfigLimits.contextWidthRound)
        let tolerance = Double(TypingConfigLimits.contextGeometryTolerance)
        func near(_ a: Double, _ b: Double) -> Bool { b > 0 && abs(a - b) <= tolerance * b }
        var out: [String] = []
        if (width / round).rounded() != (live.width / round).rounded() { out.append("width") }
        if orientation != live.orientation { out.append("orientation") }
        if posture != live.posture { out.append("posture") }
        if language != live.language { out.append("language") }
        if letters != live.letters { out.append("letters") }
        if split != live.split { out.append("split") }
        if hexPlacement != live.hexPlacement { out.append("hexPlacement") }
        if numberRow != live.numberRow { out.append("numberRow") }
        if !near(keyHeight, live.keyHeight) { out.append("keyHeight") }
        if !near(rowPitch, live.rowPitch) { out.append("rowPitch") }
        if !near(letterWidth, live.letterWidth) { out.append("letterWidth") }
        return out
    }
}
