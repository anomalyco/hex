package ly.anoma.hex.keyboard

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.text.InputType
import android.view.KeyEvent
import android.view.inputmethod.EditorInfo
import java.util.concurrent.Executor
import ly.anoma.hex.speech.Spoken
import ly.anoma.hex.speech.SpokenText
import ly.anoma.hex.typing.KeyGrid
import ly.anoma.hex.typing.KeyPoint
import ly.anoma.hex.typing.PersonalWords
import ly.anoma.hex.typing.SpatialDecoder
import ly.anoma.hex.typing.Suggestion
import ly.anoma.hex.typing.TypingModel
import ly.anoma.hex.typing.TypingPreferences
import ly.anoma.hex.typing.TypingWord

/** What the engine shows: the keys' page and the strip's slots. */
internal interface KeyboardSurface {
    fun showPage(page: KeyboardPage)
    /** The strip, in the typing model's order: what was typed first when it shows, then the fix or the
     * likeliest word, then the rest. Next words after a space are [Suggestion.Kind.Next]. */
    fun suggestions(slots: List<Suggestion>)
    /** Offer Undo dictation, or stop offering it. */
    fun undoDictation(shown: Boolean)
}

/** Whether a dictation may be joined to the text at the cursor. A signed dictation (Labs › Signature) joins
 * the text before it as usual, with its signature on a line of its own; the dictation after it starts on a
 * new line below that signature while the signed text is still right before the cursor, so it is never
 * lowercased or spaced onto the signature line. */
internal class DictationContinuation {
    /** The signed dictation's inserted text, while the next dictation still starts a line below it. */
    private var signed: String? = null

    /** How far back before the cursor to read to tell whether a signed dictation is still there. */
    val reach get() = (signed?.length ?: 0) + 1

    /** Whether the text before the cursor ([before], read back from its end) still ends with a signed
     * dictation's text: the next dictation goes on a line below it, not on its line. The trailing space
     * [insertion] may have added after the signed text is ignored. */
    fun standalone(before: String?): Boolean {
        val text = signed ?: return false
        return before != null && (before.endsWith(text) || before.endsWith("$text "))
    }

    /** A dictation went in: [inserted] when a signature was appended, so the next one starts on a new line
     * while that text stays before the cursor. An unsigned dictation leaves the text it followed, so undoing
     * it puts the signature right before the cursor again. */
    fun record(inserted: String?) { if (inserted != null) signed = inserted.trimEnd(' ') }

    /** Any edit, a new field or leaving the editor ends it. */
    fun clear() { signed = null }
}

/** Keys in, text out: the current word, autocorrection and its undo, spaces, shift and the key pages.
 * It talks to the editor only through [KeyboardTextTarget] and to the views through [KeyboardSurface]. */
