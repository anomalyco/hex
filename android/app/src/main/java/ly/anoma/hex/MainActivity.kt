package ly.anoma.hex

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.database.ContentObserver
import android.net.Uri
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.view.inputmethod.InputMethodManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.res.colorResource
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.text.style.TextAlign
import kotlinx.coroutines.delay
import ly.anoma.hex.keyboard.speechNote
import ly.anoma.hex.speech.*
import ly.anoma.hex.typing.TypingPreferences
import ly.anoma.hex.updates.Updates

internal data class Setup(val mic: Boolean = false, val enabled: Boolean = false, val chosen: Boolean = false,
                         val micNeedsSettings: Boolean = false) {
    val ready get() = mic && enabled && chosen
}

class MainActivity : ComponentActivity() {
    companion object {
        /** The keyboard's Settings tool opens Hex on this page, not Home: [PAGE_KEYBOARD] names it. */
        const val EXTRA_PAGE = "ly.anoma.hex.extra.page"
        const val PAGE_KEYBOARD = "keyboard"
        /** The keyboard's "Speed test done" line opens Labs › Diagnostics. */
        const val PAGE_DIAGNOSTICS = "diagnostics"
    }
    private val prefs by lazy { getSharedPreferences("setup", MODE_PRIVATE) }
    private val imm by lazy { getSystemService(InputMethodManager::class.java) }
    private var setup by mutableStateOf(Setup())
    private var page by mutableIntStateOf(0)
    private val speech by lazy { SpeechPreferences(this) }
    private val typing by lazy { TypingPreferences(this) }
    private var phase by mutableStateOf(SpeechPhase.Idle)
    private var transcript by mutableStateOf("")
    private var notice by mutableStateOf<String?>(null)
    /** Enable was tapped: back from Android's keyboard list with Hex on, the keyboard picker opens by itself. */
    private var enabling = false
    private lateinit var capture: MicCapture
    private val changed = object : ContentObserver(Handler(Looper.getMainLooper())) {
        override fun onChange(selfChange: Boolean) = refresh()
    }
    private val permission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { refresh() }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        capture = MicCapture(LocalSpeech(applicationContext), DictationPlace.Home,
            // Home's Dictate bar too: onStop cancels, so the screen mustn't sleep mid-dictation.
            update = { next, reason -> phase = next; notice = speechNote(reason); window.keepAwake(next) },
            meter = {}, insert = { text, _, _ -> transcript = text })
        page = prefs.getInt("page", 0).coerceIn(0, 3)
        // The keyboard's Settings tool opens Hex on its Keyboard page for the launch that carried the extra.
        // The task keeps its base intent, so a Recents relaunch still carries it: a restored or history
        // launch ignores it.
        val fromHistory = (intent.flags and Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY) != 0
        val startPage = if (savedInstanceState == null && !fromHistory) intent.getStringExtra(EXTRA_PAGE) else null
        intent.removeExtra(EXTRA_PAGE)
        // Debug builds only: am start --ez feedback_drop_first_response true loses Send feedback's first answer, to
        // show that the retry, with the same Idempotency-Key, gets the same issue back.
        if (BuildConfig.DEBUG) ly.anoma.hex.feedback.Feedback.dropFirstResponse = intent.getBooleanExtra("feedback_drop_first_response", false)
        // Debug builds only: am start --es cpu_capacity 300,300,1024,1024 (right after a force-stop, before any dictation)
        // rates the cores as given, so an emulator, which the kernel doesn't rate, keeps dictation's threads pinned.
        if (BuildConfig.DEBUG) intent.getStringExtra("cpu_capacity")?.let { given ->
            SpeechModel.capacityOverride = given.split(',').map { it.trim().toIntOrNull() }
        }
        for (name in listOf(Settings.Secure.DEFAULT_INPUT_METHOD, Settings.Secure.ENABLED_INPUT_METHODS))
            contentResolver.registerContentObserver(Settings.Secure.getUriFor(name), false, changed)
        refresh()
        ModelStore.small.refresh(this)
        ModelStore.english.refresh(this)
        setContent {
            HexTheme {
                BackHandler(page in 1..2) { go(page - 1) }
                val model = ModelStore.small.state.collectAsState().value
                if (page == 3) HexSettings(setup, speech, typing, model, { ModelStore.small.download(this) },
                    phase, transcript, notice, ::dictate, capture::cancel, { transcript = "" }, { go(it) },
                    { startActivity(Intent(Settings.ACTION_INPUT_METHOD_SETTINGS)) },
                    startPage = startPage)
                else SetupScreen(page, setup, ::go, ::askForMic,
                    enable = { enabling = true; startActivity(Intent(Settings.ACTION_INPUT_METHOD_SETTINGS)) },
                    choose = { imm.showInputMethodPicker() }, model = model,
                    download = { ModelStore.small.download(this) })
            }
        }
    }

    private fun dictate() {
        when (phase) {
            SpeechPhase.Idle -> if (setup.mic) {
                transcript = ""; capture.start(speech.options(null))
            } else askForMic()
            SpeechPhase.Opening, SpeechPhase.Listening -> capture.finish()
            SpeechPhase.Transcribing -> Unit
        }
    }
    private fun go(next: Int) {
        if (page == 3 && next != 3) { capture.cancel(); transcript = "" }
        page = next
        prefs.edit().putInt("page", next).apply()
    }
    private fun refresh() {
        val id = imm.enabledInputMethodList.firstOrNull { it.packageName == packageName }?.id
        val selected = Settings.Secure.getString(contentResolver, Settings.Secure.DEFAULT_INPUT_METHOD)
        val granted = checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
        setup = Setup(granted, id != null, id != null && id == selected,
            prefs.getBoolean("asked", false) && !granted && !shouldShowRequestPermissionRationale(Manifest.permission.RECORD_AUDIO))
    }

    private fun askForMic() {
        if (setup.micNeedsSettings) startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:" + packageName)))
        else {
            prefs.edit().putBoolean("asked", true).apply()
            permission.launch(Manifest.permission.RECORD_AUDIO)
        }
    }
    override fun onResume() { super.onResume(); refresh(); Updates.appOpened(this) }
    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (!hasFocus) return
        refresh()
        // Android asks for two steps (turn Hex on, then pick it); this saves the second tap. Android only shows
        // the picker to the window it already treats as focused, which it learns just after this call.
        if (enabling && page == 1) {
            enabling = false
            if (setup.enabled && !setup.chosen) window.decorView.postDelayed({
                if (hasWindowFocus() && page == 1 && !setup.chosen) imm.showInputMethodPicker()
            }, 300)
        }
    }
    override fun onStop() { capture.cancel(); super.onStop() }
    override fun onDestroy() { capture.close(); contentResolver.unregisterContentObserver(changed); super.onDestroy() }
}

