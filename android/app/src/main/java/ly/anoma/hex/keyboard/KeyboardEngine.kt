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
import ly.anoma.hex.typing.SmartQuotes
import ly.anoma.hex.typing.SpatialDecoder
import ly.anoma.hex.typing.Suggestion
import ly.anoma.hex.typing.SwipeReader
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
    /** The strip's tools changed (Labs › Keyboard › Keyboard tools): Undo follows the last autocorrection. */
    fun toolsChanged() {}
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
    /** The typing model's language, or null when the keys are only English's stand-in for a dictation language
     * with no layout: they keep English's keys and accents, but must not suggest or correct English words. */
    private val typingLanguage: () -> String?,
    private val loadDecoder: (String) -> SpatialDecoder,
    /** Reads Android's personal dictionary for a model; runs on [background]. */
    private val readPersonal: ((TypingModel) -> PersonalWords)? = null,
    private val background: Executor = Executor { it.run() },
    /** The bundled emoji words (Labs › Keyboard › Emoji suggestions), read lazily. */
    private val emoji: EmojiWords? = null,
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
    // Whether the field reads words at all (suggestions, next words, swipes, tap correction) and whether it
    // also autocorrects on space and punctuation: a field asking for no suggestions keeps the words, not the fix.
    private var wordField = false
    private var correctingField = false
    // A URL or web search field (FieldTyping.exact): no double-space period, text replacement or nearby letter, and
    // a picked word's space gives way to any joining mark ("github." and "github/").
    private var exactField = false
    // A field that keeps its own text (TYPE_NULL, a terminal's): nothing can be read back or edited as text, so
    // delete, the cursor and Enter go in as key events.
    private var keyEventEditor = false
    // A field that asks not to be learned from (IME_FLAG_NO_PERSONALIZED_LEARNING): suggestions stay, but
    // nothing typed there is remembered (an undone correction, an emoji pick).
    var learnsTyping = true
        private set
    private var prefix = ""
    private var expectedCaret = -1
    private data class Undo(val language: String, val original: String, val replacement: String)
    private var undo: Undo? = null
    private val shift = ShiftState()
    private var symbols = false
    private var more = false
    // A number, phone or date field's pad, or the number pad the symbol page's 1234 key opens.
    private var pad: Pad? = null
    // Whether the keys' emoji key was on the page the strip's Emoji tool mirrors (the letters).
    private var lastEmojiAvailable = true
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
    // Keyboard › Swipe typing (SwipeTyping.kt). The word a swipe just typed, until anything else changes the text:
    // the strip offers its other readings, any of which takes its place, and delete takes it back.
    private data class Swiped(val word: String, val alternatives: List<String>)
    private var swiped: Swiped? = null
    // The text before the cursor right after a swiped word: a letter, digit or opening mark tapped next goes in after
    // a space, as Gboard keeps one pending; closing marks go against the word (SpaceRules.JOINERS).
    private var swipeAnchor: String? = null
    // The word delete just took back and the text before the cursor then: the strip offers it again (Gboard).
    private var takenBack: Pair<String, String>? = null
    private val retries = SwipeRetries()
    /** A swipe is under way: from the touch becoming one until it ends. */
    var swiping = false
        private set

    fun startInput(attribute: EditorInfo) {
        edited()
        editor = attribute
        val field = FieldTyping.of(attribute.inputType, attribute.imeOptions)
        secure = field.secure
        wordField = field.words
        correctingField = field.corrects
        exactField = field.exact
        keyEventEditor = field.keyEvents
        learnsTyping = field.learns
        resetTyping(); word = null; clearSpaces(); forgetSwipe()
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
            // The cursor left the word a swipe typed (or the place delete took it back from): its strip goes.
            val s = swiped; val back = takenBack
            if (!swiping && ((s != null && (newSelStart != newSelEnd || !endsWith(s.word))) ||
                    (back != null && (newSelStart != newSelEnd || before() != back.second)))) {
                forgetSwipe(); retries.forget(); surface?.suggestions(emptyList())
            }
            refreshCaps()
            // The strip's Undo tool follows the autocorrection still being right before the cursor.
            if (feedback.keyboardTools && undo != null) surface?.toolsChanged()
        }
    }

    /** The number row changed (Labs › Keyboard › Number row): the keys redraw with it at once. */
    fun numberRowChanged() { showPlane() }

    /** The local currency changed (Labs › Keyboard › Local currency): the 123 page redraws with the phone's own. */
    fun localCurrencyChanged() { showPlane() }

    /** Suggest swear words changed: the current word's suggestions follow at once. */
    fun swearWordsChanged() {
        word?.let {
            it.suggestSwearWords = feedback.suggestSwearWords
            showSuggestions(it)
        }
    }

    /** Labs › Keyboard › Emoji suggestions changed: the current word's strip follows at once,
     * reading the bundled list then if it was never read. */
    fun emojiSuggestionsChanged() { word?.let { showSuggestions(it) } }

    /** Labs › Keyboard › Keyboard tools changed: the strip follows at once. */
    fun keyboardToolsChanged() { surface?.toolsChanged() }

    /** The clipboard changed: the strip's Paste tool comes or goes at once. */
    fun clipboardChanged() { surface?.toolsChanged() }

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
        // A number, phone or date pad never draws a currency, so it never reads the phone's.
        surface?.showPage(KeyboardPage(layoutLanguage(), symbols, more, shift.uppercase, shift.locked, EnterKey.from(editor),
            FieldKind.of(editor?.inputType ?: 0), pad, feedback.numberRow,
            if (feedback.localCurrency && symbols && pad == null) LocalCurrency.local() else null))
        // The strip's Emoji tool follows the page (Labs › Keyboard › Keyboard tools): the keys' emoji key is
        // on the letters only, never on a symbol page or a pad.
        val emoji = emojiAvailable
        val changed = emoji != lastEmojiAvailable
        lastEmojiAvailable = emoji
        if (changed && feedback.keyboardTools) surface?.toolsChanged()
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
    /** ?123 and ABC. A word being typed goes along, as on Gboard and iPhone Hex: its strip stays, a mark on the
     * symbol page ends it as on the letters ("teh" ?123 "?" is "the?"), and back on the letters it goes on. */
    fun mode() {
        symbols = !symbols; more = false; shift.forgetTap(); showPlane()
        val w = word
        if (w != null && w.token.isNotEmpty() && endsWith(prefix + w.token)) { showSuggestions(w); return }
        resetTyping(); clearSpaces(); surface?.suggestions(emptyList())
    }
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
    private fun clearSpaces() { spaceAnchor = null; pickedAnchor = null; heldAnchor = null; swipeAnchor = null }
    private fun forgetSwipe() { swiped = null; takenBack = null }
    private fun cancelChip() { pendingChip?.let(main::removeCallbacks); pendingChip = null }
    fun configureTyping(keepWord: Boolean = false) {
        val typing = typingLanguage()
        // No model of its own: English's word list still gives the keys their geometry and your personal
        // dictionary, marked as a stand-in so none of English's words reach the strip or autocorrect.
        val language = typing ?: layoutLanguage().layoutCode
        val standIn = typing == null
        val active = word
        if (wordField && keepWord && active != null && active.standIn == standIn && active.decoder.model.language == language &&
            active.spatial == feedback.spatial && active.token.isNotEmpty() && endsWith(prefix + active.token)) {
            active.addsCapitals = feedback.autoCapitalize
            showPlane(); showSuggestions(active); return
        }
        resetTyping()
        word = if (wordField) {
            try {
                TypingWord(models.getOrPut(language) { loadDecoder(language).also(::loadPersonal) }, feedback.spatial)
                    .also {
                        it.standIn = standIn
                        it.suggestSwearWords = feedback.suggestSwearWords
                        it.addsCapitals = feedback.autoCapitalize
                    }
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
            surface?.suggestions(Suggestion.withEmoji(w.strip(autocorrect = false).map(::shown), emojiFor(w)))
            // A stand-in's strip already is the finished one: nothing there changes after the pause.
            // A field that asked for no suggestions of its own gets no fix: space leaves its words alone.
            if (!feedback.autocorrect || !correctingField || w.standIn || !canEndWord()) return
            val token = w.token
            pendingChip = Runnable {
                pendingChip = null
                if (word === w && w.token == token) surface?.suggestions(Suggestion.withEmoji(w.strip(autocorrect = true).map {
                    if (it.kind == Suggestion.Kind.Fix && feedback.blocked(w.decoder.model.language, token, it.text))
                        Suggestion(it.text, Suggestion.Kind.Word) else it
                }.map(::shown), emojiFor(w)))
            }.also { main.postDelayed(it, CHIP_PAUSE_MS) }
            return
        }
        val b = if (w != null && !symbols && selStart == selEnd) before() else null
        showPredictions(if (b != null && TypingWord.boundary(b.lastOrNull())) b else null)
    }
    /** The current word's emoji when Labs › Keyboard › Emoji suggestions is on and the word names
     * one. The bundled list is read off the main thread the first time; once it is in memory this
     * word's strip is shown again. */
    private fun emojiFor(w: TypingWord): String? {
        if (w.standIn || !feedback.emojiSuggestions) return null
        val list = emoji ?: return null
        val hit = list.match(w.token, w.decoder.model.language)
        if (hit == null && !list.requested) list.load {
            // A pending autocorrect runnable draws this word again with the emoji when it fires.
            // Re-rendering now would cancel and restart its delay, flickering the Fix chip.
            main.post {
                if (pendingChip == null && word === w && w.token.isNotEmpty() && feedback.emojiSuggestions)
                    showSuggestions(w)
            }
        }
        return hit
    }

    /** Next words for the text before the cursor, capitalised as shift would be. */
    private fun showPredictions(before: String?) {
        val w = word
        surface?.suggestions(if (w != null && before != null && !symbols && canEndWord())
            w.predictions(before, capitalize = capsWanted()).map { shown(Suggestion(it, Suggestion.Kind.Next)) } else emptyList())
    }

    /** A slot as the strip shows it: with Labs › Keyboard › Smart quotes on, a word shows the ’ it
     * will insert ("I'm" shows "I’m"). A pick runs [smartWord] over this text again, which leaves a
     * curly apostrophe unchanged. */
    private fun shown(slot: Suggestion) =
        if (smartQuotesHere()) Suggestion(SmartQuotes.word(slot.text), slot.kind) else slot
    private fun before() = if (secure) "" else target.before(96).orEmpty()
    private fun endsWith(suffix: String) = !secure && target.before(suffix.length) == suffix
    private fun canEndWord() = !secure && (target.after(1)?.firstOrNull()?.let {
        !it.isLetterOrDigit() && it !in "'’_/@#"
    } ?: true)
    private fun inputDone() {
        val was = shift.uppercase
        shift.typed()
        if (was != shift.uppercase) showPlane()
    }
    /** The letter a touch near a key's edge types: the key it landed on, or a neighbour within
     * TypingWord.REACH dp ([near], distances in dp) that the word list makes clearly likelier next. Only
     * while autocorrect is on in a word field on the letters page, never in a URL field, and never with caps lock. */
    fun likelierLetter(hit: Char, near: Map<Char, Double>, keys: Collection<Char>): Char {
        val w = word ?: return hit
        if (w.standIn || exactField || near.isEmpty() || symbols || shift.locked || !feedback.autocorrect || !target.connected) return hit
        return w.likelierKey(before(), hit, near, keys)
    }
    /** Shift as the keys show it: letters type capitals while it is on or held. */
    val uppercase get() = shift.uppercase

    /** Whether the keys' emoji key is there to open the panel: on the letters page only, never on the
     * symbol pages or a number, phone or date pad, where it is the pad key or absent. */
    val emojiAvailable get() = pad == null && !symbols

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
        val held = heldAnchor != null && heldAnchor == before
        if (!held && (swipeAnchor == null || swipeAnchor != before)) return before
        heldAnchor = null; swipeAnchor = null
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
     * double-space), and a closing mark takes its place (in a URL field, also / and the other joiners). */
    fun pickSuggestion(text: String) {
        val w = word ?: return
        if (pickSwiped(text)) return
        edited()
        if ((symbols && w.token.isEmpty()) || !canEndWord()) return
        val old = w.token
        if (old.isNotEmpty() && !endsWith(prefix + old)) { resetTyping(); surface?.suggestions(emptyList()); return }
        if (old.isEmpty() && !w.startsWord(before())) return
        val holds = SpaceRules.separates(target.after(1))
        clearSpaces()
        val committed = smartWord(text)
        if (replace(old, if (holds) committed else "$committed ")) {
            resetTyping()
            val b = before()
            if (holds) heldAnchor = b else pickedAnchor = b
            showPredictions(b)
            inputDone()
        }
    }

    /** A word a pick, autocorrection or swipe commits: with Labs › Keyboard › Smart quotes on, its
     * straight apostrophes go in as ’ ("don't" becomes "don’t"). Off, it is exactly the text offered. */
    private fun smartWord(text: String) = if (smartQuotesHere()) SmartQuotes.word(text) else text

    /** Whether Labs › Keyboard › Smart quotes applies at the cursor: its toggle on, in a plain text
     * field, and not in a password, email, URL or TYPE_NULL field (a terminal), whose text must keep
     * the straight quotes it has always taken. */
    private fun smartQuotesHere(): Boolean {
        if (!feedback.smartQuotes || secure) return false
        val type = editor?.inputType ?: 0
        return (type and InputType.TYPE_MASK_CLASS) == InputType.TYPE_CLASS_TEXT && FieldKind.of(type) == FieldKind.Text
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
            word != null && !exactField && spaceAnchor == b && now - spaceAt <= SpaceRules.DOUBLE_SPACE_MS && SpaceRules.periodFits(b) -> {
                clearSpaces()
                if (replace(" ", ". ")) { resetTyping(); showPredictions(before()); inputDone(); return }
            }
        }
        // Space never changes a swiped word: it is a reading, not a typo (iPhone Hex).
        val afterSwipe = swipeAnchor != null && swipeAnchor == b
        clearSpaces()
        val w = word
        if (w != null && !symbols && !exactField && canEndWord() && !afterSwipe) {
            val old = w.token; val lang = w.decoder.model.language
            // Your text replacements first, whatever autocorrect is set to; they never offer an undo.
            val swap = w.textReplacement(b)
            if (swap != null && replace(swap.original, smartWord(swap.replacement) + " ")) { resetTyping(); afterSpace(now); return }
            if (feedback.autocorrect && correctingField && old.isNotEmpty() && endsWith(prefix + old)) {
                val fix = w.finish(' ', point, grid, ::smartWord)
                if (fix != null && !feedback.blocked(lang, fix.original, fix.replacement) &&
                    replace(fix.original, fix.replacement + " ")) {
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

    /** A quote key's own character (' or ") as it goes in: Labs › Keyboard › Smart quotes makes it
     * ’ or ‘ and “ or ” by the text before the cursor; off, and in a password field or an email or
     * URL field, it is exactly the key's character. The keys' long-press alternates never come here:
     * they type what they show. */
    fun quote(value: Char) {
        if (!target.connected) return
        val placed = if (smartQuotesHere()) SmartQuotes.typed(value, before().lastOrNull()) else value
        type(placed.toString())
    }

    fun type(value: String) {
        if (!target.connected) return
        edited()
        if (value == " ") { space(); return }
        val b = before()
        val mark = value.length == 1 && TypingWord.endsWord(value[0])
        // A clause-ending mark takes the space a picked suggestion typed ("Hello,"); in a URL field, so does any mark
        // that joins a word ("github." and "github/", never "github /").
        if (value.length == 1 && SpaceRules.takesPickedSpace(value[0], exactField) && pickedAnchor != null && pickedAnchor == b && b.endsWith(" ")) {
            clearSpaces()
            if (replace(" ", value)) { resetTyping(); showPredictions(before()); inputDone(); return }
        }
        // Anything that starts another word puts a held space in first; marks go against the word.
        val afterSwipe = swipeAnchor != null && swipeAnchor == b
        val text = if (value.any { it in SpaceRules.JOINERS || it.isWhitespace() }) b else takeHeldSpace(b)
        clearSpaces()
        val w = word
        if (mark && w != null && pad == null && !exactField && canEndWord() && !afterSwipe) {
            val swap = w.textReplacement(text)
            if (swap != null && replace(swap.original, smartWord(swap.replacement) + value)) { resetTyping(); showPredictions(before()); inputDone(); return }
            if (feedback.autocorrect && correctingField && w.token.isNotEmpty() && endsWith(prefix + w.token)) {
                val fix = w.finish(value[0], commit = ::smartWord)
                if (fix != null && !feedback.blocked(w.decoder.model.language, fix.original, fix.replacement) &&
                    replace(fix.original, fix.replacement + value)) { resetTyping(); showPredictions(before()); inputDone(); return }
            }
        }
        resetTyping(); target.commit(value); showPredictions(if (w != null) before() else null)
        inputDone()
    }
    fun delete() {
        if (!target.connected) return
        // A TYPE_NULL field's text can't be read back, and its editor takes no surrounding-text delete: a
        // delete is the key press itself, one per call, and a held key repeats the call as Gboard's does.
        if (keyEventEditor) {
            edited(); clearSpaces(); resetTyping(); surface?.suggestions(emptyList())
            target.sendKey(KeyEvent.KEYCODE_DEL)
            return
        }
        if (deleteSwiped()) return
        edited()
        // Right after a pick held its space, delete takes that space back and deletes nothing.
        if (heldAnchor != null && heldAnchor == before()) { clearSpaces(); surface?.suggestions(emptyList()); return }
        clearSpaces()
        val last = undo
        undo = null
        if (last != null && revert(last)) {
            surface?.suggestions(listOf(Suggestion(last.original.substringAfterLast(' '), Suggestion.Kind.Typed))); return
        }
        val w = word
        val track = w != null && w.token.isNotEmpty() && endsWith(prefix + w.token)
        val selected = (selStart >= 0 && selEnd >= 0 && selStart != selEnd) ||
            (!secure && target.selected()?.isNotEmpty() == true)
        if (selected) { target.commit(""); resetTyping() }
        else {
            // A whole character as people see it: 👍🏽, a flag or a family is one, as on Gboard.
            val cluster = if (track || secure) 1 else DeleteSlide.lastCharacter(target.before(32).orEmpty())
            // Drop the letter before the editor does: an editor in Hex's own process reports the new
            // selection during the delete call, and that report must find the word already shorter.
            if (track) { w!!.pop(); if (expectedCaret >= 0) expectedCaret-- }
            if (!(if (cluster > 1) target.deleteBefore(cluster) else target.deleteCodePointsBefore(1))) target.sendKey(KeyEvent.KEYCODE_DEL)
            if (!track) resetTyping()
        }
        if (w != null && w.token.isNotEmpty()) showSuggestions(w) else surface?.suggestions(emptyList())
    }
    /** Text went in through the editor's own paste (the strip's Paste tool): the word in progress is not
     * what is before the cursor any more. */
    fun pasted() { edited(); resetTyping(); clearSpaces(); surface?.suggestions(emptyList()) }

    fun enter() {
        edited()
        // A text replacement that ends the text goes in before the line break or the editor's action.
        word?.takeIf { canEndWord() }?.textReplacement(before())?.let { replace(it.original, smartWord(it.replacement)) }
        resetTyping(); clearSpaces(); surface?.suggestions(emptyList())
        val info = editor
        if (info?.actionLabel != null && (info.imeOptions and EditorInfo.IME_FLAG_NO_ENTER_ACTION) == 0 &&
            target.performEditorAction(info.actionId)) return
        if (!target.defaultEditorAction()) {
            if (keyEventEditor) target.sendKey(KeyEvent.KEYCODE_ENTER)
            else type("\n")
        }
    }

    /** A space-bar slide starts moving the cursor: the word in progress stays as typed. A field whose text
     * isn't there to read, a password or a TYPE_NULL editor, steps with arrow keys instead. */
    fun cursorSlide(): CursorSlide {
        resetTyping(); clearSpaces(); surface?.suggestions(emptyList())
        return CursorSlide(target, minOf(selStart, selEnd), maxOf(selStart, selEnd),
            readable = !secure && !keyEventEditor)
    }

    /** A slide left from delete: null where it can't run (a password or TYPE_NULL field, a selection, no known
     * cursor). */
    fun deleteSlide(): DeleteSlide? {
        if (secure || keyEventEditor || selStart < 0 || selStart != selEnd || !target.connected) return null
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
        // Spacing is decided on the straight quotes, as it always was, so a dictation's spacing is
        // unchanged: the same smart quotes as the keys and the strip then go in with their spacing.
        val spaced = dictationInsertion(shaped, before, target.after(1)?.firstOrNull(), standalone)
        val result = if (smartQuotesHere()) SmartQuotes.text(spaced, before?.lastOrNull()) else spaced
        if (result.isNotEmpty() && target.commit(result)) {
            continuation.record(if (signature) result else null)
            if (!secure) offerUndo(Dictated(result, target.after(16).orEmpty()))
        }
    }

    /** Whether the strip's Undo tool can revert the last autocorrection: it is still right before the
     * cursor with nothing selected. */
    val canUndoAutocorrection get() = undo?.let {
        !secure && target.connected && target.selected()?.isNotEmpty() != true && endsWith(it.replacement + " ")
    } == true

    /** The strip's Undo tool: the last autocorrection goes back to exactly what was typed, as Delete
     * right after it does, and what was typed shows as the literal chip. */
    fun undoAutocorrection() {
        if (!canUndoAutocorrection) { surface?.toolsChanged(); return }
        edited(); clearSpaces()
        val last = undo ?: return
        undo = null
        if (!revert(last)) { surface?.toolsChanged(); return }
        surface?.suggestions(listOf(Suggestion(last.original.substringAfterLast(' '), Suggestion.Kind.Typed)))
    }
    /** Puts back what an autocorrection replaced, with its space, if it is still right before the cursor. */
    private fun revert(last: Undo): Boolean {
        if (!endsWith(last.replacement + " ") || !replace(last.replacement + " ", last.original)) return false
        // A no-learning field still takes the undo; only the rejection is not remembered for next time.
        if (learnsTyping) feedback.undo(last.language, last.original, last.replacement)
        val typed = last.original.substringAfterLast(' ')
        prefix = before().dropLast(typed.length).takeLast(40); word?.restored(typed, prefix)
        if (expectedCaret >= 0) expectedCaret += last.original.length - last.replacement.length - 1
        return true
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
    private fun edited() { if (dictated != null) offerUndo(null); continuation.clear(); forgetSwipe(); retries.forget() }

    // MARK: Swipe typing (Keyboard › Swipe typing; SwipeTyping.kt and KeyPlaneView)

    /** What a swipe here is read with: the layout's reader and the word before. */
    class SwipeContext(val reader: SwipeReader, val previous: String?)

    /** Whether a letter touch may become a swipe now: the setting on, on the letters page of a word field
     * (not a password, email address or number, where Hex reads no words), with its typing model loaded and not
     * English's list standing in for a language without one. */
    val swipes get() = feedback.swipe && !secure && word?.standIn == false && !symbols && pad == null && target.connected

    /** A letter touch became a swipe: its key no longer types. Null when it can't be one here. */
    fun beginSwipe(): SwipeContext? {
        val w = word ?: return null
        if (!swipes) return null
        // A word delete took back stays remembered for this swipe; anything else ends the Undo dictation offer.
        if (dictated != null) offerUndo(null)
        continuation.clear(); cancelChip(); forgetSwipe()
        val reader = w.decoder.swipe.also { it.prepare(feedback.suggestSwearWords) }
        swiping = true
        surface?.suggestions(emptyList())
        val b = before()
        return SwipeContext(reader, TypingWord.previousWord(if (SwipeText.needsSpace(b)) "$b " else b).takeIf { it.isNotEmpty() })
    }

    /** The swipe under way reads as [readings] so far, best first: the strip shows the first, as it would go in. */
    fun swipePreview(readings: List<String>) {
        if (!swiping) return
        val b = before()
        val first = retries.reordered(readings, b).firstOrNull() ?: return
        surface?.suggestions(listOf(Suggestion(smartWord(swipeCased(first, swipeCapital(b, SwipeText.needsSpace(b)))),
            Suggestion.Kind.Word)))
    }

    /** The swipe ended: its best reading goes in (none when [readings] is empty, as for a cancelled swipe), after a
     * space unless the text before ends in one or in opening punctuation, with a capital from shift or the sentence,
     * and the strip offers the next three. The next tapped letter goes in after a space; a closing mark against it. */
    fun endSwipe(readings: List<String>) {
        if (!swiping) return
        swiping = false
        val w = word
        if (!target.connected || w == null || readings.isEmpty()) { surface?.suggestions(emptyList()); return }
        val b = before()
        val ordered = retries.typing(readings, b)
        val space = SwipeText.needsSpace(b)
        val capital = swipeCapital(b, space)
        val text = smartWord(swipeCased(ordered.first(), capital))
        val alternatives = swipeAlternatives(w, ordered, capital).map(::smartWord)
        clearSpaces(); resetTyping()
        if (!target.commit(if (space) " $text" else text)) return
        swiped = Swiped(text, alternatives); takenBack = null
        swipeAnchor = before()
        surface?.suggestions(alternatives.map { Suggestion(it, Suggestion.Kind.Word) })
        inputDone()
    }

    /** A swipe that read as nothing, was cancelled, or was a tap that slid: nothing goes in. */
    fun cancelSwipe() {
        if (!swiping) return
        swiping = false
        surface?.suggestions(emptyList())
    }

    /** Whether a swiped word starts with a capital: shift (one-shot, held, caps lock, or the field's own capital at
     * the cursor), or the capital the space the swipe adds brings to a sentence ("Hi." then a swipe types " There"). */
    private fun swipeCapital(before: String, space: Boolean): Boolean {
        if (shift.uppercase) return true
        if (!space || !feedback.autoCapitalize) return false
        val mask = (editor?.inputType ?: 0) and
            (InputType.TYPE_TEXT_FLAG_CAP_CHARACTERS or InputType.TYPE_TEXT_FLAG_CAP_WORDS or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES)
        if (mask == 0) return false
        return mask != InputType.TYPE_TEXT_FLAG_CAP_SENTENCES || TypingWord.startsSentence("$before ")
    }

    /** A reading as it goes in: all capitals with caps lock, a capital first letter with [capital], else as the word
     * list writes it mid-sentence (names and German nouns with their capital, "I", your own spellings). */
    private fun swipeCased(reading: String, capital: Boolean): String {
        if (shift.locked) return reading.uppercase()
        val shown = word?.swiped(reading, TypingWord.previousWord(before()).takeIf { it.isNotEmpty() }) ?: reading
        return if (capital) shown.replaceFirstChar { it.uppercase() } else shown
    }

    /** The strip's other readings after a swipe, cased as its word is; a reading that is one of your shortcuts shows
     * as its phrase (the swiped word's first), which space never puts in. */
    private fun swipeAlternatives(w: TypingWord, readings: List<String>, capital: Boolean): List<String> {
        val out = ArrayList<String>(3)
        fun phrase(reading: String) = w.decoder.personal?.shortcut(if (capital) reading.replaceFirstChar { it.uppercase() } else reading)
        phrase(readings.first())?.let(out::add)
        for (r in readings.drop(1)) {
            val item = phrase(r) ?: swipeCased(r, capital)
            if (out.none { it == item }) out.add(item)
            if (out.size == 3) break
        }
        return out
    }

    /** The strip tapped right after a swipe: another reading takes the swiped word's place, or a word delete took
     * back goes in again. False when [text] isn't one of them. */
    private fun pickSwiped(text: String): Boolean {
        val back = takenBack
        if (back != null && back.first == text) {
            if (before() != back.second || !target.commit(text)) { forgetSwipe(); return false }
            takenBack = null
            swiped = Swiped(text, emptyList()); swipeAnchor = before()
            surface?.suggestions(emptyList()); inputDone(); return true
        }
        val s = swiped ?: return false
        if (text !in s.alternatives) return false
        if (swipeAnchor != before() || !replace(s.word, text)) { forgetSwipe(); return false }
        val alternatives = (listOf(s.word) + s.alternatives.filter { it != text }).take(3)
        swiped = Swiped(text, alternatives); swipeAnchor = before()
        surface?.suggestions(alternatives.map { Suggestion(it, Suggestion.Kind.Word) })
        return true
    }

    /** Right after a swipe, delete takes back its whole word, as Gboard's does: the space before it stays, the strip
     * offers the word again, and the next swipe there types its next reading. */
    private fun deleteSwiped(): Boolean {
        val s = swiped ?: return false
        val b = before()
        if (swipeAnchor != b || !b.endsWith(s.word) || !replace(s.word, "")) return false
        if (dictated != null) offerUndo(null)
        continuation.clear(); clearSpaces(); resetTyping()
        val left = before()
        swiped = null
        retries.tookBack(s.word, left)
        takenBack = s.word to left
        surface?.suggestions(listOf(Suggestion(s.word, Suggestion.Kind.Word)))
        refreshCaps()
        return true
    }
}
