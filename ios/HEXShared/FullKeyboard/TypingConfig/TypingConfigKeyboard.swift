import Foundation

/// A keyboard a typing config may be for: its context (`TypingConfigFile.Context`) and the
/// geometry zones compile against. The keyboard records each one it runs on
/// (`TypingConfigSettings.keyboards`), so the app reviews a config against keyboards it can't draw.
nonisolated struct TypingConfigKeyboard: Codable, Equatable, Sendable {
    var context: TypingConfigFile.Context
    var geometry: KeyboardZoneGeometry

    init(context: TypingConfigFile.Context, geometry: KeyboardZoneGeometry) {
        self.context = context
        self.geometry = geometry
    }

    init(layout: KeyboardLayout) {
        self.init(context: TypingConfigFile.Context(layout: layout), geometry: KeyboardZoneGeometry(layout: layout))
    }
}

nonisolated extension TypingConfigSettings {
    /// The keyboards Hex's keys have run on with the experiment on, most recent first, at most
    /// `TypingConfigLimits.maxContexts`: what the review checks a config against.
    var keyboards: [TypingConfigKeyboard] {
        guard let data = defaults?.data(forKey: Keys.keyboards) else { return [] }
        return (try? JSONDecoder().decode([TypingConfigKeyboard].self, from: data)) ?? []
    }

    /// Puts `keyboard` first; writes only when the list changes.
    func record(_ keyboard: TypingConfigKeyboard) {
        let now = keyboards
        guard now.first != keyboard else { return }
        let next = Array(([keyboard] + now.filter { $0.context != keyboard.context }).prefix(TypingConfigLimits.maxContexts))
        if let data = try? JSONEncoder().encode(next) { defaults?.set(data, forKey: Keys.keyboards) }
    }
}
