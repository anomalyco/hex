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
import androidx.compose.foundation.Canvas
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
import androidx.compose.ui.geometry.Offset
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
import kotlin.math.abs
import kotlin.math.exp
import kotlin.math.sin

private data class Setup(val mic: Boolean = false, val enabled: Boolean = false, val chosen: Boolean = false,
                         val micNeedsSettings: Boolean = false) {
    val ready get() = mic && enabled && chosen
}

class MainActivity : ComponentActivity() {
    private val prefs by lazy { getSharedPreferences("setup", MODE_PRIVATE) }
    private val imm by lazy { getSystemService(InputMethodManager::class.java) }
    private var setup by mutableStateOf(Setup())
    private var page by mutableIntStateOf(0)
    private val changed = object : ContentObserver(Handler(Looper.getMainLooper())) {
        override fun onChange(selfChange: Boolean) = refresh()
    }
    private val permission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { refresh() }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        page = prefs.getInt("page", 0).coerceIn(0, 3)
        for (name in listOf(Settings.Secure.DEFAULT_INPUT_METHOD, Settings.Secure.ENABLED_INPUT_METHODS))
            contentResolver.registerContentObserver(Settings.Secure.getUriFor(name), false, changed)
        refresh()
        setContent {
            HexTheme {
                BackHandler(page in 1..2) { go(page - 1) }
                SetupScreen(page, setup, ::go, ::askForMic,
                    enable = { startActivity(Intent(Settings.ACTION_INPUT_METHOD_SETTINGS)) },
                    choose = { imm.showInputMethodPicker() })
            }
        }
    }

    private fun go(next: Int) {
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
    override fun onDestroy() { contentResolver.unregisterContentObserver(changed); super.onDestroy() }
}

@Composable
private fun HexTheme(content: @Composable () -> Unit) {
    val ink = colorResource(R.color.ink)
    val page = colorResource(R.color.page)
    val scheme = if (isSystemInDarkTheme()) darkColorScheme() else lightColorScheme()
    MaterialTheme(colorScheme = scheme.copy(primary = ink, onPrimary = page, surface = page,
        background = page, onSurface = ink, onBackground = ink, onSurfaceVariant = colorResource(R.color.quiet),
        surfaceVariant = colorResource(R.color.sheet), outline = colorResource(R.color.rule)), content = content)
}

@Composable
private fun SetupScreen(page: Int, setup: Setup, go: (Int) -> Unit, ask: () -> Unit,
                        enable: () -> Unit, choose: () -> Unit) {
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
                    if (page < 3) Text("Step " + (page + 1) + " of 3", color = colors.onSurfaceVariant,
                        style = MaterialTheme.typography.labelLarge)
                }
                Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()),
                    verticalArrangement = if (page == 0) Arrangement.Center else Arrangement.Top) {
                    when (page) {
                        0 -> {
                            WelcomeScope()
                            Spacer(Modifier.height(28.dp))
                            Heading("Talk instead of type.")
                            Detail("Dictate right in your keyboard. Hex is free, works offline and lets your music keep playing.")
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
                        else -> {
                            Spacer(Modifier.height(26.dp))
                            Heading(if (setup.ready) "Your keyboard is ready." else "Set up your keyboard.")
                            Detail(if (setup.ready) "Hold the Hex key anywhere you type." else "Check your keyboard and microphone in setup.")
                            Spacer(Modifier.height(18.dp))
                            TextButton(onClick = { go(0) }) { Text("Run setup again") }
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
                            0 -> go(1)
                            1 -> go(2)
                            2 -> { keyboard?.hide(); go(3) }
                            else -> go(if (setup.ready) 2 else 1)
                        }
                    }, enabled = page != 1 || setup.ready, shape = RoundedCornerShape(14.dp),
                        modifier = Modifier.fillMaxWidth().height(52.dp)) {
                        Text(listOf("Get started", "Continue", "Done", if (setup.ready) "Try the keyboard" else "Set up Hex")[page],
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

// A flat illustration of the Hex key and the dot scope, rather than a pretend recording.
@Composable private fun WelcomeScope() {
    val colors = MaterialTheme.colorScheme
    Row(Modifier.fillMaxWidth().height(112.dp).background(colors.surfaceVariant, RoundedCornerShape(18.dp))
        .padding(horizontal = 20.dp), verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(56.dp).background(colors.primary, RoundedCornerShape(12.dp)), contentAlignment = Alignment.Center) {
            Icon(painterResource(R.drawable.hex_mark), null, Modifier.size(34.dp), tint = colors.onPrimary)
        }
        Spacer(Modifier.width(20.dp))
        Canvas(Modifier.weight(1f).height(64.dp)) {
            val dx = size.width / 36
            val dy = size.height / 10
            for (x in 0..35) {
                val t = x / 35.0
                val wave = 3.7 * exp(-13.0 * (t - .52) * (t - .52)) * sin(t * 17)
                for (y in -4..4) drawCircle(if (abs(y - wave) <= .56) colors.primary else colors.outline,
                    radius = 1.05.dp.toPx(), center = Offset((x + .5f) * dx, size.height / 2 + y * dy))
            }
        }
    }
}
