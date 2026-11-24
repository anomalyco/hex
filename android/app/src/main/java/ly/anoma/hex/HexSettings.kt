package ly.anoma.hex

import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.os.Build
import android.provider.Settings
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
import androidx.compose.ui.layout.boundsInWindow
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.colorResource
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import ly.anoma.hex.diagnostics.DiagnosticsPage
import ly.anoma.hex.feedback.Feedback
import ly.anoma.hex.feedback.FeedbackForm
import ly.anoma.hex.keyboard.VoiceOnly
import ly.anoma.hex.speech.*
import ly.anoma.hex.typing.AdaptiveTouchModel
import ly.anoma.hex.typing.AdaptiveTouchStore
import ly.anoma.hex.typing.TypingPreferences
import ly.anoma.hex.updates.Updates
import ly.anoma.hex.words.WordReplacementsPage
import ly.anoma.hex.words.WordsFilePage
import ly.anoma.hex.words.WordsRows
import ly.anoma.hex.words.WordsStore

private enum class Page(val title: Int) {
    Home(R.string.app_name), Dictation(R.string.settings_dictation), Keyboard(R.string.settings_keyboard), Privacy(R.string.settings_privacy),
    Labs(R.string.settings_labs),
    Language(R.string.settings_dictation_language), Layout(R.string.settings_keyboard_layout), Styles(R.string.settings_style),
    Policy(R.string.settings_privacy_policy), Licences(R.string.settings_licences), Licence(R.string.settings_licences),
    Feedback(R.string.feedback_send_feedback), Diagnostics(R.string.diagnostics),
    Replacements(R.string.words_replacements), WordsFile(R.string.words_file);
    val parent get() = when (this) {
        Diagnostics -> Labs
        Language, Styles, Replacements, WordsFile -> Dictation
        Layout -> Keyboard
        Policy, Licences -> Privacy
        Licence -> Licences
        else -> Home
    }
}

/** A dictation style's name, in the app and the keyboard's menu. */
internal val Style.label get() = when (this) {
    Style.Polished -> R.string.style_polished
    Style.Casual -> R.string.style_casual
    Style.Notes -> R.string.style_notes
    Style.Verbatim -> R.string.style_verbatim
}

/** Licence files the credits name, in the order they appear in assets/licenses. */
private fun licenceFiles(context: Context) =
    context.assets.list("licenses").orEmpty().filter { it != "credits.txt" }.sorted()

