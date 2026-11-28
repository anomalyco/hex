package ly.anoma.hex

import android.Manifest
import android.app.PendingIntent
import android.app.StatusBarManager
import android.content.ClipboardManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.drawable.Icon
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.addCallback
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.colorResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import ly.anoma.hex.keyboard.hexClip
import ly.anoma.hex.keyboard.speechNote
import ly.anoma.hex.speech.DictationPlace
import ly.anoma.hex.speech.LocalSpeech
import ly.anoma.hex.speech.MicCapture
import ly.anoma.hex.speech.OfflineScreen
import ly.anoma.hex.speech.SpeechPhase
import ly.anoma.hex.speech.SpeechPreferences
import ly.anoma.hex.speech.keepAwake

/** Labs › Dictate from Quick Settings: a Quick Settings tile that opens [QuickDictationActivity], a small sheet over the
 * app in front that dictates as the keyboard does and copies the text to paste, for apps where Hex can't be the
 * keyboard (a work profile's, say). The lab is the tile service's enabled state, off in the manifest: while it is off
 * Android offers no Hex tile at all, and no setting can disagree with what Android shows. */
internal object QuickDictation {
    private fun tile(context: Context) = ComponentName(context, QuickDictationTile::class.java)

    fun enabled(context: Context) =
        context.packageManager.getComponentEnabledSetting(tile(context)) == PackageManager.COMPONENT_ENABLED_STATE_ENABLED

    /** On asks Android (13 and later) to offer the tile for Quick Settings at once; off removes it. */
    fun setEnabled(context: Context, on: Boolean) {
        context.packageManager.setComponentEnabledSetting(tile(context),
            if (on) PackageManager.COMPONENT_ENABLED_STATE_ENABLED else PackageManager.COMPONENT_ENABLED_STATE_DEFAULT,
            PackageManager.DONT_KILL_APP)
        if (on && Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) try {
            context.getSystemService(StatusBarManager::class.java)?.requestAddTileService(tile(context),
                context.getString(R.string.quick_tile), Icon.createWithResource(context, R.drawable.hex_mark),
                context.mainExecutor) {}
        } catch (_: RuntimeException) {
            // Android said no (or has no Quick Settings): the tile is still in Quick Settings' edit list.
        }
    }

    /** After a dictation's text is copied: the sheet closes unless a note ([noteShown]) explains something, since
     * Android 13 and later show their own copied preview; Android 12 shows none, so the sheet says Copied a moment. */
    fun closeAfterCopyMs(noteShown: Boolean, sdk: Int): Long? = when {
        noteShown -> null
        sdk >= Build.VERSION_CODES.TIRAMISU -> 0L
        else -> COPIED_MS
    }
    const val COPIED_MS = 1_200L
}

/** The tile: a tap opens the sheet. It holds no state of its own. On a locked phone Android asks for the unlock
 * itself before the sheet starts (the sheet never shows over the lock screen): System UI (Android 16 on the emulator)
 * already unlocks before delivering the click, and a further unlockAndRun there never ran its open. */
class QuickDictationTile : TileService() {
    override fun onStartListening() {
        super.onStartListening()
        qsTile?.run { state = Tile.STATE_INACTIVE; label = getString(R.string.quick_tile); updateTile() }
    }

    override fun onClick() {
        super.onClick()
        open()
    }

    private fun open() {
        val intent = Intent(this, QuickDictationActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE)
            startActivityAndCollapse(PendingIntent.getActivity(this, 0, intent,
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT))
        else @Suppress("DEPRECATION") startActivityAndCollapse(intent)
    }
}

/** The sheet: it starts listening as it opens, with the keyboard's dictation (the same language, style, model, notes
 * and 1-minute stop), and copies the text. ✕, Back or a tap outside it discard the dictation: nothing is copied. It
 * makes no network request: it neither resumes the model download ([OfflineScreen]) nor checks for updates. */
