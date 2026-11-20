package ly.anoma.hex.keyboard

import android.Manifest
import android.content.pm.PackageManager
import android.inputmethodservice.InputMethodService
import android.os.SystemClock
import android.text.InputType
import android.view.KeyEvent
import android.view.View
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import ly.anoma.hex.R
import ly.anoma.hex.speech.MicCapture
import ly.anoma.hex.speech.SpeechPhase

class HexKeyboard : InputMethodService() {
    private var keys: KeyPlane? = null
    private lateinit var capture: MicCapture
    private var shifted = false
    private var locked = false
    private var symbols = false
    private var more = false
    private var lastShift = 0L
    private var selStart = -1
    private var selEnd = -1
    private var secure = false

    override fun onCreate() {
        setTheme(R.style.HexIme)
        super.onCreate()
        capture = MicCapture(assets,
            update = { phase, note -> keys?.status(phase, note ?: idleHint()) },
            meter = { keys?.setLevel(it) }, insert = ::commitSpeech)
    }
    override fun onEvaluateFullscreenMode() = false
    override fun onCreateInputView(): View = KeyPlane(this, this).also { keys = it }

    override fun onStartInput(attribute: EditorInfo, restarting: Boolean) {
        super.onStartInput(attribute, restarting)
        capture.cancel() // Even a different field in the same app must not receive an old result.
        val kind = attribute.inputType and InputType.TYPE_MASK_CLASS
        val variation = attribute.inputType and InputType.TYPE_MASK_VARIATION
        secure = when (kind) {
            InputType.TYPE_CLASS_TEXT -> variation in setOf(InputType.TYPE_TEXT_VARIATION_PASSWORD,
                InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD, InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD)
            InputType.TYPE_CLASS_NUMBER -> variation == InputType.TYPE_NUMBER_VARIATION_PASSWORD
            else -> false
        }
        symbols = kind == InputType.TYPE_CLASS_NUMBER || kind == InputType.TYPE_CLASS_PHONE ||
            kind == InputType.TYPE_CLASS_DATETIME
        more = false; locked = false; lastShift = 0
        selStart = attribute.initialSelStart; selEnd = attribute.initialSelEnd
        refreshCaps()
    }
    override fun onStartInputView(info: EditorInfo, restarting: Boolean) {
        super.onStartInputView(info, restarting)
        keys?.cancelGestures()
        showPlane()
        keys?.status(capture.phase, idleHint())
    }
    override fun onUpdateSelection(oldSelStart: Int, oldSelEnd: Int, newSelStart: Int, newSelEnd: Int,
                                   candidatesStart: Int, candidatesEnd: Int) {
        super.onUpdateSelection(oldSelStart, oldSelEnd, newSelStart, newSelEnd, candidatesStart, candidatesEnd)
        selStart = newSelStart; selEnd = newSelEnd
        if (oldSelStart != newSelStart || oldSelEnd != newSelEnd) refreshCaps()
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
    internal fun mode() { symbols = !symbols; more = false; lastShift = 0; showPlane() }
    internal fun type(value: String) {
        currentInputConnection?.commitText(value, 1)
        lastShift = 0
        if (!locked && shifted) { shifted = false; showPlane() }
    }
    internal fun delete() {
        val ic = currentInputConnection ?: return
        if (selStart >= 0 && selEnd >= 0 && selStart != selEnd) ic.commitText("", 1)
        else if (!ic.deleteSurroundingTextInCodePoints(1, 0)) sendDownUpKeyEvents(KeyEvent.KEYCODE_DEL)
    }
    internal fun enter() {
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
        capture.start()
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
    private fun leaveEditor() { keys?.cancelGestures(); capture.cancel() }
    override fun onFinishInputView(finishingInput: Boolean) { leaveEditor(); super.onFinishInputView(finishingInput) }
    override fun onFinishInput() { leaveEditor(); super.onFinishInput() }
    override fun onWindowHidden() { leaveEditor(); super.onWindowHidden() }
    override fun onDestroy() { keys?.cancelGestures(); keys = null; capture.close(); super.onDestroy() }
}
