package ly.anoma.hex.keyboard

import android.Manifest
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.content.pm.PackageManager
import android.database.ContentObserver
import android.os.Handler
import android.os.Looper
import android.provider.UserDictionary
import java.util.concurrent.Executors
import android.inputmethodservice.InputMethodService
import android.view.View
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.view.inputmethod.InputMethodSubtype
import ly.anoma.hex.MainActivity
import ly.anoma.hex.R
import ly.anoma.hex.speech.LocalSpeech
import ly.anoma.hex.speech.MicCapture
import ly.anoma.hex.speech.Signature
import ly.anoma.hex.speech.SpeechPhase
import ly.anoma.hex.speech.SpeechOptions
import ly.anoma.hex.speech.SpeechModel
import ly.anoma.hex.speech.SpeechPreferences
import ly.anoma.hex.speech.Spoken
import ly.anoma.hex.speech.typingLayout
import ly.anoma.hex.typing.PersonalWords
import ly.anoma.hex.typing.SpatialDecoder
import ly.anoma.hex.typing.TypingModel
import ly.anoma.hex.typing.TypingPreferences
import ly.anoma.hex.updates.Updates

/** The input method: the editor's lifecycle, the views and dictation. Typing lives in [KeyboardEngine]. */
/** Dictation's notes arrive from MicCapture as English sentences: the ones a person sees, in the phone's
 * language (one not listed here shows as it is). */
internal fun Context.speechNote(note: String?) = when (note) {
    "Allow the microphone in Hex" -> getString(R.string.keyboard_hint_microphone)
    "Microphone unavailable. Try again." -> getString(R.string.keyboard_note_microphone)
    "Couldn’t transcribe. Try again." -> getString(R.string.keyboard_note_transcribe)
    "No speech heard" -> getString(R.string.keyboard_note_no_speech)
    "Speech model unavailable" -> getString(R.string.keyboard_note_model)
    "Stopped at 1 minute" -> getString(R.string.keyboard_note_minute)
    else -> note
}

class HexKeyboard : InputMethodService() {
    /** The full keyboard's view, or the voice bar's: only the one the current subtype and the lab ask for. */
    private var view: KeyboardView? = null
    private var voice: VoiceInputView? = null
    /** The mode of the subtype Android started Hex in, from the system and from its own change reports. */
    private var subtypeMode: String? = null
    private val manager by lazy { getSystemService(InputMethodManager::class.java) }
    private lateinit var capture: MicCapture
    internal val preferences by lazy { SpeechPreferences(this) }
    internal var nextEnglish: Boolean? = null
    /** The options the dictation in flight started with: its language decides mid-sentence case. */
    internal val feedback by lazy { TypingPreferences(this) }
    internal val layoutLanguage get() = feedback.layout ?: preferences.spoken.layout ?: Spoken.English
    /** The keys' typing model, or none when the layout is only English's stand-in for a dictation language
     * without one: English keys and accents, but no English next words or autocorrections for its text. */
    internal val typingLanguage get() = typingLayout(feedback.layout, preferences.spoken)?.layoutCode
    // Android's personal dictionary is read off the main thread, and again whenever it changes.
    private val words = Executors.newSingleThreadExecutor { Thread(it, "Hex personal words") }
    private val dictionaryChanged = object : ContentObserver(Handler(Looper.getMainLooper())) {
        override fun onChange(selfChange: Boolean) = engine.personalWordsChanged()
    }
    internal val engine by lazy {
        KeyboardEngine(InputConnectionTarget(this), feedback, { layoutLanguage }, { typingLanguage },
            { SpatialDecoder(TypingModel.asset(assets, it)) },
            readPersonal = { PersonalWords.read(contentResolver, it) }, background = words,
            emoji = EmojiWords(assets, words))
    }
    private val typingChanged = SharedPreferences.OnSharedPreferenceChangeListener { _, key ->
        if (key == TypingPreferences.SUGGEST_SWEAR_WORDS) engine.swearWordsChanged()
        if (key == TypingPreferences.NUMBER_ROW) engine.numberRowChanged()
        if (key == TypingPreferences.LOCAL_CURRENCY) engine.localCurrencyChanged()
        if (key == TypingPreferences.EMOJI_SUGGESTIONS) engine.emojiSuggestionsChanged()
        if (key == TypingPreferences.KEYBOARD_TOOLS) { engine.keyboardToolsChanged(); watchClipboard() }
        if (key == TypingPreferences.VOICE_INPUT) {
            VoiceInput.apply(this, feedback.voiceInput)
            if (isInputViewShown) syncInputView()
        }
    }
    // The strip's Paste tool comes and goes with the clipboard (Labs › Keyboard › Keyboard tools): the
    // listener exists only while the lab is on and the input view is shown.
    private val clipboard by lazy { getSystemService(ClipboardManager::class.java) }
    private val clipboardChanged = ClipboardManager.OnPrimaryClipChangedListener { engine.clipboardChanged() }
    private var viewing = false
    private var watchingClipboard = false
    private fun watchClipboard() {
        val watch = viewing && feedback.keyboardTools
        if (watch == watchingClipboard) return
        watchingClipboard = watch
        if (watch) clipboard.addPrimaryClipChangedListener(clipboardChanged)
        else clipboard.removePrimaryClipChangedListener(clipboardChanged)
    }
    internal val previewAllowed get() = !engine.secure

