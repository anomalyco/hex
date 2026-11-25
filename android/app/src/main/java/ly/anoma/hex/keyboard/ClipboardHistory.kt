package ly.anoma.hex.keyboard

import android.content.ClipData
import android.content.ClipDescription
import android.content.Context
import android.os.PersistableBundle
import android.util.AtomicFile
import java.io.File
import org.json.JSONArray
import org.json.JSONObject

/** A text Labs › Keyboard › Clipboard history keeps: what was copied, when Android says it was, and whether it is pinned. */
internal data class CopiedText(val text: String, val copiedAt: Long, val pinned: Boolean = false)

/** Clipboard history's rules, without Android: what is kept, in which order and for how long. */
internal object ClipboardRules {
    /** Unpinned texts kept, newest first; pinned ones have their own limit. */
    const val RECENT = 20
    const val PINNED = 20
    /** An unpinned text goes an hour after it was copied, as Android 13's own clipboard and Gboard's history. */
    const val LIFETIME_MS = 60 * 60 * 1000L
    const val MAX_LENGTH = 10_000
    /** ClipDescription.EXTRA_IS_SENSITIVE from Android 13; password managers set the same key below it. */
    const val SENSITIVE = "android.content.extra.IS_SENSITIVE"
    /** On Hex's own copies (Home's Copy, the Quick Settings tile, Diagnostics), so a dictation never joins the history. */
    const val OWN = "ly.anoma.hex.extra.OWN_COPY"

    /** Whether a copy is kept: text only, never one marked sensitive or Hex's own, none copied while a password field
     * or a field that asks not to be learned from is open, and nothing blank or very long. */
    fun keeps(text: String?, isText: Boolean, sensitive: Boolean, own: Boolean, secureField: Boolean, noLearning: Boolean) =
        isText && !sensitive && !own && !secureField && !noLearning &&
            text != null && text.isNotBlank() && text.length <= MAX_LENGTH

    /** A clip's description marks it sensitive: the Android 13 constant and the literal key password managers set below
     * it are the same key. [extra] reads one boolean of the description's extras. */
    fun sensitive(extra: (String) -> Boolean) = extra(SENSITIVE)
    /** A clip is one of Hex's own copies ([hexClip]). */
    fun own(extra: (String) -> Boolean) = extra(OWN)

    /** [text] copied at [at]: a text kept already moves to the top and keeps its pin. */
    fun add(items: List<CopiedText>, text: String, at: Long): List<CopiedText> {
        val pinned = items.any { it.text == text && it.pinned }
        return ordered(listOf(CopiedText(text, at, pinned)) + items.filter { it.text != text })
    }

    /** What is still kept at [now]: pinned first, then the newest. An unpinned text an hour old goes, and so does one
     * dated after [now] by a clock set back, rather than staying for longer than an hour. */
    fun current(items: List<CopiedText>, now: Long): List<CopiedText> =
        ordered(items.filter { it.pinned || now - it.copiedAt in 0 until LIFETIME_MS })

    private fun ordered(items: List<CopiedText>): List<CopiedText> {
        val (pinned, recent) = items.partition { it.pinned }
        return pinned.sortedByDescending { it.copiedAt }.take(PINNED) + recent.sortedByDescending { it.copiedAt }.take(RECENT)
    }

    fun pin(items: List<CopiedText>, text: String, on: Boolean) =
        ordered(items.map { if (it.text == text) it.copy(pinned = on) else it })
    fun delete(items: List<CopiedText>, text: String) = items.filter { it.text != text }
    /** Clear: every unpinned text goes, pinned ones stay. */
    fun clear(items: List<CopiedText>) = items.filter { it.pinned }

    /** How a kept text reads in the panel: its own lines, each trimmed, without the blank ones. */
    fun preview(text: String) = text.lines().map(String::trim).filter(String::isNotEmpty).joinToString("\n")
}

/** Whether Hex listens for copies at all: only while the lab is on. Off, the listener is never added and the
 * clipboard is never read, as before the lab existed; turning it off removes the listener and deletes every text. */
internal class ClipboardKeeper(private val enabled: () -> Boolean, private val listen: (Boolean) -> Unit,
                               private val read: () -> Unit, private val forget: () -> Unit) {
    var listening = false
        private set
    /** The lab's setting, as Hex starts or as it changes. */
    fun sync() {
        val on = enabled()
        if (on == listening) return
        listening = on
        listen(on)
        if (!on) forget()
    }
    /** Android announced a copy, or the keyboard opened on one it may have missed. */
    fun copied() { if (listening && enabled()) read() }
    /** The keyboard is going away: the listener goes, what was kept stays. */
    fun stop() { if (listening) { listening = false; listen(false) } }
}