@Composable
private fun HexTheme(content: @Composable () -> Unit) {
    val ink = colorResource(R.color.ink)
    val page = colorResource(R.color.page)
    val scheme = if (isSystemInDarkTheme()) darkColorScheme() else lightColorScheme()
    MaterialTheme(typography = hexTypography, colorScheme = scheme.copy(primary = ink, onPrimary = page, surface = page,
        background = page, onSurface = ink, onBackground = ink, onSurfaceVariant = colorResource(R.color.quiet),
        surfaceVariant = colorResource(R.color.sheet), outline = colorResource(R.color.rule)), content = content)
}

@Composable
private fun SetupScreen(page: Int, setup: Setup, go: (Int) -> Unit, ask: () -> Unit,
                        enable: () -> Unit, choose: () -> Unit, model: ModelStore.State, download: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    var text by remember(page) { mutableStateOf("") }
    val keyboard = LocalSoftwareKeyboardController.current
    Surface(Modifier.fillMaxSize()) {
        Box(Modifier.safeDrawingPadding(), contentAlignment = Alignment.TopCenter) {
            Column(Modifier.widthIn(max = 520.dp).fillMaxWidth().fillMaxHeight().padding(horizontal = 24.dp)) {
                Row(Modifier.fillMaxWidth().height(64.dp), verticalAlignment = Alignment.CenterVertically) {
                    if (page in 1..2) IconButton(onClick = { go(page - 1) }) {
                        Icon(painterResource(R.drawable.ic_back), stringResource(R.string.back), Modifier.size(22.dp))
                    } else {
                        Icon(painterResource(R.drawable.hex_mark), null, Modifier.size(24.dp))
                        Spacer(Modifier.width(8.dp))
                        Text(stringResource(R.string.app_name), style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
                    }
                    Spacer(Modifier.weight(1f))
                    Text(stringResource(R.string.setup_step, page + 1, 3), color = colors.onSurfaceVariant,
                        style = MaterialTheme.typography.labelLarge)
                }
                Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()),
                    verticalArrangement = if (page == 0) Arrangement.Center else Arrangement.Top) {
                    when (page) {
                        0 -> {
                            Welcome()
                        }
                        1 -> {
                            // Moves on by itself once the last row is done here, as iPhone Hex's setup does
                            // (not when Back returns to a page that was already done).
                            var wasReady by remember { mutableStateOf(setup.ready) }
                            LaunchedEffect(setup.ready) {
                                if (setup.ready && !wasReady) { delay(800); go(2) }
                                wasReady = setup.ready
                            }
                            Spacer(Modifier.height(26.dp))
                            Heading(stringResource(R.string.setup_title))
                            Detail(stringResource(R.string.setup_detail))
                            Spacer(Modifier.height(24.dp))
                            Column(Modifier.fillMaxWidth().background(colors.surfaceVariant, RoundedCornerShape(16.dp))
                                .padding(horizontal = 16.dp)) {
                                SetupRow(stringResource(R.string.setup_microphone), stringResource(R.string.setup_microphone_detail), setup.mic,
                                    stringResource(if (setup.micNeedsSettings) R.string.setup_settings else R.string.setup_allow), ask)
                                HorizontalDivider(color = colors.outline)
                                SetupRow(stringResource(R.string.setup_keyboard), stringResource(when {
                                    setup.chosen -> R.string.setup_keyboard_chosen
                                    setup.enabled -> R.string.setup_keyboard_enabled
                                    else -> R.string.setup_keyboard_off
                                }), setup.chosen, stringResource(if (setup.enabled) R.string.setup_choose else R.string.setup_enable),
                                    if (setup.enabled) choose else enable)
                                HorizontalDivider(color = colors.outline)
                                ModelRow(model, download)
                            }
                        }
                        2 -> {
                            Spacer(Modifier.height(16.dp))
                            Heading(stringResource(R.string.setup_try_title))
                            Detail(stringResource(R.string.setup_try_detail))
                            Spacer(Modifier.height(20.dp))
                            val focus = remember { FocusRequester() }
                            OutlinedTextField(value = text, onValueChange = { text = it },
                                modifier = Modifier.fillMaxWidth().focusRequester(focus),
                                placeholder = { Text(stringResource(R.string.setup_try_hint)) }, minLines = 2, maxLines = 4,
                                shape = RoundedCornerShape(12.dp), keyboardOptions = KeyboardOptions(
                                    capitalization = KeyboardCapitalization.Sentences, imeAction = ImeAction.None))
                            val windowFocused = LocalWindowInfo.current.isWindowFocused
                            LaunchedEffect(windowFocused) {
                                if (windowFocused) { focus.requestFocus(); keyboard?.show() }
                            }
                            if (!setup.chosen) TextButton(onClick = choose) { Text(stringResource(R.string.setup_choose_hex)) }
                        }
                    }
                }
                if (page == 2 && text.isBlank()) {
                    TextButton(onClick = { keyboard?.hide(); go(3) }, modifier = Modifier.align(Alignment.End)) {
                        Text(stringResource(R.string.setup_skip))
                    }
                } else {
                    Button(onClick = {
                        when (page) {
                            0 -> { download(); go(1) }
                            1 -> go(2)
                            2 -> { keyboard?.hide(); go(3) }
                        }
                    }, enabled = page != 1 || setup.ready, shape = RoundedCornerShape(14.dp),
                        modifier = Modifier.fillMaxWidth().height(52.dp)) {
                        Text(stringResource(listOf(R.string.setup_get_started, R.string.setup_continue, R.string.setup_done)[page]),
                            style = MaterialTheme.typography.titleMedium)
                    }
                }
                Spacer(Modifier.height(16.dp))
            }
        }
    }
}

