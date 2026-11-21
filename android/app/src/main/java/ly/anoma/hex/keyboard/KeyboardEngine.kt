package ly.anoma.hex.keyboard

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.text.InputType
import android.view.KeyEvent
import android.view.inputmethod.EditorInfo
import ly.anoma.hex.speech.Spoken
import ly.anoma.hex.typing.KeyGrid
import ly.anoma.hex.typing.SpatialDecoder
import ly.anoma.hex.typing.TypingPreferences
import ly.anoma.hex.typing.TypingWord

/** What the engine shows: the keys' page and the strip's words. */
internal interface KeyboardSurface {
    fun showPage(page: KeyboardPage)
    /** [literal]: what was typed, offered as typed; [correction]: what space will put in its place. */
    fun suggestions(words: List<String>, literal: String? = null, correction: String? = null)
}

/** Keys in, text out: the current word, autocorrection and its undo, shift and the key pages.
 * It talks to the editor only through [KeyboardTextTarget] and to the views through [KeyboardSurface]. */
internal class KeyboardEngine(
    private val target: KeyboardTextTarget,
    private val feedback: TypingPreferences,
    private val layoutLanguage: () -> Spoken,
    private val loadDecoder: (String) -> SpatialDecoder,
    private val clock: () -> Long = { SystemClock.uptimeMillis() },
    private val main: Handler = Handler(Looper.getMainLooper()),
) {
    private companion object {
        /** The correction chip waits for this pause in typing: working it out costs the decoder a
         * search (2 ms median, 16 ms p95 per letter on the API 31 emulator), which shouldn't delay keys. */
        const val CHIP_PAUSE_MS = 100L
    }
    private var pendingChip: Runnable? = null
    var surface: KeyboardSurface? = null
    private val models = HashMap<String, SpatialDecoder>()
    private var word: TypingWord? = null
    private var editor: EditorInfo? = null
    private var ordinaryField = false
    private var prefix = ""
    private var expectedCaret = -1
    private data class Undo(val language: String, val prefix: String, val original: String, val replacement: String)
    private var undo: Undo? = null
    private var shifted = false
    private var locked = false
    private var symbols = false
    private var more = false
    private var lastShift = 0L
    private var selStart = -1
    private var selEnd = -1
    var secure = true
        private set

    fun startInput(attribute: EditorInfo) {
        editor = attribute
        val kind = attribute.inputType and InputType.TYPE_MASK_CLASS
        val variation = attribute.inputType and InputType.TYPE_MASK_VARIATION
        secure = when (kind) {
            InputType.TYPE_CLASS_TEXT -> variation in setOf(InputType.TYPE_TEXT_VARIATION_PASSWORD,
                InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD, InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD)
            InputType.TYPE_CLASS_NUMBER -> variation == InputType.TYPE_NUMBER_VARIATION_PASSWORD
            else -> false
        }
        ordinaryField = kind == InputType.TYPE_CLASS_TEXT && !secure &&
            attribute.imeOptions and EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING == 0 &&
            variation !in setOf(InputType.TYPE_TEXT_VARIATION_URI, InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS,
                InputType.TYPE_TEXT_VARIATION_WEB_EMAIL_ADDRESS, InputType.TYPE_TEXT_VARIATION_FILTER) &&
            attribute.inputType and (InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS or InputType.TYPE_TEXT_FLAG_AUTO_COMPLETE) == 0
        resetTyping(); word = null
        symbols = kind == InputType.TYPE_CLASS_NUMBER || kind == InputType.TYPE_CLASS_PHONE ||
            kind == InputType.TYPE_CLASS_DATETIME
        more = false; locked = false; lastShift = 0
        selStart = attribute.initialSelStart; selEnd = attribute.initialSelEnd
        refreshCaps()
    }

    fun updateSelection(oldSelStart: Int, oldSelEnd: Int, newSelStart: Int, newSelEnd: Int) {
        selStart = newSelStart; selEnd = newSelEnd
        if (oldSelStart != newSelStart || oldSelEnd != newSelEnd) {
            if (expectedCaret >= 0 && (newSelStart != expectedCaret || newSelEnd != expectedCaret)) {
                // Editors may coalesce selection notifications. Compare the actual surrounding text
                // before invalidating our own in-flight commit, not a stale intermediate offset.
                val suffix = undo?.let { it.prefix + it.replacement + " " } ?: word?.token?.takeIf { it.isNotEmpty() }?.let { prefix + it }
                if (newSelStart != newSelEnd || suffix == null || !endsWith(suffix) ||
                    oldSelStart == expectedCaret) { resetTyping(); surface?.suggestions(emptyList()) }
            }
            refreshCaps()
        }
    }

    /** Suggest swear words changed: the current word's suggestions follow at once. */
    fun swearWordsChanged() {
        word?.let {
            it.suggestSwearWords = feedback.suggestSwearWords
            showSuggestions(it)
        }
    }

    private fun refreshCaps() {
        val mask = (editor?.inputType ?: 0) and
            (InputType.TYPE_TEXT_FLAG_CAP_CHARACTERS or InputType.TYPE_TEXT_FLAG_CAP_WORDS or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES)
        val next = locked || (!secure && mask != 0 && target.capsMode(mask) != 0)
        if (shifted != next) { shifted = next; showPlane() }
    }
    private fun showPlane() {
        surface?.showPage(KeyboardPage(layoutLanguage(), symbols, more, shifted, locked, EnterKey.from(editor)))
    }
    fun shift() {
        if (symbols) { more = !more; showPlane(); return }
        val now = clock()
        when {
            locked -> { locked = false; shifted = false }
            lastShift != 0L && now - lastShift < 350 -> { locked = true; shifted = true }
            else -> shifted = !shifted
        }
        lastShift = now; showPlane()
    }
    fun mode() { resetTyping(); surface?.suggestions(emptyList()); symbols = !symbols; more = false; lastShift = 0; showPlane() }
    private fun resetTyping() {
        cancelChip(); word?.reset(); undo = null; expectedCaret = -1; prefix = ""
    }
    private fun cancelChip() { pendingChip?.let(main::removeCallbacks); pendingChip = null }
    fun configureTyping(keepWord: Boolean = false) {
        val language = layoutLanguage().layoutCode
        val active = word
        if (ordinaryField && keepWord && active != null && active.decoder.model.language == language && active.spatial == feedback.spatial &&
            active.token.isNotEmpty() && endsWith(prefix + active.token)) {
            showPlane(); showSuggestions(active); return
        }
        resetTyping()
        word = if (ordinaryField) {
            try {
                TypingWord(models.getOrPut(language) { loadDecoder(language) }, feedback.spatial)
                    .also { it.suggestSwearWords = feedback.suggestSwearWords }
            } catch (_: java.io.IOException) { null } // The editor can still type if its assets cannot be read.
        } else null
        showPlane()
        showSuggestions(word)
    }
    private fun showSuggestions(w: TypingWord?) {
        cancelChip()
        if (w != null && w.token.isNotEmpty()) {
            val choices = w.suggestions()
            surface?.suggestions(choices, literal = w.literalChip(choices))
            val token = w.token
            pendingChip = Runnable {
                pendingChip = null
                if (word === w && w.token == token) correction(w)?.let { surface?.suggestions(choices, literal = token, correction = it) }
            }.also { main.postDelayed(it, CHIP_PAUSE_MS) }
            return
        }
        val b = if (w != null && !symbols && selStart == selEnd) before() else null
        surface?.suggestions(if (w != null && b != null && TypingWord.boundary(b.lastOrNull()) && canEndWord()) w.predictions(b) else emptyList())
    }
    /** What space would put in place of the word: the strip shows it before it happens. */
    private fun correction(w: TypingWord): String? {
        if (!feedback.autocorrect || !canEndWord()) return null
        return w.replacement()?.takeIf { it != w.token && !feedback.blocked(w.decoder.model.language, w.token, it) }
    }
    private fun before() = if (secure) "" else target.before(96).orEmpty()
    private fun endsWith(suffix: String) = !secure && target.before(suffix.length) == suffix
    private fun canEndWord() = !secure && (target.after(1)?.firstOrNull()?.let {
        !it.isLetterOrDigit() && it !in "'_/@#"
    } ?: true)
    private fun inputDone() {
        lastShift = 0
        if (!locked && shifted) { shifted = false; showPlane() }
    }
    fun letter(text: String, x: Double, y: Double, grid: KeyGrid, explicit: Boolean = false) {
        if (!target.connected) return
        val w = word
        if (w == null || symbols || !text.single().isLetter()) { type(text); return }
        undo = null
        if (w.token.isEmpty()) {
            val b = before(); prefix = b.takeLast(40); w.reset(b)
            if (!TypingWord.boundary(b.lastOrNull()) || !canEndWord()) { type(text); return }
        }
        if (w.token.length >= TypingWord.MAX_LENGTH) { type(text); return }
        val start = if (expectedCaret >= 0) expectedCaret else minOf(selStart, selEnd)
        expectedCaret = if (start >= 0) start + text.length else -1
        if (target.commit(text)) {
            if (explicit) w.explicit(text) else w.letter(text, x, y, grid)
            showSuggestions(w)
        } else resetTyping()
        inputDone()
    }
    private fun replace(old: String, new: String): Boolean {
        if (secure || !target.connected) return false
        // With a non-empty selection deleteSurroundingText would remove text outside the selection.
        if (target.selected()?.isNotEmpty() == true || (old.isNotEmpty() && !endsWith(old))) return false
        target.beginBatch()
        return try {
            (old.isEmpty() || target.deleteBefore(old.length)) && target.commit(new)
        } finally { target.endBatch() }
    }
    fun pickSuggestion(text: String) {
        val w = word ?: return
        if (symbols || !canEndWord()) return
        val old = w.token
        if (old.isNotEmpty() && !endsWith(prefix + old)) { resetTyping(); surface?.suggestions(emptyList()); return }
        if (old.isEmpty() && !TypingWord.boundary(before().lastOrNull())) return
        if (replace(old, text + " ")) {
            resetTyping(); surface?.suggestions(w.predictions(text)); inputDone()
        }
    }
    fun type(value: String) {
        if (!target.connected) return
        val w = word
        if (value == " " && w != null && !symbols) {
            val old = w.token; val lang = w.decoder.model.language
            val fix = if (feedback.autocorrect && old.isNotEmpty() && endsWith(prefix + old) && canEndWord()) w.replacement() else null
            if (fix != null && !feedback.blocked(lang, old, fix) && replace(old, fix + " ")) {
                undo = Undo(lang, prefix, old, fix)
                if (expectedCaret >= 0) expectedCaret += fix.length + 1 - old.length
                w.reset(); surface?.suggestions(w.predictions(fix))
            } else {
                target.commit(" "); resetTyping(); surface?.suggestions(w.predictions(before()))
            }
        } else {
            resetTyping(); target.commit(value); surface?.suggestions(emptyList())
        }
        inputDone()
    }
    fun delete() {
        if (!target.connected) return
        val last = undo
        undo = null
        if (last != null && endsWith(last.prefix + last.replacement + " ") && replace(last.replacement + " ", last.original)) {
            feedback.undo(last.language, last.original, last.replacement)
            prefix = last.prefix; word?.restored(last.original, prefix)
            if (expectedCaret >= 0) expectedCaret += last.original.length - last.replacement.length - 1
            surface?.suggestions(emptyList(), literal = last.original); return
        }
        val w = word
        val track = w != null && w.token.isNotEmpty() && endsWith(prefix + w.token)
        val selected = (selStart >= 0 && selEnd >= 0 && selStart != selEnd) ||
            (!secure && target.selected()?.isNotEmpty() == true)
        if (selected) { target.commit(""); resetTyping() }
        else {
            if (!target.deleteCodePointsBefore(1)) target.sendKey(KeyEvent.KEYCODE_DEL)
            if (track) { w!!.pop(); if (expectedCaret >= 0) expectedCaret-- } else resetTyping()
        }
        if (w != null && w.token.isNotEmpty()) showSuggestions(w) else surface?.suggestions(emptyList())
    }
    fun enter() {
        resetTyping(); surface?.suggestions(emptyList())
        val info = editor
        if (info?.actionLabel != null && (info.imeOptions and EditorInfo.IME_FLAG_NO_ENTER_ACTION) == 0 &&
            target.performEditorAction(info.actionId)) return
        if (!target.defaultEditorAction()) {
            if (editor?.inputType == InputType.TYPE_NULL) target.sendKey(KeyEvent.KEYCODE_ENTER)
            else type("\n")
        }
    }

    /** A space-bar slide starts moving the cursor: the word in progress stays as typed. */
    fun cursorSlide(): CursorSlide {
        resetTyping(); surface?.suggestions(emptyList())
        return CursorSlide(target, minOf(selStart, selEnd), maxOf(selStart, selEnd), readable = !secure)
    }

    /** Dictation starts: the word in progress is left as typed. */
    fun clearForDictation() { resetTyping(); surface?.suggestions(emptyList()) }

    /** The editor or the keyboard went away: forget the word, never carry it to the next field. */
    fun leave() { resetTyping(); word = null; surface?.suggestions(emptyList()) }

    /** Dictated text with only the separators missing at the cursor. */
    fun insertDictation(text: String) {
        if (!target.connected) return
        val result = insertion(text, target.before(1)?.lastOrNull(), target.after(1)?.firstOrNull())
        if (result.isNotEmpty()) target.commit(result)
    }
}
