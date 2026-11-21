package ly.anoma.hex.keyboard

import android.Manifest
import android.content.SharedPreferences
import android.content.pm.PackageManager
import android.inputmethodservice.InputMethodService
import android.view.View
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import ly.anoma.hex.R
import ly.anoma.hex.speech.LocalSpeech
import ly.anoma.hex.speech.MicCapture
import ly.anoma.hex.speech.SpeechPhase
import ly.anoma.hex.speech.SpeechPreferences
import ly.anoma.hex.typing.SpatialDecoder
import ly.anoma.hex.typing.TypingModel
import ly.anoma.hex.typing.TypingPreferences

/** The input method: the editor's lifecycle, the views and dictation. Typing lives in [KeyboardEngine]. */
class HexKeyboard : InputMethodService() {
    private var view: KeyboardView? = null
    private lateinit var capture: MicCapture
    internal val preferences by lazy { SpeechPreferences(this) }
    internal var nextEnglish: Boolean? = null
    internal val feedback by lazy { TypingPreferences(this) }
    internal val layoutLanguage get() = feedback.layout ?: preferences.spoken
    internal val engine by lazy {
        KeyboardEngine(InputConnectionTarget(this), feedback, { layoutLanguage }, { SpatialDecoder(TypingModel.asset(assets, it)) })
    }
    private val typingChanged = SharedPreferences.OnSharedPreferenceChangeListener { _, key ->
        if (key == TypingPreferences.SUGGEST_SWEAR_WORDS) engine.swearWordsChanged()
    }
    internal val previewAllowed get() = !engine.secure

    override fun onCreate() {
        setTheme(R.style.HexIme)
        super.onCreate()
        getSharedPreferences("typing", MODE_PRIVATE).registerOnSharedPreferenceChangeListener(typingChanged)
        capture = MicCapture(LocalSpeech(applicationContext),
            update = { phase, note -> view?.status(phase, note ?: idleHint()) },
            meter = { view?.setLevel(it) }, insert = ::commitSpeech)
    }
    override fun onEvaluateFullscreenMode() = false
    override fun onCreateInputView(): View = KeyboardView(this, this).also { view = it; engine.surface = it }

    override fun onStartInput(attribute: EditorInfo, restarting: Boolean) {
        super.onStartInput(attribute, restarting)
        nextEnglish = null
        capture.cancel() // Even a different field in the same app must not receive an old result.
        engine.startInput(attribute)
    }
    override fun onStartInputView(info: EditorInfo, restarting: Boolean) {
        super.onStartInputView(info, restarting)
        view?.cancelGestures()
        engine.configureTyping()
        view?.status(capture.phase, idleHint())
    }
    override fun onUpdateSelection(oldSelStart: Int, oldSelEnd: Int, newSelStart: Int, newSelEnd: Int,
                                   candidatesStart: Int, candidatesEnd: Int) {
        super.onUpdateSelection(oldSelStart, oldSelEnd, newSelStart, newSelEnd, candidatesStart, candidatesEnd)
        engine.updateSelection(oldSelStart, oldSelEnd, newSelStart, newSelEnd)
    }

    internal fun nextIme() {
        capture.cancel()
        if (!switchToNextInputMethod(false)) getSystemService(InputMethodManager::class.java).showInputMethodPicker()
    }
    private fun idleHint() = getString(when {
        engine.secure -> R.string.keyboard_hint_password
        checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED -> R.string.keyboard_hint_microphone
        else -> R.string.keyboard_hint_hold
    })
    internal fun beginSpeech(): Boolean {
        if (engine.secure || currentInputConnection == null || capture.phase != SpeechPhase.Idle ||
            checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            view?.status(capture.phase, idleHint())
            return false
        }
        engine.clearForDictation()
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
        if (currentInputConnection == null || engine.secure || !isInputViewShown) return
        engine.insertDictation(text)
    }
    private fun leaveEditor() { view?.closeMenu(); view?.cancelGestures(); engine.leave(); nextEnglish = null; capture.cancel() }
    override fun onFinishInputView(finishingInput: Boolean) { leaveEditor(); super.onFinishInputView(finishingInput) }
    override fun onFinishInput() { leaveEditor(); super.onFinishInput() }
    override fun onWindowHidden() { leaveEditor(); super.onWindowHidden() }
    override fun onDestroy() {
        getSharedPreferences("typing", MODE_PRIVATE).unregisterOnSharedPreferenceChangeListener(typingChanged)
        view?.cancelGestures(); view = null; engine.surface = null; capture.close(); super.onDestroy()
    }
}