@Composable private fun Heading(text: String) = Text(text, fontSize = 34.sp, lineHeight = 39.sp,
    letterSpacing = (-1).sp, fontWeight = FontWeight.SemiBold)

@Composable private fun Detail(text: String) {
    Spacer(Modifier.height(10.dp))
    Text(text, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyLarge)
}

@Composable
private fun SetupRow(title: String, note: String, ready: Boolean, action: String, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().heightIn(min = 76.dp).padding(vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(title, fontWeight = FontWeight.Medium)
            Text(note, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
        }
        if (ready) Text(stringResource(R.string.setup_ready), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.labelLarge)
        else TextButton(onClick) { Text(action) }
    }
}

@Composable private fun Welcome() {
    val colors = MaterialTheme.colorScheme
    Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
        Icon(painterResource(R.drawable.hex_mark), stringResource(R.string.app_name), Modifier.size(64.dp))
        Spacer(Modifier.height(20.dp))
        Row(Modifier.height(28.dp).background(colors.surfaceVariant, RoundedCornerShape(50))
            .padding(horizontal = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(painterResource(R.drawable.ic_lock), null, Modifier.size(13.dp))
            Spacer(Modifier.width(6.dp))
            Text(stringResource(R.string.setup_on_device), fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
        }
        Spacer(Modifier.height(16.dp))
        Text(stringResource(R.string.setup_welcome_title), fontSize = 28.sp, lineHeight = 32.sp,
            letterSpacing = (-0.4).sp, fontWeight = FontWeight.SemiBold, textAlign = TextAlign.Center)
        Spacer(Modifier.height(12.dp))
        Text(stringResource(R.string.setup_welcome_detail),
            modifier = Modifier.widthIn(max = 320.dp), color = colors.onSurfaceVariant,
            textAlign = TextAlign.Center, style = MaterialTheme.typography.bodyMedium)
    }
}

/** The speech model's state in a few words, the same in setup and in Dictation. */
@Composable internal fun modelStatus(state: ModelStore.State) = when (state) {
    ModelStore.State.Ready -> stringResource(R.string.model_ready)
    ModelStore.State.Available -> stringResource(R.string.model_available)
    is ModelStore.State.Retry -> stringResource(when (state.problem) {
        ModelStore.Problem.Connection -> R.string.model_no_connection
        ModelStore.Problem.Space -> R.string.model_no_space
        ModelStore.Problem.Server -> R.string.model_stopped
    })
    is ModelStore.State.Loading -> stringResource(R.string.model_loading, state.percent)
}
internal fun modelAction(state: ModelStore.State) = when (state) {
    ModelStore.State.Available -> R.string.model_download
    is ModelStore.State.Retry -> R.string.model_try_again
    else -> null
}

@Composable private fun ModelRow(state: ModelStore.State, download: () -> Unit) {
    Column(Modifier.fillMaxWidth().padding(vertical = 8.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.heightIn(min = 54.dp)) {
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.model_title), fontWeight = FontWeight.Medium)
                Text(modelStatus(state), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
            }
            modelAction(state)?.let { TextButton(onClick = download) { Text(stringResource(it)) } }
        }
        if (state is ModelStore.State.Loading) LinearProgressIndicator(progress = { state.percent / 100f },
            modifier = Modifier.fillMaxWidth(), trackColor = MaterialTheme.colorScheme.outline)
    }
}
