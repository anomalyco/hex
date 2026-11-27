package ly.anoma.hex.keyboard

import android.content.Context
import androidx.emoji2.emojipicker.RecentEmojiProvider

/** The emoji picker's recents where AndroidX's own provider keeps them (its preferences, newest first), held by
 * the keys so the one picker can be asked for them again each time the panel opens. */
internal class RecentEmoji(context: Context, private val learning: () -> Boolean = { true }) : RecentEmojiProvider {
    private val prefs = context.getSharedPreferences("androidx.emoji2.emojipicker.preferences", Context.MODE_PRIVATE)
    private val list = prefs.getString(KEY, null)?.split(",")?.filter { it.isNotEmpty() }?.toMutableList() ?: mutableListOf()
    // The picker reads them on a worker thread; a pick records on the main thread.
    override suspend fun getRecentEmojiList(): List<String> = recent()
    /** Newest first, for Labs › Keyboard › Emoji search. */
    fun recent(): List<String> = synchronized(list) { list.toList() }
    override fun recordSelection(emoji: String) {
        if (!learning()) return
        val joined = synchronized(list) { list.remove(emoji); list.add(0, emoji); list.joinToString(",") }
        prefs.edit().putString(KEY, joined).apply()
    }
    private companion object { const val KEY = "pref_key_recent_emoji" }
}
