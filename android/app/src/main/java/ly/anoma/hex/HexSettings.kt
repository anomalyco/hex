package ly.anoma.hex

import android.content.Context
import android.content.SharedPreferences
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.colorResource
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import ly.anoma.hex.speech.*
import ly.anoma.hex.typing.TypingPreferences

private enum class Page(val title: String) {
    Home("Hex"), Dictation("Dictation"), Keyboard("Keyboard"), Privacy("Privacy"), Experiments("Experiments"),
    Language("Dictation language"), Layout("Keyboard layout"), Styles("Style"),
    Policy("Privacy policy"), Licences("Open-source licences");
    val parent get() = when (this) {
        Language, Styles -> Dictation
        Layout -> Keyboard
        Policy, Licences -> Privacy
        else -> Home
    }
}

@Composable
internal fun HexSettings(setup: Setup, speech: SpeechPreferences, typing: TypingPreferences,
                         model: ModelStore.State, download: () -> Unit, phase: SpeechPhase, transcript: String,
                         notice: String?, dictate: () -> Unit, cancel: () -> Unit, setupPage: (Int) -> Unit,
                         systemSettings: () -> Unit) {
    var page by rememberSaveable { mutableStateOf(Page.Home) }
    var practice by remember { mutableStateOf("") }
    val context = LocalContext.current
    // The activity and the IME write the same preferences.
    // Observe them even while trying the keyboard inside this page.
    var revision by remember { mutableIntStateOf(0) }
    DisposableEffect(context) {
        val stores = listOf("speech", "typing").map { context.getSharedPreferences(it, Context.MODE_PRIVATE) }
        val changed = SharedPreferences.OnSharedPreferenceChangeListener { _, _ -> revision++ }
        stores.forEach { it.registerOnSharedPreferenceChangeListener(changed) }
        onDispose { stores.forEach { it.unregisterOnSharedPreferenceChangeListener(changed) } }
    }
    val automatic = remember(revision, LocalConfiguration.current) { speech.spoken }
    val selectedLanguage = remember(revision) { speech.language }
    val selectedLayout = remember(revision) { typing.layout }
    val style = remember(revision) { speech.style }
    val translate = remember(revision) { speech.translation }
    val clean = remember(revision) { speech.clean }
    val autocorrect = remember(revision) { typing.autocorrect }
    val spatial = remember(revision) { typing.spatial }
    val popups = remember(revision) { typing.popups }
    val haptics = remember(revision) { typing.haptics }
    fun open(next: Page) { if (phase != SpeechPhase.Idle) cancel(); page = next }
    BackHandler(page != Page.Home) { open(page.parent) }

    Box(Modifier.fillMaxSize().background(colorResource(R.color.page)).safeDrawingPadding(),
        contentAlignment = Alignment.TopCenter) {
        key(page) {
            Column(Modifier.widthIn(max = 520.dp).fillMaxWidth().verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp).padding(top = 12.dp, bottom = 24.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp)) {
                Column {
                    Row(Modifier.heightIn(min = 52.dp), verticalAlignment = Alignment.CenterVertically) {
                        if (page == Page.Home) Image(painterResource(R.drawable.hex_mark), null, Modifier.size(27.dp),
                            colorFilter = ColorFilter.tint(colorResource(R.color.ink)))
                        else Box(Modifier.size(44.dp).clickable(role = Role.Button) { open(page.parent) },
                            contentAlignment = Alignment.CenterStart) {
                            Image(painterResource(R.drawable.ic_back), "Back", Modifier.size(24.dp),
                                colorFilter = ColorFilter.tint(colorResource(R.color.ink)))
                        }
                        if (page == Page.Home) Spacer(Modifier.width(10.dp))
                        Label(page.title, size = 26, weight = FontWeight.SemiBold, mod = Modifier.semantics { heading() })
                    }
                    if (page == Page.Home) {
                        val steps = listOf(setup.mic, setup.chosen).count { !it }
                        val status = if (steps == 0) "Ready" else "Finish setup · $steps " + if (steps == 1) "step" else "steps"
                        Row((if (steps > 0) Modifier.clickable(role = Role.Button) { setupPage(1) } else Modifier)
                            .heightIn(min = if (steps > 0) 44.dp else 26.dp), verticalAlignment = Alignment.CenterVertically) {
                            if (steps == 0) Image(painterResource(R.drawable.ic_check), null, Modifier.size(16.dp),
                                colorFilter = ColorFilter.tint(colorResource(R.color.quiet)))
                            Spacer(Modifier.width(4.dp)); Label(status, muted = true, size = 14)
                            if (steps > 0) Label("  ›", muted = true)
                        }
                    }
                }
                when (page) {
                    Page.Home -> {
                        Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(colorResource(R.color.ink))
                            .height(52.dp), verticalAlignment = Alignment.CenterVertically) {
                            Box(Modifier.weight(1f).fillMaxHeight().clickable(phase != SpeechPhase.Transcribing, role = Role.Button) { dictate() },
                                contentAlignment = Alignment.Center) {
                                Label(when (phase) {
                                    SpeechPhase.Idle -> "Dictate"
                                    SpeechPhase.Transcribing -> "Transcribing…"
                                    else -> "Finish dictation"
                                }, inverted = true, weight = FontWeight.SemiBold)
                            }
                            if (phase != SpeechPhase.Idle) Box(Modifier.fillMaxHeight().width(68.dp).clickable(role = Role.Button) { cancel() },
                                contentAlignment = Alignment.Center) { Label("Cancel", inverted = true, size = 14) }
                        }
                        if (transcript.isNotEmpty()) Card { SelectionContainer { Label(transcript, mod = Modifier.padding(16.dp), lines = Int.MAX_VALUE) } }
                        if (notice != null) Label(notice, muted = true, size = 14)
                        Card {
                            listOf(Page.Dictation, Page.Keyboard, Page.Privacy, Page.Experiments).forEachIndexed { i, dest ->
                                if (i != 0) Rule()
                                Entry(dest.title, action = { open(dest) })
                            }
                        }
                        Card {
                            Entry("Keyboard settings", value = "↗", action = systemSettings)
                            Rule(); Entry("Run setup again", action = { setupPage(0) })
                        }
                    }
                    Page.Dictation -> {
                        Card {
                            Entry("Style", value = style.name, action = { open(Page.Styles) })
                            Rule(); Entry("Dictation language", if (selectedLanguage == null) automatic.label else null,
                                selectedLanguage?.label ?: "Automatic", { open(Page.Language) })
                        }
                        Card {
                            Toggle("Translate what I say", "Dictate in Spanish, write in English", translate) { speech.translation = it }
                            Rule(); Toggle("Clean up what I say", "Drop hesitations and spoken corrections", clean) { speech.clean = it }
                        }
                        Card {
                            val detail = when (model) {
                                ModelStore.State.Ready -> "Whisper small is ready"
                                ModelStore.State.Available -> "Tiny works offline"
                                ModelStore.State.Retry -> "Download interrupted; tiny is available"
                                is ModelStore.State.Loading -> "Downloading · ${model.percent}%"
                            }
                            val action = model == ModelStore.State.Available || model == ModelStore.State.Retry
                            Entry("Speech model", detail, if (action) "Download" else "",
                                if (action) download else null)
                        }
                    }
                    Page.Keyboard -> {
                        Card {
                            Entry("Keyboard layout", if (selectedLayout == null) "Follows dictation · " + automatic.label else null,
                                selectedLayout?.label ?: "Automatic", { open(Page.Layout) })
                            Rule(); Toggle("Autocorrect", "Fix typos when you press space", autocorrect) { typing.autocorrect = it }
                            Rule(); Toggle("Key pop-ups", "Show the letter above your finger", popups) { typing.popups = it }
                            Rule(); Toggle("Haptics", "A light tap when you press a key", haptics) { typing.haptics = it }
                        }
                        val ink = colorResource(R.color.ink)
                        Card {
                            BasicTextField(value = practice, onValueChange = { practice = it }, maxLines = 4,
                                modifier = Modifier.fillMaxWidth().padding(16.dp),
                                textStyle = TextStyle(color = ink, fontFamily = inter, fontSize = 16.sp),
                                cursorBrush = SolidColor(ink), decorationBox = { field ->
                                    Box { if (practice.isEmpty()) Label("Test keyboard…", muted = true); field() }
                                })
                        }
                    }
                    Page.Privacy -> {
                        Card {
                            Entry("Everything stays on this device", "Voice and typed text are never uploaded")
                            Rule(); Entry("Typing history", "Hex stores no keystrokes or transcripts")
                        }
                        Card {
                            Entry("Privacy policy", action = { open(Page.Policy) })
                            Rule(); Entry("Open-source licences", action = { open(Page.Licences) })
                        }
                    }
                    Page.Experiments -> Card {
                        Toggle("Spatial autocorrect", if (autocorrect) "Use where taps land to fix typos" else "Turn on Autocorrect in Keyboard",
                            spatial, enabled = autocorrect) { typing.spatial = it }
                    }
                    Page.Styles -> Card {
                        Style.entries.forEachIndexed { i, it ->
                            if (i != 0) Rule()
                            Entry(it.name, value = if (it == style) "✓" else "", action = { speech.style = it; open(Page.Dictation) })
                        }
                    }
                    Page.Language, Page.Layout -> Card {
                        val layout = page == Page.Layout
                        (listOf(null) + Spoken.entries).forEachIndexed { i, it ->
                            if (i != 0) Rule()
                            Entry(it?.label ?: "Automatic", if (it != null) null else if (layout) "Follow dictation language" else "Follow system language",
                                if (it == (if (layout) selectedLayout else selectedLanguage)) "✓" else "", {
                                    if (layout) typing.layout = it else speech.language = it
                                    open(page.parent)
                                })
                        }
                    }
                    Page.Policy, Page.Licences -> {
                        val asset = if (page == Page.Policy) "privacy-policy.txt" else "licenses/credits.txt"
                        val text = remember(asset) { context.assets.open(asset).bufferedReader().use { it.readText() } }
                        SelectionContainer { Label(text.trim(), size = 14, lines = Int.MAX_VALUE) }
                    }
                }
            }
        }
    }
}

