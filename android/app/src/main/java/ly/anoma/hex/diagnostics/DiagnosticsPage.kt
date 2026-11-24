package ly.anoma.hex.diagnostics

import android.content.ClipData
import android.content.ClipboardManager
import android.os.Build
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.compose.ui.res.colorResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import ly.anoma.hex.Action
import ly.anoma.hex.Card
import ly.anoma.hex.Entry
import ly.anoma.hex.Label
import ly.anoma.hex.R
import ly.anoma.hex.Rule

/** Labs › Diagnostics: buttons that collect the report and run its tests, and the report itself with Copy. */
@Composable
internal fun DiagnosticsPage() {
    val context = LocalContext.current
    val facts by Diagnostics.facts.collectAsState()
    val app by Diagnostics.app.collectAsState()
    val keyboard by Diagnostics.keyboard.collectAsState()
    val busy = app is Diagnostics.Speed.Running || keyboard is Diagnostics.Speed.Running
    val view = LocalView.current
    // The tap counts change while the person types in another app: read again on coming back and after each test tap.
    var taps by remember { mutableStateOf(Diagnostics.tapCounts()) }
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    DisposableEffect(lifecycle) {
        val observer = LifecycleEventObserver { _, event -> if (event == Lifecycle.Event.ON_RESUME) taps = Diagnostics.tapCounts() }
        lifecycle.addObserver(observer)
        onDispose { lifecycle.removeObserver(observer) }
    }
    Label(stringResource(R.string.diagnostics_contents), muted = true, size = 14, lines = Int.MAX_VALUE)
    Card { Entry(stringResource(R.string.diagnostics_check), value = "", action = { Diagnostics.check(context) }) }
    Group(stringResource(R.string.diagnostics_speed)) {
        Label(stringResource(R.string.diagnostics_speed_detail), size = 14, muted = true, lines = Int.MAX_VALUE,
            mod = Modifier.padding(horizontal = 16.dp).padding(top = 12.dp))
        Entry(stringResource(R.string.diagnostics_run_app), status(app), "",
            if (busy) null else ({ Diagnostics.runInApp(context) }))
        Rule()
        Entry(stringResource(R.string.diagnostics_run_keyboard), status(keyboard) ?: stringResource(R.string.diagnostics_run_keyboard_detail), "",
            if (busy) null else Diagnostics::runInKeyboard)
    }
    Group(stringResource(R.string.diagnostics_haptics)) {
        Label(stringResource(R.string.diagnostics_haptics_detail), size = 14, muted = true, lines = Int.MAX_VALUE,
            mod = Modifier.padding(horizontal = 16.dp).padding(top = 12.dp))
        Entry(stringResource(R.string.diagnostics_tick), stringResource(R.string.diagnostics_tick_detail), "",
            { Diagnostics.testTick(context); taps = Diagnostics.tapCounts() })
        Rule()
        Entry(stringResource(R.string.diagnostics_android_tap), stringResource(R.string.diagnostics_android_tap_detail), "",
            { Diagnostics.androidTap(view) })
        // What each of Hex's taps plays here, once the person has checked the phone.
        if (facts != null) {
            val lines = remember { tapLines(Diagnostics.tapWords(context), Diagnostics.tapPlays(context)) }
            if (lines.isNotEmpty()) Label(lines.joinToString("\n"), size = 13, muted = true, lines = Int.MAX_VALUE,
                mod = Modifier.padding(horizontal = 16.dp).padding(bottom = 10.dp))
        }
        Rule()
        Entry(stringResource(R.string.diagnostics_pulse), stringResource(R.string.diagnostics_pulse_detail), "", { Diagnostics.strongPulse(context) })
    }
    if (facts == null && app == Diagnostics.Speed.Idle && keyboard == Diagnostics.Speed.Idle) {
        Label(stringResource(R.string.diagnostics_none), muted = true, size = 14, lines = Int.MAX_VALUE)
        return
    }
    val text = report(facts, app, keyboard, taps)
    var copied by remember(text) { mutableStateOf(false) }
    Card {
        SelectionContainer {
            BasicText(text, Modifier.padding(start = 16.dp, end = 16.dp, top = 16.dp, bottom = 4.dp),
                style = TextStyle(fontFamily = FontFamily.Monospace, fontSize = 12.sp, lineHeight = 17.sp, color = colorResource(R.color.ink)))
        }
        Row(Modifier.padding(horizontal = 4.dp)) {
            Action(stringResource(if (copied) R.string.home_copied else R.string.diagnostics_copy)) {
                context.getSystemService(ClipboardManager::class.java)?.setPrimaryClip(ClipData.newPlainText("Hex diagnostics", text))
                // Android 13 and later show their own copied preview.
                copied = Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU
            }
        }
    }
}

@Composable private fun Group(title: String, content: @Composable () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Label(title, muted = true, size = 14, mod = Modifier.padding(start = 16.dp))
        Card { content() }
    }
}

@Composable private fun status(speed: Diagnostics.Speed) = when (speed) {
    Diagnostics.Speed.Idle -> null
    Diagnostics.Speed.Waiting -> stringResource(R.string.diagnostics_waiting)
    is Diagnostics.Speed.Running -> stringResource(R.string.diagnostics_running, speed.done + 1, speed.total)
    is Diagnostics.Speed.Done -> stringResource(R.string.diagnostics_done)
    Diagnostics.Speed.Failed -> stringResource(R.string.diagnostics_failed)
}
