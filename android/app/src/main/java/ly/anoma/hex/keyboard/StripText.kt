package ly.anoma.hex.keyboard

/** How the strip's words are sized: [SIZE] when they fit, shrinking towards [MIN] so a long
 * candidate reads in full before the strip ellipsizes it. */
internal object StripText {
    const val SIZE = 16f
    const val MIN = 12f
}

/** [size] when a word [width] wide at a given size fits [available], else the largest size down to
 * [min] that does. [width] measures a word at a size, so the result is the largest that fits. */
internal fun fitTextSize(available: Float, size: Float, min: Float, width: (Float) -> Float): Float {
    if (available <= 0f) return min
    if (width(size) <= available) return size
    var fits = min
    var over = size
    repeat(12) {
        val mid = (fits + over) / 2f
        if (width(mid) <= available) fits = mid else over = mid
    }
    return fits
}
