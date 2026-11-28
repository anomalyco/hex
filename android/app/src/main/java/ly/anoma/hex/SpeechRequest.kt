package ly.anoma.hex

import android.Manifest
import android.app.Activity
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.os.SystemClock
import android.speech.RecognizerIntent
import android.view.Gravity
import android.view.View
import android.view.WindowInsets
import android.view.WindowManager
import android.widget.FrameLayout
import android.window.OnBackInvokedDispatcher
import ly.anoma.hex.keyboard.VoiceBarView
import ly.anoma.hex.keyboard.speechNote
import ly.anoma.hex.speech.DictationPlace
import ly.anoma.hex.speech.LocalSpeech
import ly.anoma.hex.speech.MicCapture
import ly.anoma.hex.speech.OfflineScreen
import ly.anoma.hex.speech.SpeechPhase
import ly.anoma.hex.speech.SpeechPreferences
import ly.anoma.hex.speech.keepAwake

/** Labs › Keyboard › Answer apps' speech requests: Hex answers Android's standard speech request
 * (`RecognizerIntent.ACTION_RECOGNIZE_SPEECH`), which SwiftKey's mic sends when Google's voice typing isn't used, with
 * [SpeechRequestActivity]. The lab is that activity's enabled state, off in the manifest: while it is off Android never
 * lists Hex as a speech app, and no setting can disagree with what Android offers. */
internal object SpeechRequests {
    private fun component(context: Context) = ComponentName(context, SpeechRequestActivity::class.java)

    fun enabled(context: Context) =
        context.packageManager.getComponentEnabledSetting(component(context)) == PackageManager.COMPONENT_ENABLED_STATE_ENABLED

    fun setEnabled(context: Context, on: Boolean) {
        context.packageManager.setComponentEnabledSetting(component(context),
            if (on) PackageManager.COMPONENT_ENABLED_STATE_ENABLED else PackageManager.COMPONENT_ENABLED_STATE_DEFAULT,
            PackageManager.DONT_KILL_APP)
    }

    /** The keyboard's insets showing on the sheet's window, plus this, before it finishes: the app's field has then
     * reconnected once, so the keyboard that asked finds a field to type the text into. */
    const val SETTLE_MS = 50L
    /** The longest the hand-back waits for the keyboard before finishing anyway. */
    const val HAND_BACK_LIMIT_MS = 400L

    /** How long to wait once the keyboard shows [elapsedMs] after the hand-back began: [SETTLE_MS], never past
     * [HAND_BACK_LIMIT_MS]. */
    fun settleMs(elapsedMs: Long) = SETTLE_MS.coerceAtMost((HAND_BACK_LIMIT_MS - elapsedMs).coerceAtLeast(0L))
}

/**
 * The speech request's sheet: the voice-only bar at the bottom over the app that asked, recording at once with the
 * keyboard's dictation (the same language, style, models, notes and 1-minute stop). ✓ returns the text as the one
 * result; ✕, Back, a tap outside the bar or leaving it return nothing. It makes no network request ([OfflineScreen]).
 *
 * Android 17 and SwiftKey: SwiftKey keeps the result and types it the next time its keyboard starts on a field, trying
 * once. A window that never took the keyboard (`FLAG_ALT_FOCUSABLE_IM`) gives focus back as a report-only change, so
 * the keyboard doesn't start again; one that held it while listening gets the field restarted by Compose and React
 * Native apps just after it closes, under SwiftKey's one try. So the sheet keeps the flag while it listens and, once the
 * text is ready, hides the bar, clears the flag to take the keyboard for a moment, and finishes once the keyboard's
 * insets show on its window (plus [SpeechRequests.SETTLE_MS], at most [SpeechRequests.HAND_BACK_LIMIT_MS]).
 */
