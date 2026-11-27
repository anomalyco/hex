import Foundation

/// The typing model's own spellings (`HEXKeyboard/TypingModel/<lang>.spellings`, written by
/// `scripts/build-typing-model.py` and byte-identical to Android's): words people write with
/// capitals the model's lowercase word list can't hold ("chatgpt" -> "ChatGPT", "github" ->
/// "GitHub", "figma" -> "Figma"), whatever the phone's dictionary knows.
@MainActor
extension AutocorrectEngine {
    /// `word`, all lowercase, as the layout language's model spells it; nil when it has no
    /// spelling of its own.
    func modelSpelling(of word: String) -> String? {
        guard word == word.lowercased(), let model = contextModel ?? decoder?.model,
              model.language == language.rawValue else { return nil }
        return HexWordList.bundled(model.language, "spellings")?.text(of: word)
    }

    /// A model word (lowercase) as the strip offers it before it is cased like what you typed:
    /// its own spelling, or the capital of a name the dictionary knows only capitalised ("mary"
    /// -> "Mary"), as space and Swipe typing put them in.
    func stripSpelling(of word: String) -> String {
        if let spelled = modelSpelling(of: word) { return spelled }
        guard word.count >= 2, word == word.lowercased(), let first = word.first else { return word }
        let capitalised = first.uppercased() + word.dropFirst()
        return checker.isWordValid(capitalised) && !checker.isWordValid(word) ? capitalised : word
    }
}
