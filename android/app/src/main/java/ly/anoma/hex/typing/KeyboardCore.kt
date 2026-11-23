package ly.anoma.hex.typing

/**
 * The keyboard's Rust core (`crates/keyboard`, `libhex_keyboard.so`). Handles are `Long`s, 0 for
 * none; each `*Open*`/`*New` is released by its `*Free`. Any thread may use an open handle.
 * `proguard-rules.pro` keeps its names and what the native code makes.
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
     * Your text replacements (`shortcuts` lowercased, in step with `replacements`) and words
     * (`own`: other own spellings; `vocabulary`: your vocabulary's words), for the layout's
     * `language`; 0 when malformed. Immutable: make a new one when any of them changes.
     */
    external fun swipeWordsNew(
        language: String,
        hasDictionary: Boolean,
        shortcuts: Array<String>,
        replacements: Array<String>,
        own: Array<String>,
        vocabulary: Array<String>,
    ): Long

    external fun swipeWordsFree(words: Long)

    /** The order of a reading's candidates once rare names cost more; null when malformed. */
    external fun swipeRank(decoder: Long, words: Long, texts: Array<String>, costs: DoubleArray, checker: WordChecker): IntArray?

    /**
     * What a swipe with these `readings` (best first, those that may be offered; at least one)
     * types. `shift`: 0 off, 1 on, 2 caps lock. `autoCapitalization`: 0 none, 1 words,
     * 2 sentences, 3 all characters (0 also with Auto-capitalization off). Null when malformed.
     */
    external fun swipeTyped(
        words: Long,
        readings: Array<String>,
        before: String?,
        shift: Int,
        shiftHeld: Boolean,
        autoCapitalization: Int,
        autoCapitalizes: Boolean,
        autocorrection: Boolean,
        checker: WordChecker,
    ): SwipeTyped?

    external fun swipeNeedsSpace(before: String?): Boolean

    /** The cost of English contraction `word` (any case), as `hk_swipe_cost_of_contraction` with the core's base. */
    external fun swipeCostOfContraction(word: String): Double

    /** The cost of one of your own words. */
    external fun swipeOwnWordCost(): Double

    /** The English contractions Swipe typing reads as common words, in iOS's order. */
    external fun swipeEnglishContractions(): Array<String>

    /** `points` interleaves (x, y); `key` is the character of the key it went down and lifted on. */
    external fun swipeIsSlidTap(points: DoubleArray, pitch: Double, key: String?, firstReading: String?): Boolean
}

/** A swipe's candidates, best first, and how many words passed the key filters and were scored. */
data class SwipeReading(val texts: Array<String>, val costs: DoubleArray, val scored: Int)

/** Whether the dictionary knows a word; the core asks only while a call runs. */
fun interface WordChecker {
    fun isWordValid(word: String): Boolean
}

/** What a finished swipe types: a space first or not, the word, the strip's others. */
data class SwipeTyped(val space: Boolean, val word: String, val alternatives: Array<String>)