    override fun onCreate() {
        setTheme(R.style.HexIme)
        super.onCreate()
        getSharedPreferences("typing", MODE_PRIVATE).registerOnSharedPreferenceChangeListener(typingChanged)
        contentResolver.registerContentObserver(UserDictionary.Words.CONTENT_URI, true, dictionaryChanged)
        capture = MicCapture(LocalSpeech(applicationContext),
            update = { phase, note -> showStatus(phase, note) },
            meter = { voice?.setLevel(it) ?: view?.setLevel(it) }, insert = ::commitSpeech)
        // The lab's subtype lives in Android's own settings, so every start adds it again or clears it.
        VoiceInput.apply(this, feedback.voiceInput)
    }
    override fun onEvaluateFullscreenMode() = false
    override fun onCreateInputView(): View {
        manager.currentInputMethodSubtype?.let { subtypeMode = it.mode }
        return inputView(voiceMode())
    }

    /** The full keyboard, or the voice bar with no keys: Hex was opened in its "voice" subtype, so it only
     * dictates. A new view each time, as before, so a rotation or a new window draws at its own size. */
    private fun inputView(voice: Boolean): View {
        this.voice = null; view = null; engine.surface = null
        return if (voice) VoiceInputView(this, this).also { this.voice = it }
        else KeyboardView(this, this).also {
            it.addOnAttachStateChangeListener(object : View.OnAttachStateChangeListener {
                override fun onViewAttachedToWindow(v: View) = Unit
                override fun onViewDetachedFromWindow(v: View) { viewing = false; watchClipboard() }
            })
            view = it; engine.surface = it
        }
    }
    private fun voiceMode() = VoiceInput.active(feedback.voiceInput, subtypeMode)

    /** Shows the voice bar or the full keyboard, whichever the lab and the subtype ask for, and sets it up. */
    private fun syncInputView() {
        val voice = voiceMode()
        if (voice == (this.voice != null)) return
        setInputView(inputView(voice))
        if (voice) startVoice()
        else { engine.configureTyping(keepWord = true); viewing = true; watchClipboard() }
        showStatus(capture.phase, null)
    }

    /** Starts a voice bar's dictation by itself; a note or a password field's message leaves the bar idle. */
    private fun startVoice() {
        if (capture.phase != SpeechPhase.Idle) return
        voice?.post { if (capture.phase == SpeechPhase.Idle && voice != null && voiceMode() && isInputViewShown) beginSpeech() }
    }

    /** The field's type as Hex last started in it: an app can change it in place (restarting the same box). */
    private var fieldType = 0 to 0
    private var typeChanged = false
    override fun onStartInput(attribute: EditorInfo, restarting: Boolean) {
        super.onStartInput(attribute, restarting)
        typeChanged = restarting && fieldType != attribute.inputType to attribute.imeOptions
        fieldType = attribute.inputType to attribute.imeOptions
        nextEnglish = null
        capture.cancel() // Even a different field in the same app must not receive an old result.
        engine.startInput(attribute)
    }
    override fun onStartInputView(info: EditorInfo, restarting: Boolean) {
        super.onStartInputView(info, restarting)
        manager.currentInputMethodSubtype?.let { subtypeMode = it.mode }
        val voiceOnly = voiceMode()
        if (voiceOnly != (voice != null)) setInputView(inputView(voiceOnly))
        view?.cancelGestures()
        // Another type in the same box: back to its keys, as a new field opens (configureTyping draws them).
        if (typeChanged) view?.closeMenu(configure = false)
        if (voiceOnly) {
            showStatus(capture.phase, null)
            startVoice()
            return
        }
        engine.configureTyping()
        showStatus(capture.phase, null)
        // Hex Preview's update line (Play builds have none); never in a password field.
        Updates.keyboardShown(this, engine.secure) { view?.notice(it) }
        viewing = true
        watchClipboard()
    }
    /** The switcher's own subtype change, without a new field: Hex's full keyboard and its voice bar swap. */
    override fun onCurrentInputMethodSubtypeChanged(newSubtype: InputMethodSubtype) {
        super.onCurrentInputMethodSubtypeChanged(newSubtype)
        subtypeMode = newSubtype.mode
        if (isInputViewShown) syncInputView()
    }
    override fun onUpdateSelection(oldSelStart: Int, oldSelEnd: Int, newSelStart: Int, newSelEnd: Int,
                                   candidatesStart: Int, candidatesEnd: Int) {
        super.onUpdateSelection(oldSelStart, oldSelEnd, newSelStart, newSelEnd, candidatesStart, candidatesEnd)
        engine.updateSelection(oldSelStart, oldSelEnd, newSelStart, newSelEnd)
    }

