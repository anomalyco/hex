package ly.anoma.hex.typing

/**
 * The keyboard's Rust core (`crates/keyboard`, `libhex_keyboard.so`), in debug builds only
 * while it's proven against the Kotlin it will replace; nothing calls it yet. Handles are
 * `Long`s, 0 for none; each `*Open*`/`*New` is released by its `*Free`. Any thread may use
 * an open handle.
 */
object KeyboardCore {
    init {
        System.loadLibrary("hex_keyboard")
    }

    /** `HK_ABI_VERSION`. */
    external fun abiVersion(): Int

    /** Maps an HXTM v3 file read-only; 0 when it can't. */
    external fun modelOpen(path: String): Long

    /** Maps `length` bytes at `offset` of an open file (an uncompressed APK asset); `fd` may be closed afterwards. */
    external fun modelOpenFd(fd: Int, offset: Long, length: Long): Long

    external fun modelFree(model: Long)

    external fun modelWordCount(model: Long): Long

    /**
     * A swipe decoder with the default parameters, or 0. `neverWords` are model word indices
     * never read, `neverPairs` (first, second) index pairs interleaved; the extra words run in
     * step, a repeat left out. It keeps its own reference to `model`.
     */
    external fun swipeNew(
        model: Long,
        neverWords: IntArray,
        neverPairs: IntArray,
        extraTexts: Array<String>,
        extraCosts: DoubleArray,
        extraContractions: BooleanArray,
    ): Long

    external fun swipeFree(decoder: Long)

    /**
     * The likeliest words for a swipe, best first, or null when the arguments don't fit.
     * `points` interleaves (x, y) in the keys' coordinates; `times` is null or one per point,
     * in seconds; the key arrays run in step, each letter one Unicode code point.
     */
    external fun swipeRead(
        decoder: Long,
        points: DoubleArray,
        times: DoubleArray?,
        keyLetters: IntArray,
        keyXs: DoubleArray,
        keyYs: DoubleArray,
        pitchWidth: Double,
        pitchHeight: Double,
        previous: String?,
        limit: Int,
    ): SwipeReading?

    /**
     * Autocorrect's spelling dictionary, which stays the host's: called back on the calling
     * thread during the call that needs it, and never calls back into the handle.
     */
    interface TextChecker {
        fun isWordValid(word: String): Boolean
        fun guesses(word: String): Array<String>
        fun completions(prefix: String): Array<String>
        fun useLanguage(code: String)
    }

    /**
     * An autocorrect in English reading `neverSuggest` (the never-suggest list's text) with
     * `checker` as its dictionary, or 0. A handle is used from one thread at a time.
     */
    external fun autocorrectNew(neverSuggest: String, checker: TextChecker): Long

    external fun autocorrectFree(autocorrect: Long)

    external fun autocorrectSetSettings(
        autocorrect: Long,
        enabled: Boolean,
        allowsSwearing: Boolean,
        addsCapitals: Boolean,
        learnsPerLanguage: Boolean,
    )

    /** Follows the keyboard layout: `tag` the language's BCP 47 tag, `available` the device's dictionaries. */
    external fun autocorrectSetLanguage(autocorrect: Long, tag: String, available: Array<String>): Boolean

    /** Bilingual autocorrect's second language (`tag` null: none) with its own dictionary. */
    external fun autocorrectSetPartner(autocorrect: Long, tag: String?, available: Array<String>, checker: TextChecker?): Boolean

    /** The keys typed on: the layout's language tag and its letter grid, in step (code point, column, row). */
    external fun autocorrectSetKeys(autocorrect: Long, layout: String, letters: IntArray, columns: DoubleArray, rows: DoubleArray): Boolean

    /** Your text replacements and terms: typing `inputs[i]` gives `outputs[i]`. */
    external fun autocorrectLoadLexicon(autocorrect: Long, inputs: Array<String>, outputs: Array<String>): Boolean

    /** The vocabulary (`which` 0), or the kept words of this language's list (1) or the partner's (2). */
    external fun autocorrectSetWords(autocorrect: Long, which: Int, words: Array<String>): Boolean

    /** What Learn how I type learned; the partner's arrays null to give it the same. */
    external fun autocorrectApplyLearning(
        autocorrect: Long,
        typos: Array<String>,
        fixes: Array<String>,
        rejected: Array<String>,
        partnerTypos: Array<String>?,
        partnerFixes: Array<String>?,
        partnerRejected: Array<String>?,
    ): Boolean

    external fun autocorrectSuppressAndKeep(autocorrect: Long, word: String)

    /** Spatial autocorrect's model (`which` 0), the partner's (1) or the context model (2); 0 for none. */
    external fun autocorrectSetModel(autocorrect: Long, which: Int, model: Long): Boolean

    /** Where each letter of the word being typed was tapped, (x, y) interleaved; null when unknown. */
    external fun autocorrectSetTaps(autocorrect: Long, taps: DoubleArray?)

    /** The strip for the text before the cursor; `field` 0 text, 1 URL, 2 email; `predictionModel` 0 for none. */
    external fun autocorrectSuggestions(
        autocorrect: Long,
        before: String?,
        field: Int,
        autocorrectionOff: Boolean,
        predictionModel: Long,
        capitalizesNext: Boolean,
    ): Array<AutocorrectSuggestion>?

    /** What space puts in: [what was typed, its replacement], or null to keep it. */
    external fun autocorrectReplacementOnSpace(autocorrect: Long, before: String?, field: Int, autocorrectionOff: Boolean): Array<String>?

    /** The word before the one being finished revised now that `next` follows it, or null. */
    external fun autocorrectRevision(autocorrect: Long, word: String, before: String, next: String, field: Int, autocorrectionOff: Boolean): String?

    /** One word for `first` (its taps interleaved), a space tapped as a letter and `second`, or null. */
    external fun autocorrectMerge(autocorrect: Long, first: String, firstTaps: DoubleArray, spaceX: Double, spaceY: Double, second: String, before: String): String?

    /** Your text replacement whose shortcut ends `before`: [the shortcut, what it gives], or null. */
    external fun autocorrectTextReplacement(autocorrect: Long, before: String?): Array<String>?

    external fun autocorrectMayOffer(autocorrect: Long, word: String, previous: String?): Boolean

    /** The language of the fix space last decided on, or null. */
    external fun autocorrectLastFixLanguage(autocorrect: Long): String?

    /** The word right before the caret, or null inside URLs, handles, paths or numbers. */
    external fun currentToken(before: String?): String?

    /** Whether the core has the language manifest for `tag`. */
    external fun languageKnown(tag: String): Boolean
}

/** One slot of the suggestion strip: `kind` 0 what was typed, 1 the best word, 2 the one space puts in, 3 the runner-up, 4 a next word. */
data class AutocorrectSuggestion(val displayText: String, val replacement: String, val kind: Int)

/** A swipe's candidates, best first, and how many words passed the key filters and were scored. */
data class SwipeReading(val texts: Array<String>, val costs: DoubleArray, val scored: Int)