@Composable
internal fun HexSettings(setup: Setup, speech: SpeechPreferences, typing: TypingPreferences,
                         model: ModelStore.State, download: () -> Unit, phase: SpeechPhase, transcript: String,
                         notice: String?, dictate: () -> Unit, cancel: () -> Unit, clear: () -> Unit, setupPage: (Int) -> Unit,
                         systemSettings: () -> Unit, startPage: String? = null) {
    var page by rememberSaveable {
        mutableStateOf(when (startPage) {
            MainActivity.PAGE_KEYBOARD -> Page.Keyboard
            MainActivity.PAGE_DIAGNOSTICS -> Page.Diagnostics
            else -> Page.Home
        })
    }
    var licence by rememberSaveable { mutableStateOf("") }
    var practice by remember { mutableStateOf("") }
    val context = LocalContext.current
    // The activity and the IME write the same preferences.
    // Observe them even while trying the keyboard inside this page.
    var revision by remember { mutableIntStateOf(0) }
    DisposableEffect(context) {
        val stores = listOf("speech", "typing", AdaptiveTouchStore.FILE).map { context.getSharedPreferences(it, Context.MODE_PRIVATE) }
        val changed = SharedPreferences.OnSharedPreferenceChangeListener { _, _ -> revision++ }
        stores.forEach { it.registerOnSharedPreferenceChangeListener(changed) }
        onDispose { stores.forEach { it.unregisterOnSharedPreferenceChangeListener(changed) } }
    }
    val automatic = remember(revision, LocalConfiguration.current) { speech.spoken }
    // What Automatic dictates in: one language, or with Labs › Automatic listens for the language those it chooses among.
    val automaticChoices = remember(revision, LocalConfiguration.current) { speech.automaticCandidates }
    val listenForLanguage = remember(revision) { speech.listenForLanguage }
    val selectedLanguage = remember(revision) { speech.language }
    val listening = selectedLanguage == null && automaticChoices.size > 1
    val languageList = automaticChoices.joinToString(stringResource(R.string.language_list_separator)) { it.label }
    val selectedLayout = remember(revision) { typing.layout }
    val style = remember(revision) { speech.style }
    val translate = remember(revision) { speech.translation }
    val clean = remember(revision) { speech.clean }
    val lowercase = remember(revision) { speech.lowercase }
    val endWithPeriod = remember(revision) { speech.endWithPeriod }
    val formatLists = remember(revision) { speech.formatLists }
    val spokenPunctuation = remember(revision) { speech.spokenPunctuation }
    val customVocabulary = remember(revision) { speech.customVocabulary }
    // An import or its Undo changes the terms under the editor, so it reloads them.
    val words = remember(context) { WordsStore(context) }
    val imports = remember(revision) { words.imports }
    var vocabularyDraft by remember(imports) { mutableStateOf(speech.vocabularyTerms.joinToString("\n")) }
    val signature = remember(revision) { speech.signature }
    val fastEnglish = remember(revision) { speech.fastEnglish }
    val learnFixes = remember(revision) { speech.learnFixes }
    val fixStore = remember(context) { DictationFixStore(context) }
    val learnedFixes = remember(revision, learnFixes) { if (learnFixes) fixStore.learned() else emptyList() }
    val disabledFixes = remember(revision, learnFixes) { if (learnFixes) fixStore.disabled() else emptySet() }
    // The lab off stops learning and applying, not the records already on the phone: Delete stays reachable.
    val storedFixes = remember(revision) { fixStore.hasRecords() }
    var forgetting by remember { mutableStateOf(false) }
    val english = ModelStore.english.state.collectAsState().value
    var signatureDraft by remember { mutableStateOf(speech.signatureText) }
    val emojiSuggestions = remember(revision) { typing.emojiSuggestions }
    val keyboardTools = remember(revision) { typing.keyboardTools }
    val smartQuotes = remember(revision) { typing.smartQuotes }
    val swipeDeleteLetters = remember(revision) { typing.swipeDeleteLetters }
    val adaptiveTouch = remember(revision) { typing.adaptiveTouch }
    // Labs › Keyboard › Adaptive touch: how far it has learned (labelled taps on one key plane, and whether any reads your
    // own centres), and whether the phone keeps averages, which Reset touch learning removes even with the lab off.
    val touchStatus = remember(revision, adaptiveTouch) {
        if (adaptiveTouch) AdaptiveTouchStore.get(context).let { it.taps to it.anyActive } else null
    }
    val touchStored = remember(revision) { AdaptiveTouchStore.stored(context) }
    var resettingTouch by remember { mutableStateOf(false) }
    val passwordManager = remember(revision) { typing.passwordManagerSuggestions }
    val voiceOnly = remember(revision) { typing.voiceOnly }
    // A hidden field or leaving Labs shows the saved line again, never an unsaved draft.
    LaunchedEffect(page, signature) { if (page != Page.Labs || !signature) signatureDraft = speech.signatureText }
    LaunchedEffect(page, learnFixes) { if (page != Page.Labs || !learnFixes) forgetting = false }
    LaunchedEffect(page) { if (page != Page.Labs) resettingTouch = false }
    val autoCapitalize = remember(revision) { typing.autoCapitalize }
    val autocorrect = remember(revision) { typing.autocorrect }
    val swearWords = remember(revision) { typing.suggestSwearWords }
    val spatial = remember(revision) { typing.spatial }
    val swipe = remember(revision) { typing.swipe }
    val popups = remember(revision) { typing.popups }
    val haptics = remember(revision) { typing.haptics }
    val letterSymbols = remember(revision) { typing.letterSymbols }
    val numberRow = remember(revision) { typing.numberRow }
    val localCurrency = remember(revision) { typing.localCurrency }
    var copied by remember(transcript) { mutableStateOf(false) }
    fun open(next: Page) {
        // A send in progress keeps the form; leaving it otherwise drops what was written.
        if (page == Page.Feedback && next != Page.Feedback) { if (Feedback.sending) return; Feedback.close() }
        if (phase != SpeechPhase.Idle) cancel(); page = next
    }
    BackHandler(page != Page.Home) { open(page.parent) }
    // Home's dictation result, painted over in the form's picture of Hex: a transcript never leaves the phone.
    var transcriptCard by remember { mutableStateOf<androidx.compose.ui.geometry.Rect?>(null) }
    var transcriptCut by remember { mutableStateOf(false) }
    var capturing by remember { mutableStateOf(false) }
    val cardRadius = with(LocalDensity.current) { 14.dp.toPx() }
    fun sendFeedback() {
        val activity = context as? Activity ?: return
        if (capturing) return
        capturing = true
        val hide = transcriptCard?.takeIf { transcript.isNotEmpty() }
        // Square when scrolled part out of view, so the cut edge's corners are covered too.
        Feedback.captureWindow(activity, hide?.let { android.graphics.RectF(it.left, it.top, it.right, it.bottom) },
            if (transcriptCut) 0f else cardRadius) { picture ->
            capturing = false
            Feedback.start("Home", picture)
            open(Page.Feedback)
        }
    }

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
                            Image(painterResource(R.drawable.ic_back), stringResource(R.string.back), Modifier.size(24.dp),
                                colorFilter = ColorFilter.tint(colorResource(R.color.ink)))
                        }
                        if (page == Page.Home) Spacer(Modifier.width(10.dp))
                        Label(when {
                            page == Page.Licence -> licence
                            page == Page.Feedback && Feedback.phase is Feedback.Phase.Sent -> stringResource(R.string.feedback_thanks_title)
                            else -> stringResource(page.title)
                        }, size = 26, weight = FontWeight.SemiBold,
                            lines = 2, mod = Modifier.semantics { heading() })
                    }
                    if (page == Page.Home) {
                        val steps = listOf(setup.mic, setup.chosen).count { !it }
                        val status = if (steps == 0) stringResource(R.string.home_ready) else pluralStringResource(R.plurals.home_finish_setup, steps, steps)
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
                                Label(stringResource(when (phase) {
                                    SpeechPhase.Idle -> R.string.home_dictate
                                    SpeechPhase.Transcribing -> R.string.home_transcribing
                                    else -> R.string.home_finish
                                }), inverted = true, weight = FontWeight.SemiBold)
                            }
                            if (phase != SpeechPhase.Idle) Box(Modifier.fillMaxHeight().width(68.dp).clickable(role = Role.Button) { cancel() },
                                contentAlignment = Alignment.Center) { Label(stringResource(R.string.home_cancel), inverted = true, size = 14) }
                        }
                        if (transcript.isNotEmpty()) Card(Modifier.onGloballyPositioned {
                            transcriptCard = it.boundsInWindow()
                            transcriptCut = transcriptCard!!.height < it.size.height - 1 || transcriptCard!!.width < it.size.width - 1
                        }) {
                            SelectionContainer { Label(transcript, mod = Modifier.padding(start = 16.dp, end = 16.dp, top = 16.dp, bottom = 4.dp), lines = Int.MAX_VALUE) }
                            // iPhone Hex's Copy and Clear. Android 13 and later show their own copied preview.
                            Row(Modifier.padding(horizontal = 4.dp)) {
                                Action(stringResource(if (copied) R.string.home_copied else R.string.home_copy)) {
                                    context.getSystemService(ClipboardManager::class.java)
                                        ?.setPrimaryClip(ClipData.newPlainText(context.getString(R.string.app_name), transcript))
                                    copied = Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU
                                }
                                Action(stringResource(R.string.home_clear), clear)
                            }
                        }
                        if (notice != null) Label(notice, muted = true, size = 14, lines = 3)
                        Updates.HomeCard()
                        Card {
                            listOf(Page.Dictation, Page.Keyboard, Page.Privacy, Page.Labs).forEachIndexed { i, dest ->
                                if (i != 0) Rule()
                                Entry(stringResource(dest.title), action = { open(dest) })
                            }
                        }
                        Card {
                            Entry(stringResource(R.string.home_keyboard_settings), value = "↗", action = systemSettings)
                            Rule(); Entry(stringResource(R.string.home_run_setup), action = { setupPage(0) })
                            if (Feedback.available) { Rule(); Entry(stringResource(R.string.feedback_send_feedback), action = ::sendFeedback) }
                        }
                        Updates.SettingsCard()
                    }
                    Page.Dictation -> {
                        Card {
                            Entry(stringResource(R.string.settings_style), value = stringResource(style.label), action = { open(Page.Styles) })
                            Rule(); Toggle(stringResource(R.string.dictation_lowercase), stringResource(R.string.dictation_lowercase_detail),
                                lowercase) { speech.lowercase = it }
                            Rule(); Toggle(stringResource(R.string.dictation_format_lists), stringResource(R.string.dictation_format_lists_detail),
                                formatLists) { speech.formatLists = it }
                            Rule(); Toggle(stringResource(R.string.dictation_spoken_punctuation), stringResource(R.string.dictation_spoken_punctuation_detail),
                                spokenPunctuation) { speech.spokenPunctuation = it }
                            Rule(); Toggle(stringResource(R.string.dictation_end_with_period), stringResource(R.string.dictation_end_with_period_detail),
                                endWithPeriod) { speech.endWithPeriod = it }
                            Rule(); Entry(stringResource(R.string.settings_dictation_language),
                                if (selectedLanguage == null) languageList else null,
                                selectedLanguage?.label ?: stringResource(R.string.settings_automatic), { open(Page.Language) })
                        }
                        Card {
                            Toggle(stringResource(R.string.settings_translate), stringResource(R.string.settings_translate_detail), translate) { speech.translation = it }
                            Rule(); Toggle(stringResource(R.string.settings_clean), stringResource(R.string.settings_clean_detail), clean) { speech.clean = it }
                            Rule(); Toggle(stringResource(R.string.dictation_vocabulary), stringResource(R.string.dictation_vocabulary_detail),
                                customVocabulary) { speech.customVocabulary = it }
                            if (customVocabulary) {
                                val ink = colorResource(R.color.ink)
                                BasicTextField(value = vocabularyDraft, onValueChange = {
                                    vocabularyDraft = it; speech.vocabularyTerms = it.split('\n'); words.edited()
                                }, modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp).padding(bottom = 14.dp),
                                    textStyle = TextStyle(color = ink, fontFamily = inter, fontSize = 16.sp),
                                    cursorBrush = SolidColor(ink), decorationBox = { field ->
                                        Box { if (vocabularyDraft.isEmpty()) Label(stringResource(R.string.dictation_vocabulary_hint), muted = true); field() }
                                    })
                            }
                            WordsRows(speech, words, revision, customVocabulary, { open(Page.Replacements) }, { open(Page.WordsFile) })
                        }
                        Card {
                            val action = modelAction(model)
                            Entry(stringResource(R.string.model_title), modelStatus(model), action?.let { stringResource(it) } ?: "",
                                if (action != null) download else null)
                            // Not while Automatic chooses among several languages: those dictations all use today's model.
                            if (fastEnglish && english == ModelStore.State.Ready && !listening) {
                                Rule(); Entry(stringResource(R.string.fast_english_in_use), action = { open(Page.Labs) })
                            }
                        }
                    }
                    Page.Keyboard -> {
                        Card {
                            Entry(stringResource(R.string.settings_keyboard_layout),
                                if (selectedLayout == null) stringResource(R.string.settings_layout_follows, automatic.label) else null,
                                selectedLayout?.label ?: stringResource(R.string.settings_automatic), { open(Page.Layout) })
                            Rule(); Toggle(stringResource(R.string.settings_auto_capitalisation), stringResource(R.string.settings_auto_capitalisation_detail),
                                autoCapitalize) { typing.autoCapitalize = it }
                            Rule(); Toggle(stringResource(R.string.settings_autocorrect), stringResource(R.string.settings_autocorrect_detail), autocorrect) { typing.autocorrect = it }
                            // Shown under Autocorrect while it is on, as on iPhone (it was alone on an Experiments page, on by default).
                            if (autocorrect) { Rule(); Toggle(stringResource(R.string.settings_spatial), stringResource(R.string.settings_spatial_detail), spatial) { typing.spatial = it } }
                            Rule(); Toggle(stringResource(R.string.settings_swipe), stringResource(R.string.settings_swipe_detail), swipe) { typing.swipe = it }
                            Rule(); Toggle(stringResource(R.string.settings_swear_words), stringResource(R.string.settings_swear_words_detail), swearWords) { typing.suggestSwearWords = it }
                            // The words the keyboard keeps and suggests are Android's own personal dictionary.
                            Rule(); Entry(stringResource(R.string.settings_personal_dictionary), stringResource(R.string.settings_personal_dictionary_detail), "↗",
                                { context.startActivity(Intent(Settings.ACTION_USER_DICTIONARY_SETTINGS)) })
                            Rule(); Toggle(stringResource(R.string.settings_popups), stringResource(R.string.settings_popups_detail), popups) { typing.popups = it }
                            Rule(); Toggle(stringResource(R.string.settings_haptics), stringResource(R.string.settings_haptics_detail), haptics) { typing.haptics = it }
                        }
                        val ink = colorResource(R.color.ink)
                        Card {
                            BasicTextField(value = practice, onValueChange = { practice = it }, maxLines = 4,
                                modifier = Modifier.fillMaxWidth().padding(16.dp),
                                textStyle = TextStyle(color = ink, fontFamily = inter, fontSize = 16.sp),
                                cursorBrush = SolidColor(ink), decorationBox = { field ->
                                    Box { if (practice.isEmpty()) Label(stringResource(R.string.settings_test_keyboard), muted = true); field() }
                                })
                        }
                    }
                    Page.Privacy -> {
                        // A statement, not rows that look tappable.
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Image(painterResource(R.drawable.ic_lock), null, Modifier.size(15.dp), colorFilter = ColorFilter.tint(colorResource(R.color.ink)))
                            Spacer(Modifier.width(8.dp)); Label(stringResource(R.string.settings_privacy_line), weight = FontWeight.Medium)
                        }
                        Label(stringResource(R.string.settings_privacy_detail), muted = true, size = 14, lines = Int.MAX_VALUE)
                        Card {
                            Entry(stringResource(R.string.settings_privacy_policy), action = { open(Page.Policy) })
                            Rule(); Entry(stringResource(R.string.settings_licences), action = { open(Page.Licences) })
                        }
                    }
                    Page.Feedback -> FeedbackForm(done = { open(Page.Home) })
                    Page.Diagnostics -> DiagnosticsPage()
                    Page.Replacements -> WordReplacementsPage(speech, words, revision)
                    Page.WordsFile -> WordsFilePage(speech, words, revision)
                    Page.Labs -> {
                        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            Label(stringResource(R.string.labs_dictation), muted = true, size = 14,
                                mod = Modifier.padding(start = 16.dp))
                            Card {
                                val size = ModelStore.english.megabytes
                                                // Its line says what the download is and what stays as it was, and is never cut off, however
                                // large the font.
                                Toggle(stringResource(R.string.fast_english), stringResource(R.string.fast_english_detail, size),
                                    fastEnglish, noteLines = Int.MAX_VALUE) {
                                    speech.fastEnglish = it
                                    // Off stops a download under way and drops its partial file; a finished one stays
                                    // until Delete download.
                                    if (!it && english != ModelStore.State.Ready) ModelStore.english.delete(context)
                                }
                                if (fastEnglish) {
                                    Rule()
                                    // The download starts only on this tap, with its size on the row.
                                    when (english) {
                                        ModelStore.State.Available -> Entry(stringResource(R.string.fast_english_download, size), value = "",
                                            action = { ModelStore.english.download(context) })
                                        is ModelStore.State.Loading -> Entry(stringResource(R.string.fast_english_loading, size, english.percent))
                                        is ModelStore.State.Retry -> Entry(stringResource(when (english.problem) {
                                            ModelStore.Problem.Connection -> R.string.fast_english_no_connection
                                            ModelStore.Problem.Space -> R.string.fast_english_no_space
                                            ModelStore.Problem.Server -> R.string.fast_english_stopped
                                        }), value = stringResource(R.string.model_try_again), action = { ModelStore.english.download(context) })
                                        ModelStore.State.Ready -> Entry(stringResource(R.string.fast_english_ready, size),
                                            value = stringResource(R.string.fast_english_delete),
                                            action = { ModelStore.english.delete(context); speech.fastEnglish = false })
                                    }
                                }
                                Rule()
                                // Labs › Automatic listens for the language: off, Automatic dictates in one language, as before.
                                Toggle(stringResource(R.string.listen_for_language), stringResource(R.string.listen_for_language_detail),
                                    listenForLanguage, noteLines = Int.MAX_VALUE) { speech.listenForLanguage = it }
                                Rule()
                                // Labs › Learn from my fixes: off, the keyboard tracks nothing, applies nothing and
                                // offers no hints; on, the learned word pairs are listed with a switch each.
                                Toggle(stringResource(R.string.learn_fixes), stringResource(R.string.learn_fixes_detail),
                                    learnFixes, noteLines = Int.MAX_VALUE) {
                                    speech.learnFixes = it
                                    if (!it) forgetting = false
                                }
                                if (learnFixes) {
                                    if (learnedFixes.isEmpty()) {
                                        Rule()
                                        Label(stringResource(R.string.learn_fixes_empty), muted = true, size = 13,
                                            lines = Int.MAX_VALUE, mod = Modifier.padding(horizontal = 16.dp, vertical = 12.dp))
                                    } else {
                                        learnedFixes.forEach { fix ->
                                            Rule()
                                            Toggle(fix.heard + " → " + fix.meant,
                                                stringResource(R.string.learn_fixes_count, fix.count),
                                                fix.id !in disabledFixes) { on -> fixStore.disable(fix.id, !on) }
                                        }
                                    }
                                }
                                if (learnFixes || storedFixes) {
                                    Rule()
                                    if (forgetting) {
                                        Row(Modifier.fillMaxWidth().heightIn(min = 52.dp)
                                            .padding(start = 16.dp, end = 4.dp),
                                            verticalAlignment = Alignment.CenterVertically) {
                                            Label(stringResource(R.string.learn_fixes_delete_confirm), size = 14,
                                                lines = 2, mod = Modifier.weight(1f))
                                            Action(stringResource(R.string.learn_fixes_cancel)) { forgetting = false }
                                            Action(stringResource(R.string.learn_fixes_delete_action)) {
                                                fixStore.clear(); AdaptiveTouchStore.clear(context); forgetting = false
                                            }
                                        }
                                    } else {
                                        Entry(stringResource(R.string.learn_fixes_delete), action = { forgetting = true })
                                    }
                                }
                            }
                            Label(stringResource(R.string.labs_output), muted = true, size = 14,
                                mod = Modifier.padding(start = 16.dp))
                            Card {
                                Toggle(stringResource(R.string.signature), stringResource(R.string.signature_detail),
                                    signature) { speech.signature = it }
                                if (signature) {
                                    Rule()
                                    val ink = colorResource(R.color.ink)
                                    val problem = Signature.textProblem(signatureDraft)
                                    Column(Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
                                        verticalArrangement = Arrangement.spacedBy(4.dp)) {
                                        BasicTextField(value = signatureDraft, onValueChange = {
                                            signatureDraft = it
                                            // An invalid draft never replaces the saved line.
                                            if (Signature.textProblem(it) == null) speech.signatureText = it.trim()
                                        }, singleLine = true, modifier = Modifier.fillMaxWidth(),
                                            textStyle = TextStyle(color = ink, fontFamily = inter, fontSize = 16.sp),
                                            cursorBrush = SolidColor(ink), decorationBox = { field ->
                                                Box { if (signatureDraft.isEmpty()) Label(Signature.DEFAULT_TEXT, muted = true); field() }
                                            })
                                        if (problem != null) Label(stringResource(when (problem) {
                                            Signature.Problem.Line -> R.string.signature_one_line
                                            Signature.Problem.Length -> R.string.signature_too_long
                                        }), size = 13, muted = true)
                                    }
                                }
                            }
                            Label(stringResource(R.string.labs_keyboard), muted = true, size = 14,
                                mod = Modifier.padding(start = 16.dp))
                            Card {
                                Toggle(stringResource(R.string.letter_symbols), stringResource(R.string.letter_symbols_detail),
                                    letterSymbols) { typing.letterSymbols = it }
                                Rule()
                                Toggle(stringResource(R.string.number_row), stringResource(R.string.number_row_detail),
                                    numberRow) { typing.numberRow = it }
                                Rule()
                                Toggle(stringResource(R.string.local_currency), stringResource(R.string.local_currency_detail),
                                    localCurrency) { typing.localCurrency = it }
                                Rule()
                                Toggle(stringResource(R.string.emoji_suggestions), stringResource(R.string.emoji_suggestions_detail),
                                    emojiSuggestions) { typing.emojiSuggestions = it }
                                Rule()
                                Toggle(stringResource(R.string.keyboard_tools), stringResource(R.string.keyboard_tools_detail),
                                    keyboardTools) { typing.keyboardTools = it }
                                Rule()
                                Toggle(stringResource(R.string.smart_quotes), stringResource(R.string.smart_quotes_detail),
                                    smartQuotes) { typing.smartQuotes = it }
                                Rule()
                                Toggle(stringResource(R.string.swipe_delete_letters), stringResource(R.string.swipe_delete_letters_detail),
                                    swipeDeleteLetters) { typing.swipeDeleteLetters = it }
                                Rule()
                                // Labs › Keyboard › Adaptive touch: off, the keys read the drawn centres and learn nothing;
                                // what was learned stays on the phone until Reset touch learning or Delete what Hex learned.
                                Toggle(stringResource(R.string.adaptive_touch), stringResource(R.string.adaptive_touch_detail),
                                    adaptiveTouch, noteLines = Int.MAX_VALUE) { typing.adaptiveTouch = it }
                                touchStatus?.let { (taps, active) ->
                                    Label(if (active) stringResource(R.string.adaptive_touch_active) else stringResource(
                                        R.string.adaptive_touch_learning, minOf(taps, AdaptiveTouchModel.ACTIVE_TAPS), AdaptiveTouchModel.ACTIVE_TAPS),
                                        muted = true, size = 13, mod = Modifier.padding(start = 16.dp, end = 16.dp, bottom = 12.dp))
                                }
                                if (adaptiveTouch || touchStored) {
                                    Rule()
                                    if (resettingTouch) {
                                        Row(Modifier.fillMaxWidth().heightIn(min = 52.dp).padding(start = 16.dp, end = 4.dp),
                                            verticalAlignment = Alignment.CenterVertically) {
                                            Label(stringResource(R.string.adaptive_touch_reset_confirm), size = 14, lines = 2,
                                                mod = Modifier.weight(1f))
                                            Action(stringResource(R.string.learn_fixes_cancel)) { resettingTouch = false }
                                            Action(stringResource(R.string.adaptive_touch_reset_action)) {
                                                AdaptiveTouchStore.clear(context); resettingTouch = false
                                            }
                                        }
                                    } else {
                                        Entry(stringResource(R.string.adaptive_touch_reset), action = { resettingTouch = true })
                                    }
                                }
                                Rule()
                                Toggle(stringResource(R.string.password_manager_suggestions),
                                    stringResource(R.string.password_manager_suggestions_detail),
                                    passwordManager) { typing.passwordManagerSuggestions = it }
                                Rule()
                                Toggle(stringResource(R.string.voice_only_mode), stringResource(R.string.voice_only_detail),
                                    voiceOnly) { typing.voiceOnly = it; VoiceOnly.setEnabledFromSettings(context, it) }
                            }
                            // Hex Preview and debug builds only.
                            if (BuildConfig.DIAGNOSTICS) {
                                Label(stringResource(R.string.labs_troubleshooting), muted = true, size = 14,
                                    mod = Modifier.padding(start = 16.dp))
                                Card {
                                    Entry(stringResource(R.string.diagnostics), stringResource(R.string.diagnostics_detail),
                                        action = { open(Page.Diagnostics) })
                                }
                            }
                        }
                    }
                    Page.Styles -> Card {
                        Style.entries.forEachIndexed { i, it ->
                            if (i != 0) Rule()
                            Entry(stringResource(it.label), value = if (it == style) "✓" else "", action = { speech.style = it; open(Page.Dictation) })
                        }
                    }
                    Page.Language, Page.Layout -> Card {
                        val layout = page == Page.Layout
                        // Layouts and dictation languages are separate lists that happen to match today.
                        val choices: List<Triple<String?, Boolean, () -> Unit>> =
                            if (layout) (listOf(null) + Spoken.entries).map { Triple(it?.label, it == selectedLayout, { typing.layout = it }) }
                            else (listOf(null) + DictationLanguage.entries).map { Triple(it?.label, it == selectedLanguage, { speech.language = it }) }
                        choices.forEachIndexed { i, (label, chosen, pick) ->
                            if (i != 0) Rule()
                            // Automatic dictates in one language it picks, never the one spoken: its entry says which, now.
                            // With Labs › Automatic listens for the language, it lists the ones it chooses among.
                            Entry(label ?: stringResource(R.string.settings_automatic),
                                if (label != null) null else if (layout) stringResource(R.string.settings_layout_automatic_detail)
                                else if (automaticChoices.size > 1) languageList
                                else languageList + " · " + stringResource(R.string.dictation_language_automatic_detail),
                                if (chosen) "✓" else "", { pick(); open(page.parent) })
                        }
                    }
                    Page.Policy, Page.Licences, Page.Licence -> {
                        val asset = when (page) { Page.Policy -> "privacy-policy.txt"; Page.Licences -> "licenses/credits.txt"; else -> "licenses/$licence" }
                        val text = remember(asset) { context.assets.open(asset).bufferedReader().use { it.readText() } }
                        SelectionContainer { Column(verticalArrangement = Arrangement.spacedBy(14.dp)) { Document(text) } }
                        // The credits name these files; each opens here.
                        if (page == Page.Licences) Card {
                            licenceFiles(context).forEachIndexed { i, file ->
                                if (i != 0) Rule()
                                Entry(file, action = { licence = file; open(Page.Licence) })
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable internal fun Label(text: String, size: Int = 16, weight: FontWeight = FontWeight.Normal,
                              muted: Boolean = false, inverted: Boolean = false, lines: Int = 1, mod: Modifier = Modifier) {
    BasicText(text, mod, style = TextStyle(fontFamily = inter, fontWeight = weight, fontSize = size.sp,
        lineHeight = (size + 5).sp, color = colorResource(if (inverted) R.color.page else if (muted) R.color.quiet else R.color.ink)),
        maxLines = lines, overflow = TextOverflow.Ellipsis)
}
/** Plain text as paragraphs: hard-wrapped lines joined so they wrap to the screen, and a short first line
 * with no closing punctuation (the policy's section names) set as a heading. */
@Composable private fun Document(text: String) {
    for (block in text.trim().split(Regex("\\n\\s*\\n"))) {
        val lines = block.lines().map { it.trim() }.filter { it.isNotEmpty() }
        if (lines.isEmpty()) continue
        val titled = lines.size > 1 && lines[0].length <= 40 && lines[0].last() !in ".,:;!?)"
        if (titled) Column { Label(lines[0], size = 15, weight = FontWeight.SemiBold); Label(reflow(lines.drop(1)), size = 14, lines = Int.MAX_VALUE) }
        else Label(reflow(lines), size = 14, lines = Int.MAX_VALUE)
    }
}
/** Joins a line to the next unless it ends a sentence or a list entry, or ends with an address. */
private fun reflow(lines: List<String>) = buildString {
    for ((i, line) in lines.withIndex()) {
        append(line)
        if (i == lines.lastIndex) break
        val ends = line.last() in ".:;!?)" || line.substringAfterLast(' ').startsWith("http")
        append(if (ends) "\n" else " ")
    }
}
@Composable internal fun Action(text: String, onClick: () -> Unit) {
    Box(Modifier.heightIn(min = 48.dp).clickable(role = Role.Button, onClick = onClick).padding(horizontal = 12.dp),
        contentAlignment = Alignment.Center) { Label(text, weight = FontWeight.Medium, size = 15) }
}
@Composable internal fun Card(mod: Modifier = Modifier, content: @Composable ColumnScope.() -> Unit) {
    Column(mod.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(colorResource(R.color.sheet)), content = content)
}
@Composable internal fun Rule() {
    Spacer(Modifier.fillMaxWidth().padding(start = 16.dp).height(1.dp).background(colorResource(R.color.rule)))
}
@Composable private fun RowScope.RowTitle(title: String, note: String?, noteLines: Int = 2) {
    Column(Modifier.weight(1f)) {
        Label(title, weight = FontWeight.Medium)
        if (note != null) Label(note, size = 13, muted = true, lines = noteLines)
    }
}
@Composable internal fun Entry(title: String, note: String? = null, value: String = "›", action: (() -> Unit)? = null) {
    val click = if (action != null) Modifier.clickable(role = Role.Button, onClick = action) else Modifier
    Row(Modifier.fillMaxWidth().then(click).heightIn(min = 52.dp).padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        RowTitle(title, note)
        if (action != null && value.isNotEmpty()) Label(value, muted = true, size = if (value == "›") 22 else 15)
    }
}
@Composable internal fun Toggle(title: String, note: String, on: Boolean, enabled: Boolean = true, noteLines: Int = 2,
                                change: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth().toggleable(on, enabled = enabled, role = Role.Switch, onValueChange = change)
        .heightIn(min = 56.dp).padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        RowTitle(title, note, noteLines)
        Box(Modifier.size(38.dp, 23.dp).background(colorResource(if (on) R.color.ink else R.color.modifier), RoundedCornerShape(50))
            .padding(3.dp), contentAlignment = if (on) Alignment.CenterEnd else Alignment.CenterStart) {
            Spacer(Modifier.size(17.dp).background(colorResource(if (on) R.color.page else R.color.key), RoundedCornerShape(50)))
        }
    }
}
