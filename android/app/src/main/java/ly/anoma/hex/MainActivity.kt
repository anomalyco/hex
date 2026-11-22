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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.text.style.TextAlign
import ly.anoma.hex.speech.*
import ly.anoma.hex.typing.TypingPreferences

internal data class Setup(val mic: Boolean = false, val enabled: Boolean = false, val chosen: Boolean = false,
                         val micNeedsSettings: Boolean = false) {
    val ready get() = mic && enabled && chosen
}

class MainActivity : ComponentActivity() {
    private val prefs by lazy { getSharedPreferences("setup", MODE_PRIVATE) }
    private val imm by lazy { getSystemService(InputMethodManager::class.java) }
    private var setup by mutableStateOf(Setup())
    private var page by mutableIntStateOf(0)
    private val speech by lazy { SpeechPreferences(this) }
    private val typing by lazy { TypingPreferences(this) }
    private var phase by mutableStateOf(SpeechPhase.Idle)
    private var transcript by mutableStateOf("")
    private var notice by mutableStateOf<String?>(null)
    private lateinit var capture: MicCapture
    private val changed = object : ContentObserver(Handler(Looper.getMainLooper())) {
        override fun onChange(selfChange: Boolean) = refresh()
    }
    private val permission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { refresh() }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        capture = MicCapture(LocalSpeech(applicationContext),
            update = { next, reason -> phase = next; notice = reason },
            meter = {}, insert = { text, _ -> transcript = text })
        page = prefs.getInt("page", 0).coerceIn(0, 3)
        for (name in listOf(Settings.Secure.DEFAULT_INPUT_METHOD, Settings.Secure.ENABLED_INPUT_METHODS))
            contentResolver.registerContentObserver(Settings.Secure.getUriFor(name), false, changed)
        refresh()
        ModelStore.refresh(this)
        setContent {
            HexTheme {
                BackHandler(page in 1..2) { go(page - 1) }
                val model = ModelStore.state.collectAsState().value
                if (page == 3) HexSettings(setup, speech, typing, model, { ModelStore.download(this) },
                    phase, transcript, notice, ::dictate, capture::cancel, { go(it) },
                    { startActivity(Intent(Settings.ACTION_INPUT_METHOD_SETTINGS)) })
                else SetupScreen(page, setup, ::go, ::askForMic,
                    enable = { startActivity(Intent(Settings.ACTION_INPUT_METHOD_SETTINGS)) },
                    choose = { imm.showInputMethodPicker() }, model = model,
                    download = { ModelStore.download(this) })
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
    override fun onResume() { super.onResume(); refresh() }
    override fun onWindowFocusChanged(hasFocus: Boolean) { super.onWindowFocusChanged(hasFocus); if (hasFocus) refresh() }
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
                        Icon(painterResource(R.drawable.ic_back), "Back", Modifier.size(22.dp))
                    } else {
                        Icon(painterResource(R.drawable.hex_mark), null, Modifier.size(24.dp))
                        Spacer(Modifier.width(8.dp))
                        Text("Hex", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
                    }
                    Spacer(Modifier.weight(1f))
                    Text("Step " + (page + 1) + " of 3", color = colors.onSurfaceVariant,
                        style = MaterialTheme.typography.labelLarge)
                }
                Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()),
                    verticalArrangement = if (page == 0) Arrangement.Center else Arrangement.Top) {
                    when (page) {
                        0 -> {
                            Welcome()
                        }
                        1 -> {
                            Spacer(Modifier.height(26.dp))
                            Heading("Set up Hex")
                            Detail("Allow the microphone, then choose Hex as your keyboard.")
                            Spacer(Modifier.height(24.dp))
                            Column(Modifier.fillMaxWidth().background(colors.surfaceVariant, RoundedCornerShape(16.dp))
                                .padding(horizontal = 16.dp)) {
                                SetupRow("Microphone", "For dictation", setup.mic,
                                    if (setup.micNeedsSettings) "Settings" else "Allow", ask)
                                HorizontalDivider(color = colors.outline)
                                SetupRow("Hex keyboard", when {
                                    setup.chosen -> "Current keyboard"
                                    setup.enabled -> "Select Hex in the picker"
                                    else -> "Turn on Hex in Settings"
                                }, setup.chosen, if (setup.enabled) "Choose" else "Enable",
                                    if (setup.enabled) choose else enable)
                                HorizontalDivider(color = colors.outline)
                                ModelRow(model, download)
                            }
                        }
                        2 -> {
                            Spacer(Modifier.height(16.dp))
                            Heading("Try the keyboard")
                            Detail("Hold the Hex key, say a few words, then release.")
                            Spacer(Modifier.height(20.dp))
                            val focus = remember { FocusRequester() }
                            OutlinedTextField(value = text, onValueChange = { text = it },
                                modifier = Modifier.fillMaxWidth().focusRequester(focus),
                                placeholder = { Text("Your words appear here") }, minLines = 2, maxLines = 4,
                                shape = RoundedCornerShape(12.dp), keyboardOptions = KeyboardOptions(
                                    capitalization = KeyboardCapitalization.Sentences, imeAction = ImeAction.None))
                            val windowFocused = LocalWindowInfo.current.isWindowFocused
                            LaunchedEffect(windowFocused) {
                                if (windowFocused) { focus.requestFocus(); keyboard?.show() }
                            }
                            if (!setup.chosen) TextButton(onClick = choose) { Text("Choose Hex keyboard") }
                        }
                    }
                }
                if (page == 2 && text.isBlank()) {
                    TextButton(onClick = { keyboard?.hide(); go(3) }, modifier = Modifier.align(Alignment.End)) {
                        Text("Skip for now")
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
                        Text(listOf("Get started", "Continue", "Done")[page],
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
        if (ready) Text("Ready", color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.labelLarge)
        else TextButton(onClick) { Text(action) }
    }
}

@Composable private fun Welcome() {
    val colors = MaterialTheme.colorScheme
    Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
        Icon(painterResource(R.drawable.hex_mark), "Hex", Modifier.size(64.dp))
        Spacer(Modifier.height(20.dp))
        Row(Modifier.height(28.dp).background(colors.surfaceVariant, RoundedCornerShape(50))
            .padding(horizontal = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(painterResource(R.drawable.ic_lock), null, Modifier.size(13.dp))
            Spacer(Modifier.width(6.dp))
            Text("100% on device", fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
        }
        Spacer(Modifier.height(16.dp))
        Text("Talk instead of type,\nin any app.", fontSize = 28.sp, lineHeight = 32.sp,
            letterSpacing = (-0.4).sp, fontWeight = FontWeight.SemiBold, textAlign = TextAlign.Center)
        Spacer(Modifier.height(12.dp))
        Text("Hex uses the internet only to download its speech model (190 MB). Your voice and words stay on this device.",
            modifier = Modifier.widthIn(max = 320.dp), color = colors.onSurfaceVariant,
            textAlign = TextAlign.Center, style = MaterialTheme.typography.bodyMedium)
    }
}

@Composable private fun ModelRow(state: ModelStore.State, download: () -> Unit) {
    Column(Modifier.fillMaxWidth().padding(vertical = 8.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.heightIn(min = 54.dp)) {
            Column(Modifier.weight(1f)) {
                Text("Speech model", fontWeight = FontWeight.Medium)
                Text(when (state) {
                    ModelStore.State.Ready -> "Whisper small is ready"
                    ModelStore.State.Available -> "Tiny works offline"
                    ModelStore.State.Retry -> "Download interrupted. Tiny works offline."
                    is ModelStore.State.Loading -> "Downloading · " + state.percent + "%"
                }, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
            }
            if (state == ModelStore.State.Retry || state == ModelStore.State.Available) {
                TextButton(onClick = download) { Text(if (state == ModelStore.State.Retry) "Retry" else "Download") }
            }
        }
        if (state is ModelStore.State.Loading) LinearProgressIndicator(progress = { state.percent / 100f },
            modifier = Modifier.fillMaxWidth(), trackColor = MaterialTheme.colorScheme.outline)
    }
}
