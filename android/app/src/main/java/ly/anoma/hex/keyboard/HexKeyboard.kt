package ly.anoma.hex.keyboard

import android.Manifest
import android.content.SharedPreferences
import android.content.pm.PackageManager
import android.inputmethodservice.InputMethodService
import android.os.SystemClock
import android.text.InputType
import android.view.KeyEvent
import android.view.View
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import ly.anoma.hex.typing.KeyGrid
import ly.anoma.hex.typing.SpatialDecoder
import ly.anoma.hex.typing.TypingModel
import ly.anoma.hex.typing.TypingPreferences
import ly.anoma.hex.typing.TypingWord
import ly.anoma.hex.R
import ly.anoma.hex.speech.LocalSpeech
import ly.anoma.hex.speech.SpeechPreferences
import ly.anoma.hex.speech.MicCapture
import ly.anoma.hex.speech.SpeechPhase

class HexKeyboard : InputMethodService() {
    private var keys: KeyPlane? = null
    private lateinit var capture: MicCapture
    internal val preferences by lazy { SpeechPreferences(this) }
    internal var nextEnglish: Boolean? = null
    internal val feedback by lazy { TypingPreferences(this) }
    internal val layoutLanguage get() = feedback.layout ?: preferences.spoken
    private val models = HashMap<String, SpatialDecoder>()
    private var word: TypingWord? = null
    private val typingChanged = SharedPreferences.OnSharedPreferenceChangeListener { _, key ->
        if (key == TypingPreferences.SUGGEST_SWEAR_WORDS) word?.let {
            it.suggestSwearWords = feedback.suggestSwearWords
            showSuggestions(it)
        }
    }
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
    private var secure = true
    internal val previewAllowed get() = !secure

    override fun onCreate() {
        setTheme(R.style.HexIme)
        super.onCreate()
        getSharedPreferences("typing", MODE_PRIVATE).registerOnSharedPreferenceChangeListener(typingChanged)
        capture = MicCapture(LocalSpeech(applicationContext),
            update = { phase, note -> keys?.status(phase, note ?: idleHint()) },
            meter = { keys?.setLevel(it) }, insert = ::commitSpeech)
    }
    override fun onEvaluateFullscreenMode() = false
    override fun onCreateInputView(): View = KeyPlane(this, this).also { keys = it }

