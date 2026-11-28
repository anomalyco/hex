package ly.anoma.hex.keyboard

import android.content.res.AssetManager
import android.graphics.Paint
import java.io.IOException
import java.util.concurrent.Executor
import ly.anoma.hex.typing.EmojiShortcode
import ly.anoma.hex.typing.EmojiShortcodeIndex

/** The bundled emoji shortcode index (Labs › Keyboard › Emoji shortcodes), read once and lazily, off
 * the typing path. The keyboard holds one; it opens the asset only once a ":name" is typed with the
 * lab on, and a failed read behaves as no matches forever instead of retrying on every key. */
internal class EmojiShortcodes(private val assets: AssetManager, private val background: Executor) {
    @Volatile private var index: EmojiShortcodeIndex? = null
    /** Whether a read was started; a failed read stays requested and is never retried. */
    @Volatile var requested = false
        private set

    /** The strip's emoji for [token] when the index is in memory, else none. */
    fun matches(token: EmojiShortcode.Token): List<String> = index?.let { EmojiShortcode.matches(token, it) }.orEmpty()

    /** The emoji [token]'s name is exactly, for its closing ":", when the index is in memory. */
    fun completion(token: EmojiShortcode.Token): String? = index?.let { EmojiShortcode.completion(token, it) }

    /** Reads the asset on [background], then calls [ready] there. Only the first call reads it,
     * whether that read succeeds or fails. */
    fun load(ready: () -> Unit) {
        synchronized(this) {
            if (requested) return
            requested = true
        }
        background.execute {
            index = try {
                // Only what this phone's emoji font draws, so the plain TextView strip never shows tofu.
                val paint = Paint()
                assets.open(EmojiShortcodeIndex.ASSET).bufferedReader(Charsets.UTF_8).use {
                    EmojiShortcodeIndex.parse(it.lineSequence()) { emoji -> paint.hasGlyph(emoji) }
                }
            } catch (_: IOException) {
                null
            }
            ready()
        }
    }
}
