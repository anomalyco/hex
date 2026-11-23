import UIKit

/// The keyboard a typing config is reviewed for in the app: Hex's letters keyboard on this
/// iPhone held upright, in the language it opens in, with Settings › Keyboard's choices, as
/// `FullKeyboardView` picks them.
@MainActor
enum TypingConfigKeyboard {
    static func current(screen: CGSize = UIScreen.main.bounds.size) -> KeyboardLayout {
        let settings = LanguageSettings()
        let keyboard = FullKeyboardSettings()
        let list = KeyboardLanguageList.resolve(
            chosen: settings.keyboardLanguages,
            stored: settings.currentKeyboardLanguage,
            automatic: KeyboardLanguage.automatic(
                dictationLanguage: settings.effectiveDictationLanguage,
                inputModes: UITextInputMode.activeInputModes.compactMap(\.primaryLanguage),
                preferredLanguages: Locale.preferredLanguages
            )
        )
        return KeyboardLayout(
            width: min(screen.width, screen.height),
            orientation: .portrait,
            language: list.layout,
            letters: settings.letterLayout(for: list.layout),
            hexPlacement: keyboard.hexKeyPlacement,
            phoneSize: KeyboardPhoneSize.current(screenSize: screen),
            numberRow: keyboard.numberRow,
            dvorakPunctuation: keyboard.dvorakPunctuation,
            commaPeriodKeys: keyboard.commaPeriodKeys
        )
    }
}
