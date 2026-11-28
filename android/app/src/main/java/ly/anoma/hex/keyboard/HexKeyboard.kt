package ly.anoma.hex.keyboard

import android.Manifest
import android.content.BroadcastReceiver
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.SharedPreferences
import android.content.pm.PackageManager
import android.content.res.Configuration
import android.database.ContentObserver
import android.graphics.drawable.Icon
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.provider.UserDictionary
import java.util.concurrent.Executors
import android.inputmethodservice.InputMethodService
import android.util.Size
import android.util.TypedValue
import android.view.View
import android.view.ViewGroup
import android.view.WindowInsetsController
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InlineSuggestionsRequest
import android.view.inputmethod.InlineSuggestionsResponse
import android.view.inputmethod.InputMethodManager
import android.view.inputmethod.InputMethodSubtype
import android.widget.inline.InlinePresentationSpec
import androidx.autofill.inline.UiVersions
import androidx.autofill.inline.common.TextViewStyle
import androidx.autofill.inline.common.ViewStyle
import androidx.autofill.inline.v1.InlineSuggestionUi
import androidx.core.content.ContextCompat
import kotlin.math.roundToInt
import ly.anoma.hex.BuildConfig
import ly.anoma.hex.MainActivity
import ly.anoma.hex.R
import ly.anoma.hex.diagnostics.Diagnostics
import ly.anoma.hex.speech.LOW_MEMORY_NOTE
import ly.anoma.hex.speech.LocalSpeech
import ly.anoma.hex.speech.DictationFixStore
import ly.anoma.hex.speech.DictationPlace
import ly.anoma.hex.speech.MicCapture
import ly.anoma.hex.speech.Signature
import ly.anoma.hex.speech.SpeechPhase
import ly.anoma.hex.speech.SpeechOptions
import ly.anoma.hex.speech.SpeechModel
import ly.anoma.hex.speech.SpeechPreferences
import ly.anoma.hex.speech.Spoken
import ly.anoma.hex.speech.keepAwake
import ly.anoma.hex.speech.swipeUnreadableIn
import ly.anoma.hex.speech.typingLayout
import ly.anoma.hex.typing.AdaptiveTouchStore
import ly.anoma.hex.typing.ImportedWords
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
    LOW_MEMORY_NOTE -> getString(R.string.keyboard_note_low_memory)
    else -> note
}