class QuickDictationActivity : ComponentActivity(), OfflineScreen {
    private val speech by lazy { SpeechPreferences(this) }
    private val setupPrefs by lazy { getSharedPreferences("setup", MODE_PRIVATE) }
    private lateinit var capture: MicCapture
    private var phase by mutableStateOf(SpeechPhase.Idle)
    private var note by mutableStateOf<String?>(null)
    private var copied by mutableStateOf(false)
    /** The microphone was refused for good: only Android's app settings can allow it now. */
    private var micNeedsSettings by mutableStateOf(false)
    /** Android's microphone question is up: it is not the person leaving the sheet. */
    private var asking = false
    private val permission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        asking = false
        if (granted) begin() else {
            micNeedsSettings = !shouldShowRequestPermissionRationale(Manifest.permission.RECORD_AUDIO)
            note = getString(R.string.keyboard_hint_microphone)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // The lab turned off while a tap was on its way: nothing to show.
        if (!QuickDictation.enabled(this)) { finish(); return }
        enableEdgeToEdge()
        window.addFlags(WindowManager.LayoutParams.FLAG_DIM_BEHIND)
        window.setDimAmount(0.32f)
        capture = MicCapture(applicationContext, LocalSpeech(applicationContext), DictationPlace.QuickSettings,
            update = { next, reason -> phase = next; note = speechNote(reason); window.keepAwake(next) },
            meter = {}, insert = { text, _, _, _ -> copy(text) })
        setContent {
            QuickSheet(phase, note, copied, micNeedsSettings, dismiss = { close(sound = true) }, primary = ::primary,
                settings = { startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:$packageName"))) })
        }
        // Back discards like Cancel, with Labs › Dictation sounds' cancel sound.
        onBackPressedDispatcher.addCallback(this) { close(sound = true) }
        if (savedInstanceState == null) begin()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        // The tile again while the sheet is up: a finished or failed sheet starts over; one in progress carries on.
        if (::capture.isInitialized && phase == SpeechPhase.Idle) begin()
    }

    private fun begin() {
        note = null; copied = false; micNeedsSettings = false
        val granted = checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
        when {
            granted -> capture.start(speech.options(null))
            // Setup's own record of having asked: refused for good, Android won't ask again.
            setupPrefs.getBoolean("asked", false) && !shouldShowRequestPermissionRationale(Manifest.permission.RECORD_AUDIO) -> {
                micNeedsSettings = true
                note = getString(R.string.keyboard_hint_microphone)
            }
            else -> {
                setupPrefs.edit().putBoolean("asked", true).apply()
                asking = true
                permission.launch(Manifest.permission.RECORD_AUDIO)
            }
        }
    }

    /** The bar's main part: Finish dictation while listening, Dictate again once idle. */
    private fun primary() = when (phase) {
        SpeechPhase.Opening, SpeechPhase.Listening -> capture.finish()
        SpeechPhase.Idle -> begin()
        SpeechPhase.Transcribing -> Unit
    }

    private fun copy(text: String) {
        getSystemService(ClipboardManager::class.java)?.setPrimaryClip(hexClip(getString(R.string.app_name), text))
        copied = true
        // MicCapture publishes a note (the 1-minute stop, low memory) right after this text: decide once it has.
        window.decorView.post {
            QuickDictation.closeAfterCopyMs(note != null, Build.VERSION.SDK_INT)?.let { window.decorView.postDelayed(::close, it) }
        }
    }

    /** [sound]: the person's Cancel or a tap outside, which plays Labs › Dictation sounds' cancel sound. */
    private fun close(sound: Boolean = false) {
        if (::capture.isInitialized) capture.cancel(sound)
        finish()
    }

    // Leaving the sheet (Home, another app, the screen off) discards what it was doing, as Home's Dictate does.
    override fun onStop() {
        super.onStop()
        if (!isChangingConfigurations && !isFinishing && !asking) close()
    }
    override fun onDestroy() { if (::capture.isInitialized) capture.close(); super.onDestroy() }
}

@androidx.compose.runtime.Composable
private fun QuickSheet(phase: SpeechPhase, note: String?, copied: Boolean, micNeedsSettings: Boolean,
                       dismiss: () -> Unit, primary: () -> Unit, settings: () -> Unit) {
    val none = remember { MutableInteractionSource() }
    // A tap outside the sheet closes it, as Back does.
    Box(Modifier.fillMaxSize().clickable(none, null, onClick = dismiss).safeDrawingPadding().padding(12.dp),
        contentAlignment = Alignment.BottomCenter) {
        Card(Modifier.widthIn(max = 520.dp).clickable(remember { MutableInteractionSource() }, null) {}) {
            Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                val line = when {
                    note != null -> note
                    copied -> stringResource(R.string.quick_copied)
                    phase == SpeechPhase.Opening || phase == SpeechPhase.Listening -> stringResource(R.string.quick_hint)
                    else -> null
                }
                if (line != null) Label(line, muted = note == null, size = 15, lines = 3, mod = Modifier.padding(horizontal = 4.dp))
                if (copied && note != null) Label(stringResource(R.string.quick_copied), muted = true, size = 14,
                    lines = 2, mod = Modifier.padding(horizontal = 4.dp))
                // Home's Dictate bar: the main action, and ✕ beside it.
                Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(colorResource(R.color.ink))
                    .height(52.dp), verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.weight(1f).fillMaxHeight().clickable(phase != SpeechPhase.Transcribing, role = Role.Button,
                        onClick = primary), contentAlignment = Alignment.Center) {
                        Label(stringResource(when (phase) {
                            SpeechPhase.Idle -> R.string.home_dictate
                            SpeechPhase.Transcribing -> R.string.home_transcribing
                            else -> R.string.home_finish
                        }), inverted = true, weight = FontWeight.SemiBold)
                    }
                    Box(Modifier.fillMaxHeight().widthIn(min = 68.dp).clickable(role = Role.Button, onClick = dismiss)
                        .padding(horizontal = 12.dp), contentAlignment = Alignment.Center) {
                        Label(stringResource(if (phase == SpeechPhase.Idle) R.string.quick_close else R.string.home_cancel),
                            inverted = true, size = 14)
                    }
                }
                if (micNeedsSettings) Action(stringResource(R.string.setup_settings), settings)
            }
        }
    }
}