@Composable private fun Label(text: String, size: Int = 16, weight: FontWeight = FontWeight.Normal,
                              muted: Boolean = false, inverted: Boolean = false, lines: Int = 1, mod: Modifier = Modifier) {
    BasicText(text, mod, style = TextStyle(fontFamily = inter, fontWeight = weight, fontSize = size.sp,
        lineHeight = (size + 5).sp, color = colorResource(if (inverted) R.color.page else if (muted) R.color.quiet else R.color.ink)),
        maxLines = lines, overflow = TextOverflow.Ellipsis)
}
@Composable private fun Card(content: @Composable ColumnScope.() -> Unit) {
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(colorResource(R.color.sheet)), content = content)
}
@Composable private fun Rule() {
    Spacer(Modifier.fillMaxWidth().padding(start = 16.dp).height(1.dp).background(colorResource(R.color.rule)))
}
@Composable private fun RowScope.RowTitle(title: String, note: String?) {
    Column(Modifier.weight(1f)) {
        Label(title, weight = FontWeight.Medium)
        if (note != null) Label(note, size = 13, muted = true)
    }
}
@Composable private fun Entry(title: String, note: String? = null, value: String = "›", action: (() -> Unit)? = null) {
    val click = if (action != null) Modifier.clickable(role = Role.Button, onClick = action) else Modifier
    Row(Modifier.fillMaxWidth().then(click).heightIn(min = 52.dp).padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        RowTitle(title, note)
        if (action != null && value.isNotEmpty()) Label(value, muted = true, size = if (value == "›") 22 else 15)
    }
}
@Composable private fun Toggle(title: String, note: String, on: Boolean, enabled: Boolean = true, change: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth().toggleable(on, enabled = enabled, role = Role.Switch, onValueChange = change)
        .heightIn(min = 56.dp).padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        RowTitle(title, note)
        Box(Modifier.size(38.dp, 23.dp).background(colorResource(if (on) R.color.ink else R.color.modifier), RoundedCornerShape(50))
            .padding(3.dp), contentAlignment = if (on) Alignment.CenterEnd else Alignment.CenterStart) {
            Spacer(Modifier.size(17.dp).background(colorResource(if (on) R.color.page else R.color.key), RoundedCornerShape(50)))
        }
    }
}