class SpeechRequestActivity : Activity(), OfflineScreen {
    private val speech by lazy { SpeechPreferences(this) }
    private lateinit var capture: MicCapture
    private lateinit var bar: VoiceBarView
    private var phase = SpeechPhase.Idle
    /** The text went back: the sheet is only handing the keyboard back now, not being left. */
    private var answered = false
    /** Android's microphone question is up: it is not the person leaving the sheet. */
    private var asking = false
    private var handBackAt = 0L

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setResult(RESULT_CANCELED)
        // The lab turned off while a request was on its way: nothing to answer.
        if (!SpeechRequests.enabled(this)) { finish(); return }
        window.setDecorFitsSystemWindows(false)
        window.addFlags(WindowManager.LayoutParams.FLAG_ALT_FOCUSABLE_IM)
        bar = VoiceBarView(this, ::primary, ::close).apply { isClickable = true }
        capture = MicCapture(LocalSpeech(applicationContext), DictationPlace.SpeechRequest,
            update = ::status, meter = { bar.setLevel(it) }, insert = { text, _, _, _ -> answer(text) })
        // A tap outside the bar returns nothing, as Back does.
        setContentView(FrameLayout(this).apply {
            setOnClickListener { close() }
            addView(bar, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.WRAP_CONTENT, Gravity.BOTTOM))
        })
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU)
            onBackInvokedDispatcher.registerOnBackInvokedCallback(OnBackInvokedDispatcher.PRIORITY_DEFAULT) { close() }
        begin()
    }

    @Deprecated("Android 12's Back; 13 and later use the callback registered in onCreate.")
    override fun onBackPressed() = close()

    private fun begin() {
        if (answered) return
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) {
            capture.start(speech.options(null))
        } else if (!asking) {
            asking = true
            requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), MICROPHONE)
        }
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != MICROPHONE) return
        asking = false
        if (grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED) begin()
        else bar.status(SpeechPhase.Idle, getString(R.string.keyboard_hint_microphone))
    }

    /** ✓ while listening finishes; idle (after a note such as No speech heard) it listens again. */
    private fun primary() = when (phase) {
        SpeechPhase.Opening, SpeechPhase.Listening -> capture.finish()
        SpeechPhase.Idle -> begin()
        SpeechPhase.Transcribing -> Unit
    }

    private fun status(next: SpeechPhase, note: String?) {
        phase = next
        window.keepAwake(next)
        if (answered) return
        bar.status(next, speechNote(note) ?: getString(when (next) {
            SpeechPhase.Opening, SpeechPhase.Listening -> R.string.voice_only_listening
            SpeechPhase.Transcribing -> R.string.keyboard_transcribing
            SpeechPhase.Idle -> R.string.voice_only_hint
        }))
    }

    /** The text as the request's one result, then the hand-back described above. */
    private fun answer(text: String) {
        if (answered || isFinishing) return
        answered = true
        setResult(RESULT_OK, Intent()
            .putStringArrayListExtra(RecognizerIntent.EXTRA_RESULTS, arrayListOf(text))
            .putExtra(RecognizerIntent.EXTRA_CONFIDENCE_SCORES, floatArrayOf(1f)))
        bar.visibility = View.INVISIBLE
        handBackAt = SystemClock.uptimeMillis()
        val decor = window.decorView
        decor.setOnApplyWindowInsetsListener { view, insets ->
            if (insets.isVisible(WindowInsets.Type.ime())) {
                view.setOnApplyWindowInsetsListener(null)
                view.postDelayed(::done, SpeechRequests.settleMs(SystemClock.uptimeMillis() - handBackAt))
            }
            view.onApplyWindowInsets(insets)
        }
        window.clearFlags(WindowManager.LayoutParams.FLAG_ALT_FOCUSABLE_IM)
        decor.postDelayed(::done, SpeechRequests.HAND_BACK_LIMIT_MS)
    }

    private fun done() { if (!isFinishing) finish() }

    /** ✕, Back, a tap outside or leaving: nothing goes back. A text already returned is still handed back. */
    private fun close() {
        if (answered) return
        if (::capture.isInitialized) capture.cancel()
        setResult(RESULT_CANCELED)
        finish()
    }

    // Leaving the sheet (Home, another app, the screen off) returns nothing, as the Quick Settings sheet does.
    override fun onStop() {
        super.onStop()
        if (!isChangingConfigurations && !isFinishing && !asking) close()
    }

    override fun onDestroy() { if (::capture.isInitialized) capture.close(); super.onDestroy() }

    private companion object { const val MICROPHONE = 1 }
}