    override fun onStartInput(attribute: EditorInfo, restarting: Boolean) {
        super.onStartInput(attribute, restarting)
        nextEnglish = null
        capture.cancel() // Even a different field in the same app must not receive an old result.
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
    override fun onStartInputView(info: EditorInfo, restarting: Boolean) {
        super.onStartInputView(info, restarting)
        keys?.cancelGestures()
        configureTyping()
        keys?.status(capture.phase, idleHint())
    }
    override fun onUpdateSelection(oldSelStart: Int, oldSelEnd: Int, newSelStart: Int, newSelEnd: Int,
                                   candidatesStart: Int, candidatesEnd: Int) {
        super.onUpdateSelection(oldSelStart, oldSelEnd, newSelStart, newSelEnd, candidatesStart, candidatesEnd)
        selStart = newSelStart; selEnd = newSelEnd
        if (oldSelStart != newSelStart || oldSelEnd != newSelEnd) {
            if (expectedCaret >= 0 && (newSelStart != expectedCaret || newSelEnd != expectedCaret)) {
                // Editors may coalesce selection notifications. Compare the actual surrounding text
                // before invalidating our own in-flight commit, not a stale intermediate offset.
                val suffix = undo?.let { it.prefix + it.replacement + " " } ?: word?.token?.takeIf { it.isNotEmpty() }?.let { prefix + it }
                if (newSelStart != newSelEnd || suffix == null || !endsWith(suffix) ||
                    oldSelStart == expectedCaret) { resetTyping(); keys?.suggestions(emptyList()) }
            }
            refreshCaps()
        }
    }

    private fun refreshCaps() {
        val mask = (currentInputEditorInfo?.inputType ?: 0) and
            (InputType.TYPE_TEXT_FLAG_CAP_CHARACTERS or InputType.TYPE_TEXT_FLAG_CAP_WORDS or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES)
        val next = locked || (!secure && mask != 0 && (currentInputConnection?.getCursorCapsMode(mask) ?: 0) != 0)
        if (shifted != next) { shifted = next; showPlane() }
    }
    private fun showPlane() {
        val info = currentInputEditorInfo
        val action = info?.imeOptions ?: 0
        val actionLabel = if ((action and EditorInfo.IME_FLAG_NO_ENTER_ACTION) == 0) {
            info?.actionLabel?.toString() ?: when (action and EditorInfo.IME_MASK_ACTION) {
                EditorInfo.IME_ACTION_DONE -> "Done"
                EditorInfo.IME_ACTION_GO -> "Go"
                EditorInfo.IME_ACTION_NEXT -> "Next"
                EditorInfo.IME_ACTION_SEARCH -> "Search"
                EditorInfo.IME_ACTION_SEND -> "Send"
                else -> null
            }
        } else null
        keys?.layoutKeys(symbols, more, shifted, locked, actionLabel)
    }
    internal fun shift() {
        if (symbols) { more = !more; showPlane(); return }
        val now = SystemClock.uptimeMillis()
        when {
            locked -> { locked = false; shifted = false }
            lastShift != 0L && now - lastShift < 350 -> { locked = true; shifted = true }
            else -> shifted = !shifted
        }
        lastShift = now; showPlane()
    }
    internal fun mode() { resetTyping(); keys?.suggestions(emptyList()); symbols = !symbols; more = false; lastShift = 0; showPlane() }
    private fun resetTyping() {
        word?.reset(); undo = null; expectedCaret = -1; prefix = ""
    }
    internal fun configureTyping(keepWord: Boolean = false) {
        val language = layoutLanguage.layoutCode
        val active = word
        if (ordinaryField && keepWord && active != null && active.decoder.model.language == language && active.spatial == feedback.spatial &&
            active.token.isNotEmpty() && endsWith(prefix + active.token)) {
            showPlane(); showSuggestions(active); return
        }
        resetTyping()
        word = if (ordinaryField) {
            try {
                TypingWord(models.getOrPut(language) { SpatialDecoder(TypingModel.asset(assets, language)) }, feedback.spatial)
                    .also { it.suggestSwearWords = feedback.suggestSwearWords }
            } catch (_: java.io.IOException) { null } // The editor can still type if its assets cannot be read.
        } else null
        showPlane()
        showSuggestions(word)
    }
    private fun showSuggestions(w: TypingWord?) {
        if (w != null && w.token.isNotEmpty()) {
            val choices = w.suggestions()
            keys?.suggestions(choices, literal = w.literalChip(choices))
            return
        }
        val b = if (w != null && !symbols && selStart == selEnd) before() else null
        keys?.suggestions(if (w != null && b != null && TypingWord.boundary(b.lastOrNull()) && canEndWord()) w.predictions(b) else emptyList())
    }
    private fun before() = if (secure) "" else currentInputConnection?.getTextBeforeCursor(96, 0)?.toString().orEmpty()
    private fun endsWith(suffix: String) = !secure && currentInputConnection?.getTextBeforeCursor(suffix.length, 0)?.toString() == suffix
    private fun canEndWord() = !secure && (currentInputConnection?.getTextAfterCursor(1, 0)?.firstOrNull()?.let {
        !it.isLetterOrDigit() && it !in "'_/@#"
    } ?: true)
    private fun inputDone() {
        lastShift = 0
        if (!locked && shifted) { shifted = false; showPlane() }
    }
    internal fun letter(text: String, x: Double, y: Double, grid: KeyGrid, explicit: Boolean = false) {
        val ic = currentInputConnection ?: return
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
        if (ic.commitText(text, 1)) {
            if (explicit) w.explicit(text) else w.letter(text, x, y, grid)
            showSuggestions(w)
        } else resetTyping()
        inputDone()
    }
    private fun replace(old: String, new: String): Boolean {
        if (secure) return false
        val ic = currentInputConnection ?: return false
        // With a non-empty selection deleteSurroundingText would remove text outside the selection.
        if (ic.getSelectedText(0)?.isNotEmpty() == true || (old.isNotEmpty() && !endsWith(old))) return false
        ic.beginBatchEdit()
        return try {
            (old.isEmpty() || ic.deleteSurroundingText(old.length, 0)) && ic.commitText(new, 1)
        } finally { ic.endBatchEdit() }
    }
    internal fun pickSuggestion(text: String) {
        val w = word ?: return
        if (symbols || !canEndWord()) return
        val old = w.token
        if (old.isNotEmpty() && !endsWith(prefix + old)) { resetTyping(); keys?.suggestions(emptyList()); return }
        if (old.isEmpty() && !TypingWord.boundary(before().lastOrNull())) return
        if (replace(old, text + " ")) {
            resetTyping(); keys?.suggestions(w.predictions(text)); inputDone()
        }
    }
    internal fun type(value: String) {
        val ic = currentInputConnection ?: return
        val w = word
        if (value == " " && w != null && !symbols) {
            val old = w.token; val lang = w.decoder.model.language
            val fix = if (feedback.autocorrect && old.isNotEmpty() && endsWith(prefix + old) && canEndWord()) w.replacement() else null
            if (fix != null && !feedback.blocked(lang, old, fix) && replace(old, fix + " ")) {
                undo = Undo(lang, prefix, old, fix)
                if (expectedCaret >= 0) expectedCaret += fix.length + 1 - old.length
                w.reset(); keys?.suggestions(w.predictions(fix))
            } else {
                ic.commitText(" ", 1); resetTyping(); keys?.suggestions(w.predictions(before()))
            }
        } else {
            resetTyping(); ic.commitText(value, 1); keys?.suggestions(emptyList())
        }
        inputDone()
    }
    internal fun delete() {
        val ic = currentInputConnection ?: return
        val last = undo
        undo = null
        if (last != null && endsWith(last.prefix + last.replacement + " ") && replace(last.replacement + " ", last.original)) {
            feedback.undo(last.language, last.original, last.replacement)
            prefix = last.prefix; word?.restored(last.original, prefix)
            if (expectedCaret >= 0) expectedCaret += last.original.length - last.replacement.length - 1
            keys?.suggestions(emptyList(), literal = last.original); return
        }
        val w = word
        val track = w != null && w.token.isNotEmpty() && endsWith(prefix + w.token)
        val selected = (selStart >= 0 && selEnd >= 0 && selStart != selEnd) ||
            (!secure && ic.getSelectedText(0)?.isNotEmpty() == true)
        if (selected) { ic.commitText("", 1); resetTyping() }
        else {
            if (!ic.deleteSurroundingTextInCodePoints(1, 0)) sendDownUpKeyEvents(KeyEvent.KEYCODE_DEL)
            if (track) { w!!.pop(); if (expectedCaret >= 0) expectedCaret-- } else resetTyping()
        }
        if (w != null && w.token.isNotEmpty()) showSuggestions(w) else keys?.suggestions(emptyList())
    }
    internal fun enter() {
        resetTyping(); keys?.suggestions(emptyList())
        val info = currentInputEditorInfo
        if (info?.actionLabel != null && (info.imeOptions and EditorInfo.IME_FLAG_NO_ENTER_ACTION) == 0 &&
            currentInputConnection?.performEditorAction(info.actionId) == true) return
        if (!sendDefaultEditorAction(true)) {
            if (currentInputEditorInfo?.inputType == InputType.TYPE_NULL) sendDownUpKeyEvents(KeyEvent.KEYCODE_ENTER)
            else type("\n")
        }
    }
    internal fun nextIme() {
        capture.cancel()
        if (!switchToNextInputMethod(false)) getSystemService(InputMethodManager::class.java).showInputMethodPicker()
    }
    private fun idleHint() = when {
        secure -> "Dictation off in password fields"
        checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED -> "Allow the microphone in Hex"
        else -> "Hold Hex to talk"
    }
    internal fun beginSpeech(): Boolean {
        if (secure || currentInputConnection == null || capture.phase != SpeechPhase.Idle ||
            checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            keys?.status(capture.phase, idleHint())
            return false
        }
        resetTyping(); keys?.suggestions(emptyList())
        capture.start(preferences.options(nextEnglish))
        nextEnglish = null
        return true
    }
    internal fun toggleSpeech() = when (capture.phase) {
        SpeechPhase.Idle -> { beginSpeech(); Unit }
        SpeechPhase.Opening, SpeechPhase.Listening -> finishSpeech()
        SpeechPhase.Transcribing -> Unit
    }
    internal fun finishSpeech() {
        if (capture.phase == SpeechPhase.Listening || capture.phase == SpeechPhase.Opening) capture.finish()
    }
    internal fun cancelSpeech() = capture.cancel()
    private fun commitSpeech(text: String) {
        val ic = currentInputConnection ?: return
        if (secure || !isInputViewShown) return
        val result = insertion(text, ic.getTextBeforeCursor(1, 0)?.lastOrNull(), ic.getTextAfterCursor(1, 0)?.firstOrNull())
        if (result.isNotEmpty()) ic.commitText(result, 1)
    }
    private fun leaveEditor() { keys?.closeMenu(); keys?.cancelGestures(); resetTyping(); word = null; keys?.suggestions(emptyList()); nextEnglish = null; capture.cancel() }
    override fun onFinishInputView(finishingInput: Boolean) { leaveEditor(); super.onFinishInputView(finishingInput) }
    override fun onFinishInput() { leaveEditor(); super.onFinishInput() }
    override fun onWindowHidden() { leaveEditor(); super.onWindowHidden() }
    override fun onDestroy() {
        getSharedPreferences("typing", MODE_PRIVATE).unregisterOnSharedPreferenceChangeListener(typingChanged)
        keys?.cancelGestures(); keys = null; capture.close(); super.onDestroy()
    }
}