internal class KeyboardEngine(
    private val target: KeyboardTextTarget,
    private val feedback: TypingPreferences,
    private val layoutLanguage: () -> Spoken,
    private val loadDecoder: (String) -> SpatialDecoder,
    /** Reads Android's personal dictionary for a model; runs on [background]. */
    private val readPersonal: ((TypingModel) -> PersonalWords)? = null,
    private val background: Executor = Executor { it.run() },
    private val clock: () -> Long = { SystemClock.uptimeMillis() },
    private val main: Handler = Handler(Looper.getMainLooper()),
) {
    private companion object {
        /** The fix and the decoder's readings wait for this pause in typing: working them out costs a
         * search (2 ms median, 16 ms p95 per letter on the API 31 emulator), which shouldn't delay keys. */
        const val CHIP_PAUSE_MS = 100L
        /** Undo dictation stays this long unless something else happens first (iPhone Hex: 12 s). */
        const val UNDO_MS = 12_000L
    }
    private var pendingChip: Runnable? = null
    var surface: KeyboardSurface? = null
    private val models = HashMap<String, SpatialDecoder>()
    private var word: TypingWord? = null
    private var editor: EditorInfo? = null
    private var ordinaryField = false
    private var prefix = ""
    private var expectedCaret = -1
    private data class Undo(val language: String, val original: String, val replacement: String)
    private var undo: Undo? = null
    private val shift = ShiftState()
    private var symbols = false
    private var more = false
    // A number, phone or date field's pad, or the number pad the symbol page's 1234 key opens.
    private var pad: Pad? = null
    private var selStart = -1
    private var selEnd = -1
    // Spaces, each remembered with the text before the cursor it left, so anything else in between
    // (another key, a tap in the text) cancels it: a space for double-space, a picked suggestion's
    // own space, and a space a pick held because the text after the cursor already separates it.
    private var spaceAnchor: String? = null
    private var spaceAt = 0L
    private var pickedAnchor: String? = null
    private var heldAnchor: String? = null
    var secure = true
        private set
    // Undo dictation, as iPhone Hex's: the dictation just inserted and the text after it then, offered
    // back until the next edit, 12 s, or the cursor leaving its end, so the chip never does nothing.
    private data class Dictated(val text: String, val after: String)
    private var dictated: Dictated? = null
    private val dropDictation = Runnable { offerUndo(null) }
    private val continuation = DictationContinuation()

    fun startInput(attribute: EditorInfo) {
        edited()
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
        resetTyping(); word = null; clearSpaces()
        pad = Pad.of(attribute.inputType)
        symbols = pad != null
        more = false; shift.reset()
        selStart = attribute.initialSelStart; selEnd = attribute.initialSelEnd
        refreshCaps(force = true)
    }

    fun updateSelection(oldSelStart: Int, oldSelEnd: Int, newSelStart: Int, newSelEnd: Int) {
        selStart = newSelStart; selEnd = newSelEnd
        dictated?.let { if (!undoable(it)) offerUndo(null) }
        if (oldSelStart != newSelStart || oldSelEnd != newSelEnd) {
            if (expectedCaret >= 0 && (newSelStart != expectedCaret || newSelEnd != expectedCaret)) {
                // Editors may coalesce selection notifications. Compare the actual surrounding text
                // before invalidating our own in-flight commit, not a stale intermediate offset.
                val suffix = undo?.let { it.replacement + " " } ?: word?.token?.takeIf { it.isNotEmpty() }?.let { prefix + it }
                if (newSelStart != newSelEnd || suffix == null || !endsWith(suffix) ||
                    oldSelStart == expectedCaret) { resetTyping(); surface?.suggestions(emptyList()) }
            }
            refreshCaps()
        }
    }

    /** The number row changed (Labs › Keyboard › Number row): the keys redraw with it at once. */
    fun numberRowChanged() { showPlane() }

    /** Suggest swear words changed: the current word's suggestions follow at once. */
    fun swearWordsChanged() {
        word?.let {
            it.suggestSwearWords = feedback.suggestSwearWords
            showSuggestions(it)
        }
    }

    /** Android's personal dictionary changed: every loaded language reads it again, off the main thread. */
    fun personalWordsChanged() { for (decoder in models.values) loadPersonal(decoder) }
    private fun loadPersonal(decoder: SpatialDecoder) {
        val read = readPersonal ?: return
        background.execute { decoder.personal = read(decoder.model) }
    }

    /** Whether the field asks for a capital here (with Auto-capitalisation on), or caps lock is on: what shift would show. */
    private fun capsWanted(): Boolean {
        if (shift.locked) return true
        if (!feedback.autoCapitalize) return false
        val mask = (editor?.inputType ?: 0) and
            (InputType.TYPE_TEXT_FLAG_CAP_CHARACTERS or InputType.TYPE_TEXT_FLAG_CAP_WORDS or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES)
        return !secure && mask != 0 && target.capsMode(mask) != 0
    }
    private fun refreshCaps(force: Boolean = false) {
        val was = shift.uppercase
        shift.auto(capsWanted())
        if (force || was != shift.uppercase) showPlane()
    }
    private fun showPlane() {
        surface?.showPage(KeyboardPage(layoutLanguage(), symbols, more, shift.uppercase, shift.locked, EnterKey.from(editor),
            FieldKind.of(editor?.inputType ?: 0), pad, feedback.numberRow))
    }
    /** Shift went down: it acts at once, so a held shift and a letter type a capital. */
    fun shiftDown() {
        if (symbols) return
        shift.down(clock()); showPlane()
    }
    /** Shift lifted; on the symbol pages its key switches between them, on lifting. */
    fun shiftUp() {
        if (symbols) { more = !more; showPlane(); return }
        val was = shift.uppercase
        shift.up()
        if (was != shift.uppercase) showPlane()
    }
    fun mode() { resetTyping(); clearSpaces(); surface?.suggestions(emptyList()); symbols = !symbols; more = false; shift.forgetTap(); showPlane() }
    /** Back to the letters page from wherever (the emoji panel's and the symbol pad's ABC). */
    fun letters() {
        if (pad == null && !symbols) return
        resetTyping(); clearSpaces(); surface?.suggestions(emptyList())
        pad = null; symbols = false; more = false; shift.forgetTap(); showPlane()
    }
    /** A pad's own switches: the phone pad's symbols and back, the symbol page's 1234 key. */
    fun pad(next: Pad) { resetTyping(); clearSpaces(); surface?.suggestions(emptyList()); pad = next; symbols = true; more = false; showPlane() }
    /** The symbol pad's !?#: back to the symbol page. */
    fun symbolsPage() { resetTyping(); pad = null; symbols = true; more = false; showPlane() }
    private fun resetTyping() {
        cancelChip(); word?.reset(); undo = null; expectedCaret = -1; prefix = ""
    }
    private fun clearSpaces() { spaceAnchor = null; pickedAnchor = null; heldAnchor = null }
    private fun cancelChip() { pendingChip?.let(main::removeCallbacks); pendingChip = null }
    fun configureTyping(keepWord: Boolean = false) {
        val language = layoutLanguage().layoutCode
        val active = word
        if (ordinaryField && keepWord && active != null && active.decoder.model.language == language && active.spatial == feedback.spatial &&
            active.token.isNotEmpty() && endsWith(prefix + active.token)) {
            active.addsCapitals = feedback.autoCapitalize
            showPlane(); showSuggestions(active); return
        }
        resetTyping()
        word = if (ordinaryField) {
            try {
                TypingWord(models.getOrPut(language) { loadDecoder(language).also(::loadPersonal) }, feedback.spatial)
                    .also { it.suggestSwearWords = feedback.suggestSwearWords; it.addsCapitals = feedback.autoCapitalize }
            } catch (_: java.io.IOException) { null } // The editor can still type if its assets cannot be read.
        } else null
        showPlane()
        showSuggestions(word)
    }

    // The strip while a word is typed: completions at once, then, after a pause, the model's whole
    // strip with what space will put in. A fix you turned down twice shows as a plain word.
    private fun showSuggestions(w: TypingWord?) {
        cancelChip()
        if (w != null && w.token.isNotEmpty()) {
            surface?.suggestions(w.strip(autocorrect = false))
            if (!feedback.autocorrect || !canEndWord()) return
            val token = w.token
            pendingChip = Runnable {
                pendingChip = null
                if (word === w && w.token == token) surface?.suggestions(w.strip(autocorrect = true).map {
                    if (it.kind == Suggestion.Kind.Fix && feedback.blocked(w.decoder.model.language, token, it.text))
                        Suggestion(it.text, Suggestion.Kind.Word) else it
                })
            }.also { main.postDelayed(it, CHIP_PAUSE_MS) }
            return
        }
        val b = if (w != null && !symbols && selStart == selEnd) before() else null
        showPredictions(if (b != null && TypingWord.boundary(b.lastOrNull())) b else null)
    }
    /** Next words for the text before the cursor, capitalised as shift would be. */
    private fun showPredictions(before: String?) {
        val w = word
        surface?.suggestions(if (w != null && before != null && !symbols && canEndWord())
            w.predictions(before, capitalize = capsWanted()).map { Suggestion(it, Suggestion.Kind.Next) } else emptyList())
    }
    private fun before() = if (secure) "" else target.before(96).orEmpty()
    private fun endsWith(suffix: String) = !secure && target.before(suffix.length) == suffix
    private fun canEndWord() = !secure && (target.after(1)?.firstOrNull()?.let {
        !it.isLetterOrDigit() && it !in "'_/@#"
    } ?: true)
    private fun inputDone() {
        val was = shift.uppercase
        shift.typed()
        if (was != shift.uppercase) showPlane()
    }
    /** The letter a touch near a key's edge types: the key it landed on, or a neighbour within
     * TypingWord.REACH dp ([near], distances in dp) that the word list makes clearly likelier next. Only
     * while autocorrect is on in an ordinary text field on the letters page, and never with caps lock. */
    fun likelierLetter(hit: Char, near: Map<Char, Double>, keys: Collection<Char>): Char {
        val w = word ?: return hit
        if (near.isEmpty() || symbols || shift.locked || !feedback.autocorrect || !target.connected) return hit
        return w.likelierKey(before(), hit, near, keys)
    }
    /** Shift as the keys show it: letters type capitals while it is on or held. */
    val uppercase get() = shift.uppercase

    fun letter(text: String, x: Double, y: Double, grid: KeyGrid, explicit: Boolean = false) {
        if (!target.connected) return
        edited()
        val w = word
        if (w == null || symbols || !text.single().isLetter()) { type(text); return }
        undo = null
        if (w.token.isEmpty()) {
            val b = takeHeldSpace(before())
            prefix = b.takeLast(40); w.reset(b)
            if (!w.startsWord(b) || !canEndWord()) { type(text); return }
        }
        clearSpaces()
        if (w.token.length >= TypingWord.MAX_LENGTH) { type(text); return }
        val start = if (expectedCaret >= 0) expectedCaret else minOf(selStart, selEnd)
        expectedCaret = if (start >= 0) start + text.length else -1
        if (target.commit(text)) {
            if (explicit) w.explicit(text) else w.letter(text, x, y, grid)
            showSuggestions(w)
        } else resetTyping()
        inputDone()
    }
    // A space a pick held goes in before the next word starts; returns the text before the cursor.
    private fun takeHeldSpace(before: String): String {
        if (heldAnchor == null || heldAnchor != before) return before
        heldAnchor = null
        if (!target.commit(" ")) return before
        if (expectedCaret >= 0) expectedCaret++
        return "$before "
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

    /** A strip slot was tapped. What was typed, a fix or a word goes in with its space, unless the text
     * after the cursor already starts with a space or closing mark: then the space waits for the next
     * key that starts a word. A space right after a picked word is swallowed (it still counts towards
     * double-space), and a closing mark takes its place. */
    fun pickSuggestion(text: String) {
        val w = word ?: return
        edited()
        if (symbols || !canEndWord()) return
        val old = w.token
        if (old.isNotEmpty() && !endsWith(prefix + old)) { resetTyping(); surface?.suggestions(emptyList()); return }
        if (old.isEmpty() && !w.startsWord(before())) return
        val holds = SpaceRules.separates(target.after(1))
        clearSpaces()
        if (replace(old, if (holds) text else "$text ")) {
            resetTyping()
            val b = before()
            if (holds) heldAnchor = b else pickedAnchor = b
            showPredictions(b)
            inputDone()
        }
    }

    /** The space bar, tapped at [point] in [grid]'s coordinates (null off the letters page). */
    fun space(point: KeyPoint? = null, grid: KeyGrid? = null) {
        if (!target.connected) return
        edited()
        val now = clock()
        val b = before()
        when {
            // The text after the cursor already separates the picked word: space has nothing to add.
            heldAnchor != null && heldAnchor == b -> { clearSpaces(); showPredictions(b); return }
            pickedAnchor != null && pickedAnchor == b -> { clearSpaces(); spaceAnchor = b; spaceAt = now; showPredictions(b); return }
            word != null && spaceAnchor == b && now - spaceAt <= SpaceRules.DOUBLE_SPACE_MS && SpaceRules.periodFits(b) -> {
                clearSpaces()
                if (replace(" ", ". ")) { resetTyping(); showPredictions(before()); inputDone(); return }
            }
        }
        clearSpaces()
        val w = word
        if (w != null && !symbols && canEndWord()) {
            val old = w.token; val lang = w.decoder.model.language
            // Your text replacements first, whatever autocorrect is set to; they never offer an undo.
            val swap = w.textReplacement(b)
            if (swap != null && replace(swap.original, swap.replacement + " ")) { resetTyping(); afterSpace(now); return }
            if (feedback.autocorrect && old.isNotEmpty() && endsWith(prefix + old)) {
                val fix = w.finish(' ', point, grid)
                if (fix != null && !feedback.blocked(lang, fix.original, fix.replacement) && replace(fix.original, fix.replacement + " ")) {
                    cancelChip()
                    undo = Undo(lang, fix.original, fix.replacement)
                    if (expectedCaret >= 0) expectedCaret += fix.replacement.length + 1 - fix.original.length
                    w.reset(); afterSpace(now); return
                }
            }
        }
        target.commit(" "); resetTyping(); afterSpace(now)
    }
    private fun afterSpace(now: Long) {
        val b = before()
        spaceAnchor = b; spaceAt = now
        showPredictions(b); inputDone()
    }

    fun type(value: String) {
        if (!target.connected) return
        edited()
        if (value == " ") { space(); return }
        val b = before()
        val mark = value.length == 1 && TypingWord.endsWord(value[0])
        // A clause-ending mark takes the space a picked suggestion typed ("Hello,").
        if (mark && value[0] in SpaceRules.CLAUSE_MARKS && pickedAnchor != null && pickedAnchor == b && b.endsWith(" ")) {
            clearSpaces()
            if (replace(" ", value)) { resetTyping(); showPredictions(before()); inputDone(); return }
        }
        // Anything that starts another word puts a held space in first; marks go against the word.
        val text = if (value.any { it in SpaceRules.JOINERS || it.isWhitespace() }) b else takeHeldSpace(b)
        clearSpaces()
        val w = word
        if (mark && w != null && !symbols && canEndWord()) {
            val swap = w.textReplacement(text)
            if (swap != null && replace(swap.original, swap.replacement + value)) { resetTyping(); showPredictions(before()); inputDone(); return }
            if (feedback.autocorrect && w.token.isNotEmpty() && endsWith(prefix + w.token)) {
                val fix = w.finish(value[0])
                if (fix != null && !feedback.blocked(w.decoder.model.language, fix.original, fix.replacement) &&
                    replace(fix.original, fix.replacement + value)) { resetTyping(); showPredictions(before()); inputDone(); return }
            }
        }
        resetTyping(); target.commit(value); showPredictions(if (w != null) before() else null)
        inputDone()
    }
    fun delete() {
        if (!target.connected) return
        edited()
        // Right after a pick held its space, delete takes that space back and deletes nothing.
        if (heldAnchor != null && heldAnchor == before()) { clearSpaces(); surface?.suggestions(emptyList()); return }
        clearSpaces()
        val last = undo
        undo = null
        if (last != null && endsWith(last.replacement + " ") && replace(last.replacement + " ", last.original)) {
            feedback.undo(last.language, last.original, last.replacement)
            val typed = last.original.substringAfterLast(' ')
            prefix = before().dropLast(typed.length).takeLast(40); word?.restored(typed, prefix)
            if (expectedCaret >= 0) expectedCaret += last.original.length - last.replacement.length - 1
            surface?.suggestions(listOf(Suggestion(typed, Suggestion.Kind.Typed))); return
        }
        val w = word
        val track = w != null && w.token.isNotEmpty() && endsWith(prefix + w.token)
        val selected = (selStart >= 0 && selEnd >= 0 && selStart != selEnd) ||
            (!secure && target.selected()?.isNotEmpty() == true)
        if (selected) { target.commit(""); resetTyping() }
        else {
            // A whole character as people see it: 👍🏽, a flag or a family is one, as on Gboard.
            val cluster = if (track || secure) 1 else DeleteSlide.lastCharacter(target.before(32).orEmpty())
            if (!(if (cluster > 1) target.deleteBefore(cluster) else target.deleteCodePointsBefore(1))) target.sendKey(KeyEvent.KEYCODE_DEL)
            if (track) { w!!.pop(); if (expectedCaret >= 0) expectedCaret-- } else resetTyping()
        }
        if (w != null && w.token.isNotEmpty()) showSuggestions(w) else surface?.suggestions(emptyList())
    }
    fun enter() {
        edited()
        // A text replacement that ends the text goes in before the line break or the editor's action.
        word?.takeIf { canEndWord() }?.textReplacement(before())?.let { replace(it.original, it.replacement) }
        resetTyping(); clearSpaces(); surface?.suggestions(emptyList())
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
        resetTyping(); clearSpaces(); surface?.suggestions(emptyList())
        return CursorSlide(target, minOf(selStart, selEnd), maxOf(selStart, selEnd), readable = !secure)
    }

    /** A slide left from delete: null where it can't run (a password field, a selection, no known cursor). */
    fun deleteSlide(): DeleteSlide? {
        if (secure || selStart < 0 || selStart != selEnd || !target.connected) return null
        resetTyping(); clearSpaces(); surface?.suggestions(emptyList())
        return DeleteSlide(target, selStart, target.before(2000).orEmpty())
    }
    /** The delete slide lifted with words selected: they go. */
    fun deleteSelection() {
        if (!target.connected) return
        edited()
        target.commit(""); resetTyping(); surface?.suggestions(emptyList())
    }

    /** Dictation starts: the word in progress is left as typed. The signed dictation before still makes this
     * one stand alone, so its continuation is kept. */
    fun clearForDictation() { offerUndo(null); resetTyping(); clearSpaces(); surface?.suggestions(emptyList()) }

    /** The editor or the keyboard went away: forget the word, never carry it to the next field. */
    fun leave() { edited(); resetTyping(); clearSpaces(); word = null; surface?.suggestions(emptyList()) }

    /** Dictated text with only the separators missing at the cursor; mid-sentence its first word loses
     * the capital Whisper gave it ([SpokenText.continuing], [language] as dictated or "en" translated).
     * [signature] says a signature was appended (Labs › Signature): it joins the text at the cursor as
     * usual, with the signature on a line of its own. The next dictation starts on a new line below that
     * signature while the signed text is still right before the cursor, so it is never lowercased or
     * spaced onto the signature line. */
    fun insertDictation(text: String, language: String?, signature: Boolean = false) {
        if (!target.connected) return
        val before = if (secure) null else target.before(maxOf(64, continuation.reach))
        val standalone = continuation.standalone(before)
        val joinable = !signature && !standalone
        val shaped = if (!joinable) text else {
            // The keyboard's own word list for that language, when loaded, knows far more ordinary words than
            // the speech side's short list: a word it has in lowercase and the books rarely capitalise
            // loses Whisper's capital; a name keeps it (iOS asks UITextChecker the same).
            val model = models[if (language == "no") "nb" else language]?.model
            if (model == null) SpokenText.continuing(text, before, language)
            else SpokenText.continuing(text, before, language) { model.index(it) >= 0 && (model.capitals?.percent(it) ?: -1) < 50 }
        }
        val result = dictationInsertion(shaped, before, target.after(1)?.firstOrNull(), standalone)
        if (result.isNotEmpty() && target.commit(result)) {
            continuation.record(if (signature) result else null)
            if (!secure) offerUndo(Dictated(result, target.after(16).orEmpty()))
        }
    }

    /** Undo dictation: the last dictation goes, if it is still right before the cursor with nothing selected. */
    fun undoDictation() {
        val d = dictated ?: return
        offerUndo(null)
        if (undoable(d) && replace(d.text, "")) {
            resetTyping(); clearSpaces(); showPredictions(before())
        }
    }
    private fun undoable(d: Dictated): Boolean {
        if (secure || !target.connected || target.selected()?.isNotEmpty() == true || !endsWith(d.text)) return false
        val after = target.after(d.after.length).orEmpty()
        return after.startsWith(d.after) || d.after.startsWith(after)
    }
    private fun offerUndo(next: Dictated?) {
        main.removeCallbacks(dropDictation)
        val shown = dictated != null
        dictated = next
        if (next != null) main.postDelayed(dropDictation, UNDO_MS)
        if (shown || next != null) surface?.undoDictation(next != null)
    }
    /** Any edit through the keys ends the Undo dictation offer, and a signed dictation's effect on the next. */
    private fun edited() { if (dictated != null) offerUndo(null); continuation.clear() }
}
