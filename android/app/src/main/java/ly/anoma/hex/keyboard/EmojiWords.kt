package ly.anoma.hex.keyboard

import android.content.res.AssetManager
import android.graphics.Paint
import java.io.IOException
import java.util.concurrent.Executor
import ly.anoma.hex.typing.EmojiKeywords

/** The bundled English emoji words, read once and lazily, off the typing path. The keyboard holds
 * one; it opens the asset only after Labs › Keyboard › Emoji suggestions is on, and a failed read
 * behaves as no matches forever instead of retrying on every key. */
internal class EmojiWords(private val assets: AssetManager, private val background: Executor) {
    @Volatile private var keywords: EmojiKeywords? = null
    /** Whether a read was started; a failed read stays requested and is never retried. */
    @Volatile var requested = false
        private set

    /** [word]'s emoji for a typing model in [language] when the list is in memory, else null. */
    fun match(word: String, language: String): String? = keywords?.match(word, language)

    /** Reads the asset on [background], then calls [ready] there. Only the first call reads it,
     * whether that read succeeds or fails. */
    fun load(ready: () -> Unit) {
        synchronized(this) {
            if (requested) return
            requested = true
        }
        background.execute {
            keywords = try {
                // Only what this platform can draw: minSdk 31 stops at Emoji 13.1, so a newer
                // asset entry would otherwise be offered as tofu in the plain TextView strip.
                val paint = Paint()
                assets.open(EmojiKeywords.ASSET).bufferedReader(Charsets.UTF_8).use {
                    EmojiKeywords.parse(it.lineSequence()) { emoji -> paint.hasGlyph(emoji) }
                }
            } catch (_: IOException) {
                null
            }
            ready()
        }
    }
}