    /** The strip's Paste tool: the editor's own paste, so Hex never reads the clipboard itself. */
    internal fun pasteClipboard() {
        if (!engine.secure && currentInputConnection?.performContextMenuAction(android.R.id.paste) == true) engine.pasted()
    }
    /** The strip's Settings tool: Hex opens on its Keyboard page, not Home. */
    internal fun openKeyboardSettings() = startActivity(Intent(this, MainActivity::class.java)
        .putExtra(MainActivity.EXTRA_PAGE, MainActivity.PAGE_KEYBOARD)
        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP))
    internal fun nextIme() {
        capture.cancel()
        if (!switchToNextInputMethod(false)) getSystemService(InputMethodManager::class.java).showInputMethodPicker()
    }
    private fun idleHint() = getString(when {
        engine.secure -> R.string.keyboard_hint_password
        checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED -> R.string.keyboard_hint_microphone
        voiceMode() -> R.string.keyboard_hint_voice
        else -> R.string.keyboard_hint_hold
    })
    private fun showStatus(phase: SpeechPhase, note: String?) {
        val hint = speechNote(note) ?: idleHint()
        voice?.status(phase, hint) ?: view?.status(phase, hint, note != null)
    }
    internal fun beginSpeech(): Boolean {
        if (engine.secure || currentInputConnection == null || !capture.canStart ||
            checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            showStatus(capture.phase, null)
            return false
        }
        engine.clearForDictation()
        capture.start(preferences.options(nextEnglish))
        nextEnglish = null
        return true
    }
    internal fun toggleSpeech() = when (capture.phase) {
        // A dictation may start while the one before still transcribes; texts go in in order.
        SpeechPhase.Idle, SpeechPhase.Transcribing -> { beginSpeech(); Unit }
        SpeechPhase.Opening, SpeechPhase.Listening -> finishSpeech()
    }
    internal fun finishSpeech() {
        if (capture.phase == SpeechPhase.Listening || capture.phase == SpeechPhase.Opening) capture.finish()
    }
    internal fun cancelSpeech() = capture.cancelLatest()
    /** The voice bar's ✓: starts listening, finishes the recording, or waits out a transcription. */
    internal fun toggleVoice() {
        when (capture.phase) {
            SpeechPhase.Idle -> beginSpeech()
            SpeechPhase.Opening, SpeechPhase.Listening -> finishSpeech()
            SpeechPhase.Transcribing -> Unit
        }
    }
    /** The voice bar's ✕: discards what records or transcribes, then leaves for the usual keyboard. */
    internal fun cancelVoice() {
        capture.cancelLatest()
        returnToKeyboard()
    }
    /** Back to the keyboard someone was using before Hex's voice subtype, else Android's own picker. Never
     * while the words are still coming: switching hides Hex and would take them with it. */
    private fun returnToKeyboard() {
        if (!switchToPreviousInputMethod()) manager.showInputMethodPicker()
    }
    private fun commitSpeech(text: String, options: SpeechOptions) {
        if (currentInputConnection == null || engine.secure || !isInputViewShown) return
        // Read now, so a toggle or text changed while it transcribed applies to this dictation. Hex's own
        // fields, such as Settings' practice box, never get the signature.
        val own = currentInputEditorInfo?.packageName == packageName
        val signed = if (own) null else Signature.append(text, preferences.signatureText, preferences.signature)
        engine.insertDictation(signed ?: text, if (options.translate) "en" else options.spoken.code, signed != null)
        if (voice != null) returnToKeyboard()
    }
    private fun leaveEditor() {
        viewing = false; watchClipboard()
        view?.reset(); view?.cancelGestures(); voice?.reset()
        engine.leave(); nextEnglish = null; capture.cancel()
    }
    override fun onFinishInputView(finishingInput: Boolean) { leaveEditor(); super.onFinishInputView(finishingInput) }
    override fun onFinishInput() { leaveEditor(); super.onFinishInput() }
    override fun onWindowHidden() { leaveEditor(); super.onWindowHidden() }
    // The speech model is a few hundred MB while loaded; give it back when Android runs short. Not on
    // TRIM_MEMORY_UI_HIDDEN, which only says the keyboard hid (every app switch), not that memory is low.
    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        if (level >= TRIM_MEMORY_RUNNING_LOW && level != TRIM_MEMORY_UI_HIDDEN) SpeechModel.releaseSoon()
    }
    override fun onDestroy() {
        getSharedPreferences("typing", MODE_PRIVATE).unregisterOnSharedPreferenceChangeListener(typingChanged)
        contentResolver.unregisterContentObserver(dictionaryChanged); words.shutdownNow()
        viewing = false; watchClipboard()
        view?.cancelGestures(); view = null; voice = null; engine.surface = null
        capture.close(); super.onDestroy()
    }
}
