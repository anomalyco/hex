package ly.anoma.hex.speech

/** Faster English dictation (Labs): NVIDIA's Parakeet TDT-CTC 110M through transcribe.cpp, in libhexenglish.so, which
 * is loaded the first time this is used, so never while that switch is off. */
internal object Parakeet {
    init { System.loadLibrary("hexenglish") }
    /** A session, or 0 when it failed ([outOfMemory] says whether memory was why). */
    external fun load(file: String, threads: Int): Long
    external fun outOfMemory(): Boolean
    external fun free(handle: Long)
    /** UTF-8: the text, then one "start\tend\tword" line per word (ms), or null when it failed or [abort] stopped it.
     * [vocabulary] is the person's own words, in order. */
    external fun decode(handle: Long, pcm: ShortArray, count: Int, vocabulary: Array<String>?, job: Long): ByteArray?
    external fun abort(job: Long)
}