class HexKeyboard : InputMethodService() {
    private var view: KeyboardView? = null
    /** Labs › Keyboard › Voice-only mode's bar, in place of the keyboard while the voice subtype is current. */
    private var bar: VoiceBarView? = null
    /** The subtype the system last announced; a fresh bind may announce none, so [subtypeMode] asks the manager. */
    private var currentSubtype: InputMethodSubtype? = null
    /** The bar asked to go back for this dictation: its note won't ask again, and only ✕ forces a retry. */
    private var left = false
    /** Posted after a failed dictation's note, so the note is read before the keyboard comes back. */
    private val pendingLeave = Runnable { leaveVoiceOnly() }
    /** The current subtype's mode, "voice" for Labs › Keyboard › Voice-only mode's subtype. */
    private fun subtypeMode(): String? {
        val manager = getSystemService(InputMethodManager::class.java).currentInputMethodSubtype
        if (manager != null) currentSubtype = manager
        return (manager ?: currentSubtype)?.mode
    }
    /** The bar's mode, asked only while the lab is on: with it off the system query never runs. */
    private fun voiceOnly(subtype: InputMethodSubtype? = null) =
        feedback.voiceOnly && VoiceOnly.shows(true, subtype?.mode ?: subtypeMode())
    /** Labs › Keyboard › Voice bar from the Hex menu: the menu's Voice only collapsed the keys for this showing. */
    private var collapsed = false
    /** What the input view should be: null for the keys, false for the voice subtype's bar, true for the collapsed bar.
     * The voice subtype's bar wins, and a password field always gets its keys. */
    private fun barKind(subtype: InputMethodSubtype? = null): Boolean? = when {
        voiceOnly(subtype) -> false
        VoiceOnly.collapses(feedback.voiceBarFromMenu, collapsed, engine.secure) -> true
        else -> null
    }
    private lateinit var capture: MicCapture
    /** The field dictations are spoken in, so a text only ever goes back into its own. */
    private val field = DictationField()
    internal val preferences by lazy { SpeechPreferences(this) }
    /** Labs › Learn from my fixes: the word pairs, their counts and when each was last made, and the
     * tracker's reports. */
    private val fixes by lazy { DictationFixStore(this) }
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
            readPersonal = { PersonalWords.read(contentResolver, it, if (feedback.importedWords) ImportedWords.read(this) else emptyList()) },
            background = words,
            emoji = EmojiWords(assets, words),
            learned = { if (preferences.learnFixes) fixes.record(it) },
            adaptiveTouch = { if (feedback.adaptiveTouch) AdaptiveTouchStore.get(this) else null },
            swipeUnavailable = ::swipeUnavailable)
    }
    private val typingChanged = SharedPreferences.OnSharedPreferenceChangeListener { _, key ->
        if (key == TypingPreferences.SUGGEST_SWEAR_WORDS) engine.swearWordsChanged()
        if (key == TypingPreferences.KEY_COLORS || key == TypingPreferences.KEY_COLOR) recolor()
        if (key == TypingPreferences.NUMBER_ROW) engine.numberRowChanged()
        if (key == TypingPreferences.SWIFTKEY_LAYOUT) engine.swiftKeyLayoutChanged()
        if (key == TypingPreferences.LOCAL_CURRENCY) engine.localCurrencyChanged()
        if (key == TypingPreferences.EMOJI_SUGGESTIONS) engine.emojiSuggestionsChanged()
        if (key == TypingPreferences.KEYBOARD_TOOLS) { engine.keyboardToolsChanged(); watchClipboard() }
        if (key == TypingPreferences.VOICE_ONLY) modeChanged()
        if (key == TypingPreferences.VOICE_BAR_FROM_MENU) { collapsed = false; modeChanged(keepDictation = true) }
        if (key == TypingPreferences.PASSWORD_MANAGER_SUGGESTIONS) clearInline()
        if (key == TypingPreferences.HEIGHT) resize()
        if (key == TypingPreferences.ADAPTIVE_TOUCH) engine.adaptiveTouchChanged()
        if (key == TypingPreferences.HEX_KEY_LEFT) view?.hexKeySideChanged()
        if (key == TypingPreferences.USE_IMPORTED_WORDS || key == TypingPreferences.IMPORTED_WORDS) engine.personalWordsChanged()
        if (key == TypingPreferences.CLIPBOARD_HISTORY) clipKeeper.sync()
    }
    // The strip's Paste tool comes and goes with the clipboard (Labs › Keyboard › Keyboard tools): the
    // listener exists only while the lab is on and the input view is shown.
    private val clipboard by lazy { getSystemService(ClipboardManager::class.java) }
    private val clipboardChanged = ClipboardManager.OnPrimaryClipChangedListener { engine.clipboardChanged() }
    private var viewing = false
    private var watchingClipboard = false
    private fun watchClipboard() {
        val watch = viewing && feedback.keyboardTools && bar == null
        if (watch == watchingClipboard) return
        watchingClipboard = watch
        if (watch) clipboard.addPrimaryClipChangedListener(clipboardChanged)
        else clipboard.removePrimaryClipChangedListener(clipboardChanged)
    }
    internal val previewAllowed get() = !engine.secure

    // Labs › Keyboard › Clipboard history: while the lab is on, every copy is looked at as Android announces it,
    // with the keyboard shown or not (Android lets the default keyboard read the clipboard and shows it no notice).
    // Off, the listener is gone and nothing is read.
    private val clipCopied: ClipboardManager.OnPrimaryClipChangedListener =
        ClipboardManager.OnPrimaryClipChangedListener { clipKeeper.copied() }
    private val clipKeeper: ClipboardKeeper = ClipboardKeeper(enabled = { feedback.clipboardHistory },
        listen = { on -> if (on) clipboard.addPrimaryClipChangedListener(clipCopied) else clipboard.removePrimaryClipChangedListener(clipCopied) },
        read = ::keepClip, forget = { ClipboardHistory.deleteAll(this) })
    /** A field is open: copies made in a password field, or one that asks not to be learned from, are not kept. */
    private var inField = false
    private fun keepClip() = ClipboardHistory.copied(this, clipboard.primaryClipDescription, { clipboard.primaryClip },
        secureField = inField && engine.secure, noLearning = inField && !engine.learnsTyping)
    // Expired texts leave the file when they expire, not when someone next looks: a timer for the earliest expiry while
    // this process lives (none while nothing unpinned is kept), and a purge as the keyboard starts and each time it
    // shows. The timer counts only while the phone is awake and never sees a clock moved on, so the screen coming on and
    // a clock change purge too, listened for only while a text waits to expire. No alarm, no permission, nothing that
    // keeps Hex running; if Android has stopped Hex, the next start purges.
    private val expiry = Handler(Looper.getMainLooper())
    private val expire = Runnable { expireClips() }
    private val wakeOrClock = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) = expireClips()
    }
    private var awaitingExpiry = false
    private fun expireClips() {
        expiry.removeCallbacks(expire)
        val next = if (feedback.clipboardHistory) ClipboardHistory.purge(this) else null
        if (next != null) expiry.postDelayed(expire, (next - System.currentTimeMillis()).coerceAtLeast(0))
        awaitExpiry(next != null)
    }
    private fun awaitExpiry(await: Boolean) {
        if (await == awaitingExpiry) return
        awaitingExpiry = await
        if (await) ContextCompat.registerReceiver(this, wakeOrClock,
            IntentFilter(Intent.ACTION_SCREEN_ON).apply { addAction(Intent.ACTION_TIME_CHANGED) }, ContextCompat.RECEIVER_NOT_EXPORTED)
        else unregisterReceiver(wakeOrClock)
    }
    /** A text from the Hex menu's Clipboard page goes in as typed; never in a password field. */
    internal fun pasteCopied(text: String) {
        if (engine.secure) return
        if (currentInputConnection?.commitText(text, 1) == true) engine.pasted()
    }

    override fun onCreate() {
        setTheme(R.style.HexIme)
        super.onCreate()
        getSharedPreferences("typing", MODE_PRIVATE).registerOnSharedPreferenceChangeListener(typingChanged)
        contentResolver.registerContentObserver(UserDictionary.Words.CONTENT_URI, true, dictionaryChanged)
        capture = MicCapture(applicationContext, LocalSpeech(applicationContext), DictationPlace.Keyboard, update = ::showStatus, meter = ::showLevel,
            insert = ::commitSpeech)
        // The lab off is the keyboard it always was: never touch the system's subtype list for it.
        if (feedback.voiceOnly) VoiceOnly.setEnabled(this, true)
        clipKeeper.sync()
        ClipboardHistory.changed = { expiry.post(expire) }
        expireClips()
    }
    override fun onEvaluateFullscreenMode() = false
    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        matchNavigationBar()
    }

    /** HexIme's navigation bar colour and icon tint reach the window once, in the light or dark mode it was made in;
     * the keys follow a later switch, so the bar under them (Android's back and switcher) is set again to match. */
    private fun matchNavigationBar() {
        val w = window?.window ?: return
        @Suppress("DEPRECATION") // Still the colour Android draws under the IME's back and switcher buttons.
        w.navigationBarColor = getColor(R.color.keyboard)
        val light = WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS
        w.insetsController?.setSystemBarsAppearance(if (resources.getBoolean(R.bool.light_bars)) light else 0, light)
    }
    override fun onCreateInputView(): View = inputView()

    /** The bar for the voice subtype with the lab on, or the collapsed bar, else the full keyboard. */
    private fun inputView(): View {
        bar?.removeCallbacks(pendingLeave)
        val kind = barKind()
        if (kind != null) {
            view = null; engine.surface = null
            return VoiceBarView(this, ::tapVoice, ::cancelVoiceOnly, collapsed = kind).also { bar = it; left = false }
        }
        bar = null
        return KeyboardView(this, this).also {
            it.addOnAttachStateChangeListener(object : View.OnAttachStateChangeListener {
                override fun onViewAttachedToWindow(v: View) = Unit
                override fun onViewDetachedFromWindow(v: View) { viewing = false; watchClipboard() }
            })
            view = it; engine.surface = it
        }
    }

    /** The voice subtype, or its lab, changed under a shown or remembered view: swap it and start dictation.
     * [keepDictation]: collapsing or expanding the keys in the same field, so a dictation going on carries over. */
    private fun modeChanged(subtype: InputMethodSubtype? = null, keepDictation: Boolean = false) {
        // The voice subtype's bar takes over: coming back to Hex's keyboard subtype shows the keys, not the collapsed bar.
        if (voiceOnly(subtype)) collapsed = false
        if (barKind(subtype) == bar?.collapsed) return
        if (view == null && bar == null) return // Nothing shown yet: onCreateInputView picks the mode.
        // Neither mode inherits a dictation started in the other, except the keys and their collapsed bar.
        if (!keepDictation) capture.cancel() else { view?.cancelGestures(); view?.reset() }
        setInputView(inputView())
        if (!isInputViewShown) return
        val voice = bar
        if (voice != null) {
            if (voice.collapsed) { voice.status(capture.phase, barText(capture.phase)); viewing = true; watchClipboard() }
            else beginVoice()
        } else {
            // The swapped-in keyboard is shown for the first time: give it what onStartInputView would. Back from the
            // collapsed bar in the same field, a word half-typed before Voice only keeps its suggestions.
            engine.configureTyping(keepWord = keepDictation)
            view?.status(capture.phase, idleHint())
            Updates.keyboardShown(this, engine.secure) { view?.notice(it) }
            viewing = true
            watchClipboard()
            // Only Show keyboard (or the lab going off) offers a dictation waiting from the collapsed bar; the voice
            // subtype coming back to the keys stays as it was.
            if (keepDictation) offerKept()
        }
    }
    override fun onCurrentInputMethodSubtypeChanged(newSubtype: InputMethodSubtype) {
        super.onCurrentInputMethodSubtypeChanged(newSubtype)
        currentSubtype = newSubtype
        modeChanged(newSubtype)
    }
    /** Labs › Keyboard › Key colors changed (tried in Hex's own settings, the keyboard may be showing): the keys are
     * built again in the new colours, at once unless a dictation is running; then the next showing has them. */
    private fun recolor() {
        val old = view ?: return
        if (old.colored(feedback.drawnKeyColor) || capture.phase != SpeechPhase.Idle) return
        old.cancelGestures(); old.reset()
        setInputView(inputView())
        if (!isInputViewShown) return
        engine.configureTyping(keepWord = true); view?.status(capture.phase, idleHint())
        viewing = true; watchClipboard()
    }

    /** The field's type as Hex last started in it: an app can change it in place (restarting the same box). */
    private var fieldType = 0 to 0
    private var typeChanged = false
    override fun onStartInput(attribute: EditorInfo, restarting: Boolean) {
        super.onStartInput(attribute, restarting)
        typeChanged = restarting && fieldType != attribute.inputType to attribute.imeOptions
        fieldType = attribute.inputType to attribute.imeOptions
        nextEnglish = null
        // Another field never receives an old result: its dictation is kept to offer. A restart counts too: browsers,
        // WebViews and Compose restart one view to move between their inputs, which Hex can't tell from the same field.
        field.started(attribute.packageName)
        capture.leave()
        engine.startInput(attribute)
        inField = true
    }
    override fun onStartInputView(info: EditorInfo, restarting: Boolean) {
        super.onStartInputView(info, restarting)
        matchNavigationBar()
        expireClips()
        // A subtype or lab change may have arrived while no view existed: it shows the right one.
        if (voiceOnly()) collapsed = false
        val kind = barKind()
        if (kind != bar?.collapsed || (kind == null && view == null)) setInputView(inputView())
        bar?.takeIf { it.collapsed }?.let {
            // The collapsed bar stays idle in a new field until its Hex key is tapped.
            it.status(capture.phase, barText(capture.phase))
            viewing = true; watchClipboard()
            return
        }
        if (bar != null) {
            // A restart after the bar asked to leave (text inserted, or Android's picker standing in for a
            // switch) keeps it idle instead of recording the same field again.
            if (VoiceOnly.startsOnShow(restarting, left)) beginVoice()
            else bar?.status(SpeechPhase.Idle, getString(R.string.voice_only_hint))
            viewing = true
            return
        }
        // Settings › Keyboard › Height or Labs › Keyboard › Key colors changed while a dictation ran: the new keys
        // now, before they show.
        if (view?.sized(feedback.height) == false || view?.colored(feedback.drawnKeyColor) == false) setInputView(inputView())
        view?.cancelGestures()
        // Another type in the same box: back to its keys, as a new field opens (configureTyping draws them).
        if (typeChanged) view?.closeMenu(configure = false)
        engine.configureTyping()
        view?.status(capture.phase, idleHint())
        // Hex Preview's update line (Play builds have none); never in a password field.
        Updates.keyboardShown(this, engine.secure) { view?.notice(it) }
        if (BuildConfig.DIAGNOSTICS) Diagnostics.keyboardShown(this, info.packageName, engine.secure) { view?.notice(it) }
        viewing = true
        watchClipboard()
        offerKept()
    }
    override fun onUpdateSelection(oldSelStart: Int, oldSelEnd: Int, newSelStart: Int, newSelEnd: Int,
                                   candidatesStart: Int, candidatesEnd: Int) {
        super.onUpdateSelection(oldSelStart, oldSelEnd, newSelStart, newSelEnd, candidatesStart, candidatesEnd)
        engine.updateSelection(oldSelStart, oldSelEnd, newSelStart, newSelEnd)
    }

    // Android 11's inline autofill suggestions (Labs › Keyboard › Password manager suggestions): the
    // service builds the chips and fills the field when one is tapped; Hex asks for them and lays
    // them out, and never reads one. Off, no request is made, so the system's own autofill dropdown
    // is exactly as before.
    /** A response's inflation still running when the strip moves on is dropped, not shown late. */
    private var inlineGeneration = 0
    /** The strip's inline chips go: a new field, the keyboard hiding, or the lab turning off. */
    private fun clearInline() {
        inlineGeneration++
        view?.inlineSuggestions(emptyList(), emptyList())
    }
    override fun onCreateInlineSuggestionsRequest(uiExtras: Bundle): InlineSuggestionsRequest? {
        // The voice-only bar has no strip to show chips in: asking would put Android in inline mode
        // where nothing shows them, so the system's own autofill UI would never appear either.
        if (!feedback.passwordManagerSuggestions || barKind() != null) return null
        val density = resources.displayMetrics.density
        val strip = (if (resources.configuration.orientation == Configuration.ORIENTATION_LANDSCAPE)
            KeyboardMetrics.STRIP_LANDSCAPE else KeyboardMetrics.STRIP) * density
        val height = InlineAutofill.chipHeight(strip.roundToInt(), density)
        val widths = InlineAutofill.chipWidths(density)
        // Password managers draw their own chip only when the style bundle carries the androidx
        // UiVersions they know; its text follows the strip's. The chip's background is set too,
        // tinted with the key colour: left out, the service draws its own white pill, and the
        // strip's light dark-mode text on it was unreadable (#2888).
        val chip = ViewStyle.Builder().setBackground(
            Icon.createWithResource(this, R.drawable.autofill_chip).setTint(getColor(R.color.key)))
            .build()
        val style = InlineSuggestionUi.newStyleBuilder()
            .setChipStyle(chip)
            .setTitleStyle(TextViewStyle.Builder().setTextColor(getColor(R.color.ink))
                .setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f).build())
            .setSubtitleStyle(TextViewStyle.Builder().setTextColor(getColor(R.color.quiet))
                .setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f).build())
            .build()
        val spec = InlinePresentationSpec.Builder(Size(widths.first, height), Size(widths.last, height))
            .setStyle(UiVersions.newStylesBuilder().addStyle(style).build())
            .build()
        return InlineSuggestionsRequest.Builder(listOf(spec))
            .setMaxSuggestionCount(InlineAutofill.MAX)
            .build()
    }
    /** The service's answer. A tap lets Android fill the field; Hex never reads a login, a code or
     * a card. The unpinned chips fill the scrolling row and the pinned ones sit at its edge. */
    override fun onInlineSuggestionsResponse(response: InlineSuggestionsResponse): Boolean {
        if (!feedback.passwordManagerSuggestions || barKind() != null) return false
        val strip = view ?: return false
        val suggestions = response.inlineSuggestions
        val split = InlineAutofill.split(suggestions.map { it.info.isPinned })
        // A new answer replaces the old row at once, then each chip appears as its inflation
        // lands: one that never does cannot keep stale chips up or hold back the rest.
        val generation = ++inlineGeneration
        strip.inlineSuggestions(emptyList(), emptyList())
        if (suggestions.isEmpty()) return true
        val views = arrayOfNulls<View>(suggestions.size)
        for (index in suggestions.indices) {
            val suggestion = suggestions[index]
            suggestion.inflate(this, Size(ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT), mainExecutor) { content ->
                if (generation != inlineGeneration || view !== strip) return@inflate
                views[index] = content
                strip.inlineSuggestions(
                    split.unpinned.mapNotNull { views[it] }, split.pinned.mapNotNull { views[it] })
            }
        }
        return true
    }

    /** The strip's Paste tool: the editor's own paste, so Hex never reads the clipboard itself. */
    internal fun pasteClipboard() {
        if (!engine.secure && currentInputConnection?.performContextMenuAction(android.R.id.paste) == true) engine.pasted()
    }
    /** The strip's Settings tool: Hex opens on its Keyboard page, not Home. */
    internal fun openKeyboardSettings() = startActivity(Intent(this, MainActivity::class.java)
        .putExtra(MainActivity.EXTRA_PAGE, MainActivity.PAGE_KEYBOARD)
        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP))

    /** Settings › Keyboard › Height changed: the keys are built again at the new size, at once unless a
     * dictation is running (its bar stays until it ends; the next time the keyboard opens has them). */
    private fun resize() {
        val old = view ?: return
        if (old.sized(feedback.height) || capture.phase != SpeechPhase.Idle) return
        old.cancelGestures(); old.reset()
        setInputView(inputView())
        if (isInputViewShown) { engine.configureTyping(keepWord = true); view?.status(capture.phase, idleHint()) }
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
    /** A swipe in a dictation language without a typing model types nothing: the strip names the language until typing
     * starts, a tap or its ✕ (in place of Hex Preview's update line, which comes back in the next field). */
    private fun swipeUnavailable() {
        val language = swipeUnreadableIn(feedback.layout, preferences.spoken) ?: return
        view?.notice(StripNotice(getString(R.string.keyboard_note_swipe, language), getString(R.string.keyboard_note_dismiss),
            tap = {}, dismiss = {}))
    }
    internal fun beginSpeech(): Boolean {
        if (engine.secure || currentInputConnection == null || !capture.canStart ||
            checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            view?.status(capture.phase, idleHint())
            return false
        }
        engine.clearForDictation()
        capture.start(preferences.options(nextEnglish), field.current)
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

    /** The bar records as it appears, with [beginSpeech]'s guards: a field or microphone that can't dictate
     * says why in the bar, which keeps its ✕ to go back. */
    private fun beginVoice() {
        val voice = bar ?: return
        voice.removeCallbacks(pendingLeave)
        left = false
        val blocked = when {
            engine.secure -> getString(R.string.keyboard_hint_password)
            currentInputConnection == null -> getString(R.string.voice_only_unavailable)
            !capture.canStart -> getString(R.string.keyboard_note_microphone)
            checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED ->
                getString(R.string.keyboard_hint_microphone)
            else -> null
        }
        if (blocked != null) { voice.status(SpeechPhase.Idle, blocked); return }
        engine.clearForDictation()
        capture.start(preferences.options(nextEnglish), field.current)
        nextEnglish = null
    }
    /** The bar's ✓: finish the dictation, or start one again after a blocked or failed start. */
    private fun tapVoice() {
        if (bar == null) return
        when (capture.phase) {
            SpeechPhase.Idle -> beginVoice()
            SpeechPhase.Opening, SpeechPhase.Listening -> finishSpeech()
            SpeechPhase.Transcribing -> Unit
        }
    }
    /** Labs › Keyboard › Voice bar from the Hex menu: the menu's Voice only collapses the keys into the bar in this
     * field; the text, the cursor and the field stay, as does a dictation still transcribing. */
    internal fun collapseKeys() {
        if (!feedback.voiceBarFromMenu || engine.secure) return
        collapsed = true
        modeChanged(keepDictation = true)
    }
    /** The collapsed bar's Show keyboard: the keys again in the same field. A recording finishes as with ✓, so what
     * was said still goes in. */
    private fun expandKeys() {
        finishSpeech()
        collapsed = false
        modeChanged(keepDictation = true)
    }
    /** The bar's ✕: always leaves, even when an automatic leave only opened Android's picker. The collapsed bar's
     * first key is Show keyboard instead. */
    private fun cancelVoiceOnly() {
        if (bar?.collapsed == true) { expandKeys(); return }
        cancelSpeech()
        left = false
        leaveVoiceOnly()
    }
    /** The bar is done: back to the keyboard that opened it, or Android's picker when there is none. A picker
     * is not a leave (Hex is still up), so only ✕ asks again; its note never does. */
    private fun leaveVoiceOnly() {
        // The collapsed bar is the full keyboard's own: it stays, idle, for the next dictation.
        if (left || bar == null || bar?.collapsed == true || !isInputViewShown) return
        left = true
        bar?.removeCallbacks(pendingLeave)
        if (!switchToPreviousInputMethod()) getSystemService(InputMethodManager::class.java).showInputMethodPicker()
    }
    private fun showStatus(phase: SpeechPhase, note: String?) {
        // Hands-free dictation can outlast the display timeout; a hidden IME would cancel it.
        window?.window.keepAwake(phase)
        val spoken = speechNote(note)
        val voice = bar
        if (voice != null) {
            voice.status(phase, spoken ?: barText(phase))
            if (voice.collapsed) return
            // A failed dictation says so for a moment before the keyboard comes back, unless its text
            // already went back: the 1-minute stop inserts and then adds that note.
            if (spoken != null && phase == SpeechPhase.Idle && !left) {
                voice.removeCallbacks(pendingLeave)
                voice.postDelayed(pendingLeave, VOICE_NOTE_MS)
            }
            return
        }
        view?.status(phase, spoken ?: idleHint(), note != null)
    }
    /** The bar's line for [phase] when there is no note. */
    private fun barText(phase: SpeechPhase) = getString(when (phase) {
        SpeechPhase.Opening, SpeechPhase.Listening -> R.string.voice_only_listening
        SpeechPhase.Transcribing -> R.string.keyboard_transcribing
        SpeechPhase.Idle -> when {
            bar?.collapsed != true -> R.string.voice_only_hint
            kept?.let(::offerable) == true -> R.string.voice_bar_kept
            else -> R.string.voice_bar_hint
        }
    })
    private fun showLevel(level: Float) {
        bar?.setLevel(level); view?.setLevel(level)
    }
    /** A dictation's text, for the field it was spoken in ([spokenIn]): typed there even with the keyboard hidden, or
     * dropped when that field is gone. */
    private fun commitSpeech(text: String, heard: String, options: SpeechOptions, spokenIn: Int) {
        if (field.takes(spokenIn, currentInputConnection != null, engine.secure)) insertSpoken(text, heard, options)
        else keep(text, heard, options, field.appOf(spokenIn))
        leaveVoiceOnly()
    }
    private fun insertSpoken(text: String, heard: String, options: SpeechOptions) {
        // Read now, so a toggle or text changed while it transcribed applies to this dictation. Hex's own
        // fields, such as Settings' practice box, never get the signature.
        val own = currentInputEditorInfo?.packageName == packageName
        val signed = if (own) null else Signature.append(text, preferences.signatureText, preferences.signature)
        engine.insertDictation(signed ?: text, if (options.translate) "en" else options.spoken.code, signed != null,
            heard, options.learnFixes && preferences.learnFixes && !options.translate)
    }

    /** A dictation whose field went away, held in memory only for [KEPT_MS], never written to disk, the clipboard or a
     * log. The strip offers it once, in a field of the same app that isn't a password one; it goes when inserted,
     * dismissed, when that showing ends, or after [KEPT_MS]. Several join in the order they were spoken. */
    private class Kept(val text: String, val heard: String, val options: SpeechOptions, val app: String) {
        var offered = false
    }
    private var kept: Kept? = null
    private val main = Handler(Looper.getMainLooper())
    private val forgetKept = Runnable { dropKept() }
    private fun keep(text: String, heard: String, options: SpeechOptions, app: String?) {
        if (app == null) return
        val earlier = kept?.takeIf { it.app == app && !it.offered }
        kept = Kept(earlier?.let { it.text + " " + text } ?: text, earlier?.let { it.heard + " " + heard } ?: heard, options, app)
        main.removeCallbacks(forgetKept)
        main.postDelayed(forgetKept, KEPT_MS)
        offerKept()
    }
    private fun offerable(waiting: Kept) = !waiting.offered && isInputViewShown && currentInputConnection != null &&
        DictationField.offersKept(waiting.app, currentInputEditorInfo?.packageName, engine.secure)
    private fun offerKept() {
        val waiting = kept ?: return
        if (!offerable(waiting)) return
        // The collapsed bar has no strip: its line says a dictation waits, and Show keyboard offers it there.
        val voice = bar
        if (voice?.collapsed == true) { if (capture.phase == SpeechPhase.Idle) voice.status(SpeechPhase.Idle, barText(SpeechPhase.Idle)); return }
        val strip = view ?: return
        waiting.offered = true
        strip.notice(StripNotice(getString(R.string.keyboard_kept), getString(R.string.keyboard_kept_discard),
            tap = { insertKept(waiting) }, dismiss = { dropKept() }))
    }
    private fun insertKept(waiting: Kept) {
        if (kept !== waiting) return
        dropKept()
        if (currentInputConnection != null && DictationField.offersKept(waiting.app, currentInputEditorInfo?.packageName, engine.secure))
            insertSpoken(waiting.text, waiting.heard, waiting.options)
    }
    private fun dropKept() {
        val had = kept != null
        kept = null; main.removeCallbacks(forgetKept)
        // The collapsed bar's "dictation waiting" line goes with it.
        val voice = bar
        if (had && voice?.collapsed == true && capture.phase == SpeechPhase.Idle) voice.status(SpeechPhase.Idle, barText(SpeechPhase.Idle))
    }
    /** The keyboard goes away. Hidden with its field still there (an app hiding it, Back), a recording stops as with ✓
     * and its text still goes into that field; with the field gone ([fieldGone]), it stops the same way and its text is
     * kept to offer, never typed into another field. Either way the microphone closes now. A kept dictation the strip
     * already offered goes with this showing. */
    private fun leaveEditor(fieldGone: Boolean) {
        bar?.removeCallbacks(pendingLeave)
        viewing = false; watchClipboard()
        clearInline(); view?.reset(); view?.cancelGestures(); engine.leave(); nextEnglish = null
        if (fieldGone) capture.leave() else finishSpeech()
        if (kept?.offered == true) { dropKept(); view?.notice(null) }
    }
    override fun onFinishInputView(finishingInput: Boolean) { leaveEditor(finishingInput); super.onFinishInputView(finishingInput) }
    override fun onFinishInput() { field.finished(); leaveEditor(fieldGone = true); inField = false; super.onFinishInput() }
    // The keyboard dismissed: the next showing has its keys again (Labs › Keyboard › Voice bar from the Hex menu).
    override fun onWindowHidden() { leaveEditor(fieldGone = false); collapsed = false; super.onWindowHidden() }
    // The speech model is a few hundred MB while loaded; give it back when Android runs short. Not on
    // TRIM_MEMORY_UI_HIDDEN, which only says the keyboard hid (every app switch), not that memory is low.
    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        if (level >= TRIM_MEMORY_RUNNING_LOW && level != TRIM_MEMORY_UI_HIDDEN) SpeechModel.releaseSoon()
    }
    override fun onDestroy() {
        dropKept()
        getSharedPreferences("typing", MODE_PRIVATE).unregisterOnSharedPreferenceChangeListener(typingChanged)
        contentResolver.unregisterContentObserver(dictionaryChanged); words.shutdownNow()
        viewing = false; watchClipboard()
        clipKeeper.stop()
        ClipboardHistory.changed = null; expiry.removeCallbacks(expire); awaitExpiry(false)
        bar?.removeCallbacks(pendingLeave)
        view?.cancelGestures(); view = null; bar = null; engine.surface = null; capture.close(); super.onDestroy()
    }

    private companion object {
        /** A failed voice-only dictation's note stays this long before the keyboard comes back. */
        const val VOICE_NOTE_MS = 1_200L
        /** How long a dictation whose field went away waits, in memory only, to be offered. */
        const val KEPT_MS = 120_000L
    }
}
