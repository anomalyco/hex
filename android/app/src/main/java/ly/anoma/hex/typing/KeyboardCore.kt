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
}

/** A swipe's candidates, best first, and how many words passed the key filters and were scored. */
data class SwipeReading(val texts: Array<String>, val costs: DoubleArray, val scored: Int)