/** A copy Hex makes itself (a dictation, a diagnostics report), marked so Clipboard history never keeps it. */
internal fun hexClip(label: CharSequence, text: CharSequence): ClipData = ClipData.newPlainText(label, text).apply {
    description.extras = PersistableBundle().apply { putBoolean(ClipboardRules.OWN, true) }
}

/** The kept texts on this phone: one small file in noBackupFilesDir, which no backup and no transfer to a new phone
 * copies, read once per process and written on each change. Nothing leaves the phone. */
internal object ClipboardHistory {
    private const val FILE = "clipboard-history.json"
    private var loaded: List<CopiedText>? = null
    /** The newest copy already looked at: reopening the keyboard never brings back a deleted text, and turning the lab
     * on keeps only what is copied after. */
    private var seen = 0L

    private fun file(context: Context) = AtomicFile(File(context.applicationContext.noBackupFilesDir, FILE))

    @Synchronized fun items(context: Context, now: Long = System.currentTimeMillis()): List<CopiedText> {
        val all = loaded ?: read(context)
        val current = ClipboardRules.current(all, now)
        val changed = loaded == null && current != all || loaded != null && current != loaded
        loaded = current
        if (changed) write(context)
        return current
    }

    /** The lab turned on: only copies made from now on are kept. */
    @Synchronized fun begin(context: Context) {
        items(context)
        seen = maxOf(seen, System.currentTimeMillis())
        write(context)
    }

    /** The primary clip changed, or the keyboard opened on one it may have missed. The clip itself is asked for only
     * once its description allows keeping it. */
    @Synchronized fun copied(context: Context, description: ClipDescription?, clip: () -> ClipData?,
                             secureField: Boolean, noLearning: Boolean) {
        description ?: return
        val at = description.timestamp.takeIf { it > 0 } ?: System.currentTimeMillis()
        val items = items(context)
        if (at <= seen) return
        seen = at
        val extras = description.extras
        val isText = description.hasMimeType("text/*")
        val sensitive = ClipboardRules.sensitive { extras?.getBoolean(it) == true }
        val own = ClipboardRules.own { extras?.getBoolean(it) == true }
        if (!ClipboardRules.keeps("-", isText, sensitive, own, secureField, noLearning)) { write(context); return }
        // The first item's own text only: coerceToText would open a content: address.
        val text = clip()?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.text?.toString()
        if (!ClipboardRules.keeps(text, isText, sensitive, own, secureField, noLearning)) { write(context); return }
        update(context, ClipboardRules.add(items, text!!, at))
    }

    @Synchronized fun pin(context: Context, text: String, on: Boolean) =
        update(context, ClipboardRules.pin(items(context), text, on))
    @Synchronized fun delete(context: Context, text: String) = update(context, ClipboardRules.delete(items(context), text))
    @Synchronized fun clearUnpinned(context: Context) = update(context, ClipboardRules.clear(items(context)))
    /** The lab turned off: every text goes, from memory and from the phone. */
    @Synchronized fun deleteAll(context: Context) {
        loaded = emptyList(); seen = 0L
        file(context).delete()
    }

    private fun update(context: Context, items: List<CopiedText>) {
        loaded = ClipboardRules.current(items, System.currentTimeMillis())
        write(context)
    }

    private fun read(context: Context): List<CopiedText> = runCatching {
        val json = JSONObject(String(file(context).readFully()))
        seen = json.optLong("seen")
        val list = json.getJSONArray("items")
        List(list.length()) { i -> list.getJSONObject(i).let { CopiedText(it.getString("text"), it.getLong("at"), it.optBoolean("pinned")) } }
    }.getOrDefault(emptyList())

    private fun write(context: Context) {
        val json = JSONObject().put("seen", seen).put("items", JSONArray().apply {
            loaded.orEmpty().forEach { put(JSONObject().put("text", it.text).put("at", it.copiedAt).put("pinned", it.pinned)) }
        })
        val f = file(context)
        val out = f.startWrite()
        runCatching { out.write(json.toString().toByteArray()); f.finishWrite(out) }.onFailure { f.failWrite(out) }
    }
}
