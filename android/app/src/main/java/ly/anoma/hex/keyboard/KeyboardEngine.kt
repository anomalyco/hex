package ly.anoma.hex.keyboard

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.text.InputType
import android.view.KeyEvent
import android.view.inputmethod.EditorInfo
import java.util.concurrent.Executor
import ly.anoma.hex.speech.DictationEdit
import ly.anoma.hex.speech.DictationEdits
import ly.anoma.hex.speech.DictationText
import ly.anoma.hex.speech.Spoken
import ly.anoma.hex.speech.SpokenText
import ly.anoma.hex.typing.AdaptiveTouch
import ly.anoma.hex.typing.Correction
import ly.anoma.hex.typing.KeyGrid
import ly.anoma.hex.typing.KeyPoint
import ly.anoma.hex.typing.LearnedWords
import ly.anoma.hex.typing.PendingKeep
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
    fun suggestions(slots: List<Suggestion>) = shown(Suggestion.place(slots))
    /** The strip's slots as shown, left to right ([Suggestion.SLOTS], null for an empty one). */
    fun shown(slots: List<Suggestion?>)
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
    /** A swipe that can't be read ([swipeUnreadable]) went down: the strip says why, the first time in a field. */
    private val swipeUnavailable: () -> Unit = {},
    private val clock: () -> Long = { SystemClock.uptimeMillis() },
    private val main: Handler = Handler(Looper.getMainLooper()),
    /** Labs › Learn from my fixes: what became of a tracked dictation. Null reports nothing; the tracker
     * only follows a dictation when [insertDictation] is told to learn. */
    private val learned: ((DictationEdit) -> Unit)? = null,
    /** Labs › Keyboard › Adaptive touch while it is on, else null. */
    private val adaptiveTouch: () -> AdaptiveTouch? = { null },
    /** Labs › Keyboard › Learn new words: adds a spelling (and its model's language) to Android's personal dictionary. */
    private val learnWord: ((String, String) -> Unit)? = null,
) {
    private companion object {
        /** The fix and the decoder's readings wait for this pause in typing: working them out costs a
         * search (2 ms median, 16 ms p95 per letter on the API 31 emulator), which shouldn't delay keys. */
        const val CHIP_PAUSE_MS = 100L
        /** Undo dictation stays this long unless something else happens first (iPhone Hex: 12 s). */
        const val UNDO_MS = 12_000L
    }
    private var pendingChip: Runnable? = null
    private val steady = SteadyStrip()
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
    // Labs › Keyboard › Learn new words: the spelling an undone autocorrection put back, the text before it then
    // ([before]) and right after it ([anchor]). It is learned only once that same word ends unchanged (space or a
    // closing mark); any delete, letter or other edit drops it, so holding Delete never teaches a typo.
    private val kept = PendingKeep()
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
    // With Labs › Keyboard › Punctuation after autocorrect on, the space an autocorrection or a text replacement typed
    // with its fix on space: a clause mark takes it as it takes a pick's ("keyboadr", space, "." gives "keyboard.").
    // A space typed after an unchanged word keeps it. Off, it is never set, so nothing changes.
    private var fixedAnchor: String? = null
    private var heldAnchor: String? = null
    var secure = true
        private set
    // Undo dictation, as iPhone Hex's: the dictation just inserted and the text after it then, offered
    // back until the next edit, 12 s, or the cursor leaving its end, so the chip never does nothing.
    private data class Dictated(val text: String, val after: String)
    private var dictated: Dictated? = null
    private val dropDictation = Runnable { offerUndo(null) }
    private val continuation = DictationContinuation()
    // Labs › Learn from my fixes: the dictation just inserted, followed by anchors until it settles (60 s, a new
    // dictation, or leaving the field). It holds text only in memory and reports only what it saw in place.
    private val tracker = DictationTracker(target, clock) { learned?.invoke(it) }
    private val checkDictation = object : Runnable {
        override fun run() {
            tracker.observe()
            if (tracker.isTracking) main.postDelayed(this, DictationTracker.checkInterval)
        }
    }
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
    /** The strip already said, in this field, that its swipes can't be read. */
    private var toldSwipe = false

    fun startInput(attribute: EditorInfo) {
        tracker.finish()
        edited()
        editor = attribute
        val field = FieldTyping.of(attribute.inputType, attribute.imeOptions)
        secure = field.secure
        wordField = field.words
        correctingField = field.corrects
        exactField = field.exact
        keyEventEditor = field.keyEvents
        learnsTyping = field.learns
        resetTyping(); word = null; clearSpaces(); forgetSwipe(); toldSwipe = false
        adaptive = adaptiveTouch(); adaptive?.beginSession()
        pad = Pad.of(attribute.inputType)
        symbols = pad != null
        more = false; shift.reset()
        selStart = attribute.initialSelStart; selEnd = attribute.initialSelEnd
        refreshCaps(force = true)
    }

    fun updateSelection(oldSelStart: Int, oldSelEnd: Int, newSelStart: Int, newSelEnd: Int) {
        tracker.observe()
        selStart = newSelStart; selEnd = newSelEnd
        dictated?.let { if (!undoable(it)) offerUndo(null) }
        if (oldSelStart != newSelStart || oldSelEnd != newSelEnd) {
            if (expectedCaret >= 0 && (newSelStart != expectedCaret || newSelEnd != expectedCaret)) {
                // Editors may coalesce selection notifications. Compare the actual surrounding text
                // before invalidating our own in-flight commit, not a stale intermediate offset.
                val suffix = undo?.let { it.replacement + " " } ?: word?.token?.takeIf { it.isNotEmpty() }?.let { prefix + it }
                if (newSelStart != newSelEnd || suffix == null || !endsWith(suffix) ||
                    oldSelStart == expectedCaret) { adaptive?.forget(); resetTyping(); showStrip(emptyList()) }
            }
            // The cursor left the word a swipe typed (or the place delete took it back from): its strip goes.
            val s = swiped; val back = takenBack
            if (!swiping && ((s != null && (newSelStart != newSelEnd || !endsWith(s.word))) ||
                    (back != null && (newSelStart != newSelEnd || before() != back.second)))) {
                forgetSwipe(); retries.forget(); showStrip(emptyList())
            }
            refreshCaps()
            // The strip's Undo tool follows the autocorrection still being right before the cursor.
            if (feedback.keyboardTools && undo != null) surface?.toolsChanged()
        }
    }

    /** The number row changed (Labs › Keyboard › Number row): the keys redraw with it at once. */
    fun numberRowChanged() { showPlane() }

    /** Labs › Keyboard › SwiftKey layout changed: the keys redraw with it, or without it, at once. */
    fun swiftKeyLayoutChanged() { showPlane() }

    /** The local currency changed (Labs › Keyboard › Local currency): the ?123 page and d's held symbol
     * redraw with the phone's own. */
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

    /** Labs › Keyboard › Keyboard tools changed: the strip's tools follow at once, and a field with
     * nothing before the cursor asks for its predictions again because its sentence starters depend
     * on the lab. A word being typed, a swipe's readings and a word Delete took back own the strip,
     * so they are left alone. */
    fun keyboardToolsChanged() {
        val w = word
        if (w != null && w.token.isEmpty() && !secure && !symbols && !swiping &&
            swiped == null && takenBack == null && selStart == selEnd) {
            val b = before()
            if (b.isEmpty()) showPredictions(b)
        }
        surface?.toolsChanged()
    }

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
        // A number, phone or date pad never draws a currency, so it never reads the phone's. The letters
        // page reads it for d's held symbol, as the ?123 page does for its $.
        surface?.showPage(KeyboardPage(layoutLanguage(), symbols, more, shift.uppercase, shift.locked, EnterKey.from(editor),
            FieldKind.of(editor?.inputType ?: 0), pad, feedback.numberRow,
            if (feedback.localCurrency && pad == null) LocalCurrency.local() else null, feedback.swiftKeyLayout))
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
        resetTyping(); clearSpaces(); showStrip(emptyList())
    }
    /** Back to the letters page from wherever (the emoji panel's and the symbol pad's ABC). */
    fun letters() {
        if (pad == null && !symbols) return
        adaptive?.forget(); resetTyping(); clearSpaces(); showStrip(emptyList())
        pad = null; symbols = false; more = false; shift.forgetTap(); showPlane()
    }
    /** A pad's own switches: the phone pad's symbols and back, the symbol page's 1234 key. */
    fun pad(next: Pad) { adaptive?.forget(); resetTyping(); clearSpaces(); showStrip(emptyList()); pad = next; symbols = true; more = false; showPlane() }
    /** The symbol pad's !?#: back to the symbol page. */
    fun symbolsPage() { adaptive?.forget(); resetTyping(); pad = null; symbols = true; more = false; showPlane() }
    private fun resetTyping() {
        cancelChip(); word?.reset(); undo = null; expectedCaret = -1; prefix = ""; kept.drop()
    }
    private fun clearSpaces() { spaceAnchor = null; pickedAnchor = null; fixedAnchor = null; heldAnchor = null; swipeAnchor = null }

    // Labs › Keyboard › Adaptive touch while it is on: the word reads your own key centres once it has them.
    private var adaptive: AdaptiveTouch? = null
    // It learns only where autocorrect reads words: a field that takes corrections and lets Hex learn, never a password
    // or a terminal, with Autocorrect and Spatial autocorrect on and a typing model of the layout's own.
    private val teacher get() = adaptive?.takeIf {
        correctingField && learnsTyping && !secure && !keyEventEditor && feedback.autocorrect && feedback.spatial &&
            word?.standIn == false
    }
    /** Labs › Keyboard › Adaptive touch was turned on or off: the word in progress reads with it, or without it, now. */
    fun adaptiveTouchChanged() {
        adaptive?.save(); adaptive?.forget()
        adaptive = adaptiveTouch()
        word?.adaptive = adaptive
    }
    // What a space, mark or Return tells Adaptive touch about the word it ends, [before] the text still ending in it: the
    // word as typed, or what autocorrect made of it ([fix]). Two words joined teach nothing, and neither does a word the
    // keys lost track of.
    private fun teachEnd(before: String, fix: Correction? = null) {
        val t = teacher ?: return
        val token = word?.token.orEmpty()
        if (token.isNotEmpty() && !before.endsWith(token)) return t.forget()
        if (fix == null) return t.finish(token.ifEmpty { null }, before)
        if (' ' in fix.original && ' ' !in fix.replacement) return t.forget()
        val kept = fix.replacement == token || fix.replacement.endsWith(" $token")
        t.finish(token.ifEmpty { null }, before, if (kept) null else fix.replacement, ' ' in fix.original)
    }
    private fun forgetSwipe() { swiped = null; takenBack = null }
    private fun cancelChip() { pendingChip?.let(main::removeCallbacks); pendingChip = null }
    fun configureTyping(keepWord: Boolean = false) {
        val typing = typingLanguage()
        // No model of its own: English's word list still gives the keys their geometry and your personal
        // dictionary, marked as a stand-in so none of English's words reach the strip or autocorrect. A layout
        // with keys of its own but no model yet (Русский) reads no words: its letters type as tapped.
        val layout = layoutLanguage()
        val language = typing ?: layout.layoutCode
        val standIn = typing == null
        literalLayout = typing == null && !layout.hasModel
        val active = word
        if (wordField && keepWord && active != null && active.standIn == standIn && active.decoder.model.language == language &&
            active.spatial == feedback.spatial && active.token.isNotEmpty() && endsWith(prefix + active.token)) {
            active.addsCapitals = feedback.autoCapitalize
            showPlane(); showSuggestions(active); return
        }
        resetTyping()
        word = if (wordField && !literalLayout) {
            try {
                TypingWord(models.getOrPut(language) { loadDecoder(language).also(::loadPersonal) }, feedback.spatial)
                    .also {
                        it.standIn = standIn
                        it.suggestSwearWords = feedback.suggestSwearWords
                        it.addsCapitals = feedback.autoCapitalize
                        it.adaptive = adaptive
                    }
            } catch (_: java.io.IOException) { null } // The editor can still type if its assets cannot be read.
        } else null
        showPlane()
        showSuggestions(word)
    }

    // The strip while a word is typed: completions at once, then, after a pause, the model's whole
    // strip with what space will put in. A fix you turned down twice shows as a plain word. Words keep
    // their slots from one strip to the next while the same word is typed ([SteadyStrip]).
    private fun showSuggestions(w: TypingWord?) {
        cancelChip()
        if (w != null && w.token.isNotEmpty()) {
            // A stand-in's strip already is the finished one: nothing there changes after the pause.
            // A field that asked for no suggestions of its own gets no fix: space leaves its words alone.
            // With Keyboard › Autocorrect off the near words still come, as plain words space never puts in.
            val pause = correctingField && !w.standIn && canEndWord()
            val token = w.token
            val typed = shown(Suggestion(token, Suggestion.Kind.Typed))
            showWord(token, Suggestion.withEmoji(w.strip(autocorrect = false).map(::shown), emojiFor(w)),
                if (pause) typed else null, early = pause)
            if (!pause) return
            val fixes = feedback.autocorrect
            pendingChip = Runnable {
                pendingChip = null
                if (word === w && w.token == token) showWord(token, Suggestion.withEmoji(w.strip(fixes, nearWords = true).map {
                    if (it.kind == Suggestion.Kind.Fix && feedback.blocked(w.decoder.model.language, token, it.text))
                        Suggestion(it.text, Suggestion.Kind.Word) else it
                }.map(::shown), emojiFor(w)), typed)
            }.also { main.postDelayed(it, CHIP_PAUSE_MS) }
            return
        }
        val b = if (w != null && !symbols && selStart == selEnd) before() else null
        showPredictions(if (b != null && TypingWord.boundary(b.lastOrNull())) b else null)
    }
    /** The word being typed's strip, its words kept in their slots ([SteadyStrip]). */
    private fun showWord(token: String, slots: List<Suggestion>, typed: Suggestion?, early: Boolean = false) =
        surface?.shown(steady.arrange(token, slots, typed, early))
    /** Any other strip (next words, a swipe's, nothing): the next word's strip is laid out afresh. */
    private fun showStrip(slots: List<Suggestion>) {
        steady.forget()
        surface?.suggestions(slots)
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

    /** Next words for the text before the cursor, capitalised as shift would be. An empty field with
     * Keyboard tools on leaves the strip's line to the tools instead of sentence starters. */
    private fun showPredictions(before: String?) {
        val w = word
        showStrip(if (w != null && before != null && !symbols && canEndWord() &&
            !KeyboardTools.showsInEmptyField(feedback.keyboardTools, before) { target.after(1) })
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

    /** A letter key typed [text] at (x, y) on [grid]'s keys; [explicit] when it was a held option, [aimed] false when the
     * touch slid onto the key, so where it went down says nothing of where you aim. */
    fun letter(text: String, x: Double, y: Double, grid: KeyGrid, explicit: Boolean = false, aimed: Boolean = true) {
        if (!target.connected) return
        edited(); kept.drop()
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
            if (explicit) { w.explicit(text); teacher?.untapped() }
            else { w.letter(text, x, y, grid); teacher?.typed(text.single(), x, y, grid, aimed) }
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
     * double-space), and a closing mark takes its place (in a URL field, also / and the other joiners).
     * A next word picked right after a word with no space yet ("up to‸ best") puts that space in first,
     * where the strip offers next words ([showPredictions]: nothing letter-like after the cursor, so never
     * inside a word); and a word being typed with a letter after the cursor (Gmail's Smart Compose text)
     * goes in with its space before that text rather than a tap that does nothing. */
    fun pickSuggestion(text: String) {
        val w = word ?: return
        if (pickSwiped(text)) return
        edited()
        if (secure || (symbols && w.token.isEmpty())) return
        val old = w.token
        if (old.isNotEmpty() && !endsWith(prefix + old)) { resetTyping(); showStrip(emptyList()); return }
        val lead = old.isEmpty() && canEndWord() && !w.startsWord(before()) && TypingWord.endsBareWord(before())
        // A next word goes in only where the strip offers next words: never before a letter, as inside a word.
        if (old.isEmpty() && (!canEndWord() || (!lead && !w.startsWord(before())))) return
        val holds = SpaceRules.separates(target.after(1))
        clearSpaces()
        val committed = smartWord(text)
        // Adaptive touch: a pick of the same length teaches the letters it changed, and tapping what was typed over the
        // fix space would make teaches all of them.
        teacher?.let { t ->
            if (old.isNotEmpty()) t.picked(old, text, overCorrection = text == old && feedback.autocorrect && correctingField &&
                w.replacement() != null, before = before())
        }
        // Labs › Keyboard › Learn new words: what was typed, tapped as typed, is a spelling you keep.
        val keptBefore = if (old.isNotEmpty() && sameSpelling(text, old)) before().dropLast(old.length) else null
        if (replace(old, (if (lead) " " else "") + (if (holds) committed else "$committed "))) {
            if (keptBefore != null) learn(w, old, keptBefore)
            resetTyping()
            val b = before()
            if (holds) heldAnchor = b else pickedAnchor = b
            showPredictions(b)
            inputDone()
        }
    }

    private fun sameSpelling(a: String, b: String) = a.replace('\u2019', '\'') == b.replace('\u2019', '\'')

    /** Labs › Keyboard › Learn new words: the word an undone autocorrection put back ends unchanged here, [b] the text
     * before the cursor as it ends: it is a spelling you kept. */
    private fun learnKept(b: String) {
        val w = word
        val k = kept.ended(b, w?.token, w?.decoder?.model?.language) ?: return
        learn(w ?: return, k.word, k.before)
    }

    /** Labs › Keyboard › Learn new words: [typed], kept as typed after [before], joins the personal dictionary when no
     * dictionary knows it. Only in a field Hex may learn from that reads words with a model of its own: never a
     * password, URL or incognito field, nor a stand-in's keys. */
    private fun learn(w: TypingWord, typed: String, before: String) {
        val add = learnWord ?: return
        if (!LearnedWords.allowed(feedback.learnWords, secure, exactField, keyEventEditor, learnsTyping, w.standIn)) return
        val spelling = LearnedWords.candidate(typed, before, { w.decoder.model.code(it) != 0 }, w::knows,
            blocked = { w.decoder.model.neverSuggest.blocks(it, false, -1) }) ?: return
        add(spelling, w.decoder.model.language)
    }

    /** A word a pick, autocorrection or swipe commits: with Labs › Keyboard › Smart quotes on, its
     * straight apostrophes go in as ’ ("don't" becomes "don’t"). Off, it is exactly the text offered. */
    private fun smartWord(text: String) = if (smartQuotesHere()) SmartQuotes.word(text) else text

    /** Whether Labs › Keyboard › Smart quotes applies at the cursor: its toggle on, in a plain text
     * field, and not in a password, email, URL or TYPE_NULL field (a terminal), whose text must keep
     * the straight quotes it has always taken. */
    private fun smartQuotesHere(): Boolean = feedback.smartQuotes && plainTextField()

    /** Labs › Keyboard › Closing quotes take the space (#3001): a " or ” typed while a quote is open in the
     * paragraph replaces the one space before the cursor, whoever typed it ("see you soon " then " gives
     * "see you soon"), going in as ” with Smart quotes. It reads only the text, so the space survives the
     * switch to ?123. Off, or outside a plain text field, the quote goes in as before. */
    private fun closeQuote(mark: Char): Boolean {
        if (!feedback.closingQuoteTakesSpace || exactField || !plainTextField() || !before().endsWith(" ")) return false
        if (!SpaceRules.closingQuoteTakesSpace(target.before(SpaceRules.QUOTE_LOOKBACK).orEmpty())) return false
        edited()
        val placed = if (mark == '"' && smartQuotesHere()) SmartQuotes.CLOSING_DOUBLE else mark
        clearSpaces()
        if (!replace(" ", placed.toString())) return false
        resetTyping(); showPredictions(before()); inputDone()
        return true
    }

    /** A plain text field: not a password, email, URL or TYPE_NULL field (a terminal). */
    private fun plainTextField(): Boolean {
        if (secure) return false
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
            // The text after the cursor already separates the picked word: the first space keeps the held
            // space for the next word, and a second one goes in.
            heldAnchor != null && heldAnchor == b && spaceAnchor != b -> {
                spaceAnchor = b; spaceAt = now; showPredictions(b); return
            }
            heldAnchor != null && heldAnchor == b -> {
                clearSpaces(); resetTyping()
                if (target.commit(" ")) afterSpace(now)
                return
            }
            pickedAnchor != null && pickedAnchor == b -> { clearSpaces(); spaceAnchor = b; spaceAt = now; showPredictions(b); return }
            (word != null || (wordField && literalLayout)) && !exactField && spaceAnchor == b && now - spaceAt <= SpaceRules.DOUBLE_SPACE_MS && SpaceRules.periodFits(b) -> {
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
            if (swap != null && replace(swap.original, smartWord(swap.replacement) + " ")) {
                teachEnd(b, swap); resetTyping(); afterSpace(now); if (feedback.marksTakeFixSpace) fixedAnchor = spaceAnchor; return
            }
            if (feedback.autocorrect && correctingField && old.isNotEmpty() && endsWith(prefix + old)) {
                val fix = w.finish(' ', point, grid, ::smartWord)
                if (fix != null && !feedback.blocked(lang, fix.original, fix.replacement) &&
                    replace(fix.original, fix.replacement + " ")) {
                    cancelChip()
                    undo = Undo(lang, fix.original, fix.replacement)
                    if (expectedCaret >= 0) expectedCaret += fix.replacement.length + 1 - fix.original.length
                    teachEnd(b, fix); w.reset(); afterSpace(now); if (feedback.marksTakeFixSpace) fixedAnchor = spaceAnchor; return
                }
            }
        }
        teachEnd(b); learnKept(b); target.commit(" "); resetTyping(); afterSpace(now)
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
        if (value == '"' && closeQuote(value)) return
        val placed = if (smartQuotesHere()) SmartQuotes.typed(value, before().lastOrNull()) else value
        type(placed.toString())
    }

    fun type(value: String) {
        if (!target.connected) return
        edited()
        if (value == " ") { space(); return }
        if (value == SmartQuotes.CLOSING_DOUBLE.toString() && closeQuote(SmartQuotes.CLOSING_DOUBLE)) return
        val b = before()
        val mark = value.length == 1 && TypingWord.endsWord(value[0])
        // A clause-ending mark takes the space a picked suggestion, an autocorrection or a text replacement typed
        // ("Hello,", or "Hello, " with Space after punctuation); in a URL field, so does any mark that joins a word
        // ("github." and "github/", never "github /").
        val typedSpace = (pickedAnchor != null && pickedAnchor == b) || (fixedAnchor != null && fixedAnchor == b)
        if (value.length == 1 && SpaceRules.takesPickedSpace(value[0], exactField) && typedSpace && b.endsWith(" ")) {
            clearSpaces()
            // Labs › Keyboard › Space after punctuation: the space goes after the mark instead ("here, "), and a space
            // typed next is swallowed as one after a pick is.
            val moves = feedback.marksKeepSpace && SpaceRules.movesPickedSpace(value[0], exactField)
            if (replace(" ", if (moves) "$value " else value)) {
                resetTyping(); val after = before(); if (moves) pickedAnchor = after; showPredictions(after); inputDone(); return
            }
        }
        // Anything that starts another word puts a held space in first; marks go against the word.
        val afterSwipe = swipeAnchor != null && swipeAnchor == b
        val text = if (value.any { it in SpaceRules.JOINERS || it.isWhitespace() }) b else takeHeldSpace(b)
        clearSpaces()
        val w = word
        if (mark && w != null && pad == null && !exactField && canEndWord() && !afterSwipe) {
            val swap = w.textReplacement(text)
            if (swap != null && replace(swap.original, smartWord(swap.replacement) + value)) {
                teachEnd(text, swap); resetTyping(); showPredictions(before()); inputDone(); return
            }
            if (feedback.autocorrect && correctingField && w.token.isNotEmpty() && endsWith(prefix + w.token)) {
                val fix = w.finish(value[0], commit = ::smartWord)
                if (fix != null && !feedback.blocked(w.decoder.model.language, fix.original, fix.replacement) &&
                    replace(fix.original, fix.replacement + value)) {
                    teachEnd(text, fix); resetTyping(); showPredictions(before()); inputDone(); return
                }
            }
        }
        // A mark, digit or Return ends the word as typed; a letter outside a word, or an emoji, is nothing to follow.
        if (value.length == 1 && !value[0].isLetter()) teachEnd(text) else adaptive?.forget()
        if (mark) learnKept(b)
        resetTyping(); target.commit(value); showPredictions(if (w != null) before() else null)
        inputDone()
    }
    fun delete() {
        if (!target.connected) return
        kept.drop()
        // Hex's own Delete is the person removing text, so a field that then empties was cleared by them.
        tracker.keyboardRemoved()
        // A TYPE_NULL field's text can't be read back, and its editor takes no surrounding-text delete: a
        // delete is the key press itself, one per call, and a held key repeats the call as Gboard's does.
        if (keyEventEditor) {
            edited(); clearSpaces(); resetTyping(); adaptive?.forget(); showStrip(emptyList())
            target.sendKey(KeyEvent.KEYCODE_DEL)
            return
        }
        if (deleteSwiped()) return
        edited()
        // Right after a pick held its space, delete takes that space back and deletes nothing.
        if (heldAnchor != null && heldAnchor == before()) { clearSpaces(); showStrip(emptyList()); return }
        clearSpaces()
        val last = undo
        undo = null
        if (last != null && revert(last)) {
            showStrip(listOf(Suggestion(last.original.substringAfterLast(' '), Suggestion.Kind.Typed))); return
        }
        val w = word
        val track = w != null && w.token.isNotEmpty() && endsWith(prefix + w.token)
        val selected = (selStart >= 0 && selEnd >= 0 && selStart != selEnd) ||
            (!secure && target.selected()?.isNotEmpty() == true)
        if (selected) { adaptive?.forget(); target.commit(""); resetTyping() }
        else {
            // Adaptive touch: a letter erased and retyped at the same place teaches where the first tap meant to go.
            teacher?.willDelete(before())
            // A whole character as people see it: 👍🏽, a flag or a family is one, as on Gboard.
            val cluster = if (track || secure) 1 else DeleteSlide.lastCharacter(target.before(32).orEmpty())
            // Drop the letter before the editor does: an editor in Hex's own process reports the new
            // selection during the delete call, and that report must find the word already shorter.
            if (track) { w!!.pop(); if (expectedCaret >= 0) expectedCaret-- }
            if (!(if (cluster > 1) target.deleteBefore(cluster) else target.deleteCodePointsBefore(1))) target.sendKey(KeyEvent.KEYCODE_DEL)
            if (!track) resetTyping()
        }
        if (w != null && w.token.isNotEmpty()) showSuggestions(w) else showStrip(emptyList())
    }
    /** Text went in through the editor's own paste (the strip's Paste tool): the word in progress is not
     * what is before the cursor any more. */
    fun pasted() { edited(); adaptive?.forget(); resetTyping(); clearSpaces(); showStrip(emptyList()) }

    fun enter() {
        edited(); adaptive?.forget()
        // A text replacement that ends the text goes in before the line break or the editor's action.
        word?.takeIf { canEndWord() }?.textReplacement(before())?.let { replace(it.original, smartWord(it.replacement)) }
        resetTyping(); clearSpaces(); showStrip(emptyList())
        val info = editor
        // Before any action or key press: a Return that ends a word teaches adaptive touch as a typed one does.
        val b = if (keyEventEditor) "" else before()
        // Only a multi-line field takes "\n" as text; a one-line field without an action gets the Enter key (#2864).
        when (EnterPress.run(target, info?.inputType ?: (InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE),
            info?.imeOptions ?: 0, info?.actionLabel, info?.actionId ?: 0)) {
            EnterPress.Action -> tracker.sent()
            EnterPress.LineBreak -> type("\n")
            // The key press, with the bookkeeping type("\n") does: the word ends, next words follow.
            EnterPress.Key -> if (!keyEventEditor) {
                teachEnd(b); showPredictions(if (word != null) before() else null); inputDone()
            }
        }
    }

    /** A space-bar slide starts moving the cursor: the word in progress stays as typed. A field whose text
     * isn't there to read, a password or a TYPE_NULL editor, steps with arrow keys instead. */
    fun cursorSlide(): CursorSlide {
        adaptive?.forget(); resetTyping(); clearSpaces(); showStrip(emptyList())
        return CursorSlide(target, minOf(selStart, selEnd), maxOf(selStart, selEnd),
            readable = !secure && !keyEventEditor)
    }

    /** A slide left from delete: null where it can't run (a password or TYPE_NULL field, a selection, no known
     * cursor). */
    fun deleteSlide(): DeleteSlide? {
        if (secure || keyEventEditor || selStart < 0 || selStart != selEnd || !target.connected) return null
        adaptive?.forget(); resetTyping(); clearSpaces(); showStrip(emptyList())
        return DeleteSlide(target, selStart, target.before(2000).orEmpty())
    }
    /** Labs › Keyboard › Delete words when held: a held Delete takes the word before the cursor (what a delete slide
     * would select first). Returns the UTF-16 length removed, 0 with nothing before the cursor, or -1 where words
     * can't be read (the lab off, a password or TYPE_NULL field, a selection, no known cursor), so the held key
     * repeats letters as before. */
    fun deleteWord(): Int {
        if (!feedback.deleteWords || secure || keyEventEditor || selStart < 0 || selStart != selEnd || !target.connected) return -1
        val before = target.before(2000) ?: return -1
        val length = DeleteSlide.wordLengths(before).firstOrNull() ?: return 0
        tracker.keyboardRemoved()
        edited(); adaptive?.forget(); resetTyping(); clearSpaces(); showStrip(emptyList())
        return if (target.deleteBefore(length)) length else -1
    }
    /** The delete slide lifted with words selected: they go. */
    fun deleteSelection() {
        if (!target.connected) return
        tracker.keyboardRemoved()
        edited(); adaptive?.forget()
        target.commit(""); resetTyping(); showStrip(emptyList())
    }

    /** Dictation starts: the word in progress is left as typed. The signed dictation before still makes this
     * one stand alone, so its continuation is kept. The dictation before is judged now, before this one's text
     * reaches the field. */
    fun clearForDictation() {
        tracker.settle(); scheduleDictationCheck()
        offerUndo(null); adaptive?.forget(); resetTyping(); clearSpaces(); showStrip(emptyList())
    }

    /** The editor or the keyboard went away: forget the word, never carry it to the next field. */
    fun leave() {
        tracker.finish(); main.removeCallbacks(checkDictation)
        adaptive?.forget(); adaptive?.save()
        edited(); resetTyping(); clearSpaces(); word = null; showStrip(emptyList())
    }

    /** Dictated text with only the separators missing at the cursor; mid-sentence its first word loses
     * the capital Whisper gave it ([SpokenText.continuing], [language] as dictated or "en" translated).
     * [signature] says a signature was appended (Labs › Signature): it joins the text at the cursor as
     * usual, with the signature on a line of its own. The next dictation starts on a new line below that
     * signature while the signed text is still right before the cursor, so it is never lowercased or
     * spaced onto the signature line. */
    fun insertDictation(text: String, language: String?, signature: Boolean = false, heard: String? = null,
                        learn: Boolean = false) {
        if (!target.connected) return
        val before = if (secure) null else target.before(maxOf(64, continuation.reach))
        // The lab's anchors; off, the single character spacing has always read.
        val after = target.after(if (learn) DictationTracker.anchorLength else 1)
        val standalone = continuation.standalone(before)
        val joinable = !signature && !standalone
        val shaped = if (!joinable) text else {
            // The keyboard's own word list for that language, when loaded, knows far more ordinary words than
            // the speech side's short list: a word it has in lowercase and the books rarely capitalise
            // loses Whisper's capital; a name, or a word with a spelling of its own ("ChatGPT"), keeps it (iOS asks
            // UITextChecker the same).
            val model = models[if (language == "no") "nb" else language]?.model
            if (model == null) SpokenText.continuing(text, before, language)
            else SpokenText.continuing(text, before, language) {
                model.index(it) >= 0 && (model.capitals?.percent(it) ?: -1) < 50 && model.spellings?.of(it) == null
            }
        }
        // Spacing is decided on the straight quotes, as it always was, so a dictation's spacing is
        // unchanged: the same smart quotes as the keys and the strip then go in with their spacing.
        val spaced = dictationInsertion(shaped, before, after?.firstOrNull(), standalone)
        val result = if (smartQuotesHere()) SmartQuotes.text(spaced, before?.lastOrNull()) else spaced
        // The dictation before is judged against the field as it is now, never the one this call is about to add.
        tracker.settle()
        if (result.isNotEmpty() && target.commit(result)) {
            adaptive?.forget()
            continuation.record(if (signature) result else null)
            if (!secure) offerUndo(Dictated(result, target.after(16).orEmpty()))
            val words = DictationText.words(result).size
            // A signed dictation carries a line Hex added, not just what was said: it is never tracked.
            if (learn && !signature && heard != null && !secure && wordField && !keyEventEditor && learnsTyping &&
                words in 1..DictationEdits.maxWords) tracker.inserted(heard, result, before, after)
            scheduleDictationCheck()
        }
    }

    /** Keeps the tracker's clock while it follows a dictation or holds one for a redictation. */
    private fun scheduleDictationCheck() {
        main.removeCallbacks(checkDictation)
        if (tracker.isTracking) main.postDelayed(checkDictation, DictationTracker.checkInterval)
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
        showStrip(listOf(Suggestion(last.original.substringAfterLast(' '), Suggestion.Kind.Typed)))
    }
    /** Puts back what an autocorrection replaced, with its space, if it is still right before the cursor. */
    private fun revert(last: Undo): Boolean {
        if (!endsWith(last.replacement + " ") || !replace(last.replacement + " ", last.original)) return false
        // A no-learning field still takes the undo; only the rejection is not remembered for next time.
        if (learnsTyping) feedback.undo(last.language, last.original, last.replacement)
        // Adaptive touch: the word kept as typed over the fix teaches where its taps meant to go.
        teacher?.undone(last.original)
        val typed = last.original.substringAfterLast(' ')
        prefix = before().dropLast(typed.length).takeLast(40); word?.restored(typed, prefix)
        // Labs › Keyboard › Learn new words: kept only once the word ends unchanged ([learnKept]); one of two words a
        // missed space joined is not a word the person kept.
        kept.undone(last.original, typed, last.language, prefix, before())
        if (expectedCaret >= 0) expectedCaret += last.original.length - last.replacement.length - 1
        return true
    }

    /** Undo dictation: the last dictation goes, if it is still right before the cursor with nothing selected. */
    fun undoDictation() {
        val d = dictated ?: return
        offerUndo(null)
        if (undoable(d) && replace(d.text, "")) {
            tracker.undone()
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
    /** Where [swipes] would be but English's list only stands in for the dictation language's words, or the layout has
     * no model yet: a swipe types nothing, as before, and the strip says so ([swipeUnavailable]). */
    val swipeUnreadable get() = feedback.swipe && !secure && (word?.standIn == true || (wordField && literalLayout)) &&
        !symbols && pad == null && target.connected
    /** The layout has keys but no typing model yet (Русский): no word is read, so taps type as tapped. */
    private var literalLayout = false

    /** A letter touch became a swipe: its key no longer types. Null when it can't be one here. */
    fun beginSwipe(): SwipeContext? {
        if (!swipes) {
            if (swipeUnreadable && !toldSwipe) { toldSwipe = true; swipeUnavailable() }
            return null
        }
        val w = word ?: return null
        // A word delete took back stays remembered for this swipe; anything else ends the Undo dictation offer.
        if (dictated != null) offerUndo(null)
        continuation.clear(); cancelChip(); forgetSwipe(); adaptive?.forget()
        val reader = w.decoder.swipe.also { it.prepare(feedback.suggestSwearWords) }
        swiping = true
        showStrip(emptyList())
        val b = before()
        return SwipeContext(reader, TypingWord.previousWord(if (SwipeText.needsSpace(b)) "$b " else b).takeIf { it.isNotEmpty() })
    }

    /** The swipe under way reads as [readings] so far, best first: the strip shows the first, as it would go in. */
    fun swipePreview(readings: List<String>) {
        if (!swiping) return
        val b = before()
        val first = retries.reordered(readings, b).firstOrNull() ?: return
        showStrip(listOf(Suggestion(smartWord(swipeCased(first, swipeCapital(b, SwipeText.needsSpace(b)))),
            Suggestion.Kind.Word)))
    }

    /** The swipe ended: its best reading goes in (none when [readings] is empty, as for a cancelled swipe), after a
     * space unless the text before ends in one or in opening punctuation, with a capital from shift or the sentence,
     * and the strip offers the next three. The next tapped letter goes in after a space; a closing mark against it. */
    fun endSwipe(readings: List<String>) {
        if (!swiping) return
        swiping = false
        val w = word
        if (!target.connected || w == null || readings.isEmpty()) { showStrip(emptyList()); return }
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
        showStrip(alternatives.map { Suggestion(it, Suggestion.Kind.Word) })
        inputDone()
    }

    /** A swipe that read as nothing, was cancelled, or was a tap that slid: nothing goes in. */
    fun cancelSwipe() {
        if (!swiping) return
        swiping = false
        showStrip(emptyList())
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
            showStrip(emptyList()); inputDone(); return true
        }
        val s = swiped ?: return false
        if (text !in s.alternatives) return false
        if (swipeAnchor != before() || !replace(s.word, text)) { forgetSwipe(); return false }
        val alternatives = (listOf(s.word) + s.alternatives.filter { it != text }).take(3)
        swiped = Swiped(text, alternatives); swipeAnchor = before()
        showStrip(alternatives.map { Suggestion(it, Suggestion.Kind.Word) })
        return true
    }

    /** Right after a swipe, delete takes back its whole word, as Gboard's does: the space before it stays, the strip
     * offers the word again, and the next swipe there types its next reading. */
    private fun deleteSwiped(): Boolean {
        val s = swiped ?: return false
        val b = before()
        if (!SwipeText.takesBack(b, swipeAnchor, s.word, feedback.swipeDeleteLetters) || !replace(s.word, "")) return false
        adaptive?.forget()
        if (dictated != null) offerUndo(null)
        continuation.clear(); clearSpaces(); resetTyping()
        val left = before()
        swiped = null
        retries.tookBack(s.word, left)
        takenBack = s.word to left
        showStrip(listOf(Suggestion(s.word, Suggestion.Kind.Word)))
        refreshCaps()
        return true
    }
}
