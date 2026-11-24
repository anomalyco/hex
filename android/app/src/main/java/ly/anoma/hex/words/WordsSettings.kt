package ly.anoma.hex.words

import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.colorResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.FileProvider
import ly.anoma.hex.Action
import ly.anoma.hex.Card
import ly.anoma.hex.Entry
import ly.anoma.hex.Label
import ly.anoma.hex.R
import ly.anoma.hex.Rule
import ly.anoma.hex.Toggle
import ly.anoma.hex.inter
import ly.anoma.hex.speech.DictationFixStore
import ly.anoma.hex.speech.SpeechPreferences
import ly.anoma.hex.speech.WordReplacements
import java.io.File
import java.io.IOException

/** Settings › Dictation › Custom vocabulary › Word replacements: add and remove the person's own entries. Any
 * edit ends the last import's Undo. [revision] changes whenever the speech preferences do. */
@Composable
internal fun WordReplacementsPage(speech: SpeechPreferences, store: WordsStore, revision: Int) {
    val list = remember(revision) { speech.wordReplacements }
    var heard by remember { mutableStateOf("") }
    var write by remember { mutableStateOf("") }
    var problem by remember { mutableStateOf<WordReplacements.Problem?>(null) }
    Label(stringResource(R.string.words_replacements_intro), muted = true, size = 14, lines = Int.MAX_VALUE)
    Card {
        Field(heard, stringResource(R.string.words_heard_hint)) { heard = it; problem = null }
        Rule()
        Field(write, stringResource(R.string.words_write_hint), singleLine = false) { write = it; problem = null }
        Rule()
        Row(Modifier.fillMaxWidth().heightIn(min = 52.dp).padding(start = 16.dp, end = 4.dp),
            verticalAlignment = Alignment.CenterVertically) {
            Label(problem?.let { stringResource(problemText(it)) } ?: "", muted = true, size = 13, lines = 3, mod = Modifier.weight(1f))
            Action(stringResource(R.string.words_add)) {
                val entry = WordReplacements.cleaned(heard, write)
                problem = WordReplacements.problem(entry, list)
                if (problem == null) {
                    speech.wordReplacements = list + entry
                    store.edited()
                    heard = ""; write = ""
                }
            }
        }
    }
    if (list.isNotEmpty()) Card {
        list.forEachIndexed { index, entry ->
            if (index != 0) Rule()
            Row(Modifier.fillMaxWidth().heightIn(min = 52.dp).padding(start = 16.dp, end = 4.dp),
                verticalAlignment = Alignment.CenterVertically) {
                Label(entry.heard + " → " + entry.write, lines = 3, mod = Modifier.weight(1f))
                Action(stringResource(R.string.words_remove)) {
                    speech.wordReplacements = list.filterIndexed { i, _ -> i != index }
                    store.edited()
                }
            }
        }
    }
}

@Composable private fun Field(value: String, hint: String, singleLine: Boolean = true, change: (String) -> Unit) {
    val ink = colorResource(R.color.ink)
    BasicTextField(value = value, onValueChange = change, singleLine = singleLine, maxLines = if (singleLine) 1 else 4,
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 14.dp),
        textStyle = TextStyle(color = ink, fontFamily = inter, fontSize = 16.sp), cursorBrush = SolidColor(ink),
        decorationBox = { field -> Box { if (value.isEmpty()) Label(hint, muted = true); field() } })
}

private fun problemText(problem: WordReplacements.Problem) = when (problem) {
    WordReplacements.Problem.NoLetters -> R.string.words_problem_no_letters
    WordReplacements.Problem.Hidden -> R.string.words_problem_hidden
    WordReplacements.Problem.TooLong -> R.string.words_problem_too_long
    WordReplacements.Problem.TooManyWords -> R.string.words_problem_too_many_words
    WordReplacements.Problem.Blank -> R.string.words_problem_blank
    WordReplacements.Problem.Duplicate -> R.string.words_problem_duplicate
    WordReplacements.Problem.Full -> R.string.words_problem_full
}

/** Settings › Dictation › Custom vocabulary › Words file: open one, or export the terms and Word replacements
 * through the share sheet or Save to…, with the learned fixes in use only when the person includes them. */
@Composable
internal fun WordsFilePage(speech: SpeechPreferences, store: WordsStore, revision: Int) {
    val context = LocalContext.current
    var includeLearned by remember { mutableStateOf(false) }
    var status by remember { mutableStateOf<String?>(null) }
    val learnedInUse = remember(revision) { learned(context, speech).any { it.inUse } }
    val canUndo = remember(revision) { store.canUndo }
    val open = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) context.startActivity(Intent(context, WordsActivity::class.java).setData(uri)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION))
    }
    val res = context.resources
    fun exported(written: WordsWritten): String {
        val counts = res.getString(R.string.words_exported,
            res.getQuantityString(R.plurals.words_terms, written.terms, written.terms),
            res.getQuantityString(R.plurals.words_replacement_count, written.replacements, written.replacements))
        if (written.notExported.isEmpty()) return counts
        val why = written.notExported.map { it.reason }.distinct().joinToString(", ") { res.getString(skipped(it)) }
        return counts + " " + res.getString(R.string.words_left_out, written.notExported.size, why)
    }
    val save = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument(WordsWriter.MIME_TYPE)) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        val written = write(context, speech, includeLearned)
        status = try {
            context.contentResolver.openOutputStream(uri, "wt")?.use { it.write(written.bytes) } ?: throw IOException()
            exported(written)
        } catch (_: Exception) { res.getString(R.string.words_export_failed) }
    }
    Card {
        Entry(stringResource(R.string.words_open), stringResource(R.string.words_open_detail), action = { open.launch(arrayOf("*/*")) })
    }
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Label(stringResource(R.string.words_export), muted = true, size = 14, mod = Modifier.padding(start = 16.dp))
        Card {
            if (learnedInUse) {
                Toggle(stringResource(R.string.words_include_learned), stringResource(R.string.words_include_learned_detail),
                    includeLearned) { includeLearned = it }
                Rule()
            }
            Entry(stringResource(R.string.words_share), action = {
                val written = write(context, speech, includeLearned)
                status = try { share(context, written); exported(written) }
                    catch (_: Exception) { res.getString(R.string.words_export_failed) }
            })
            Rule()
            Entry(stringResource(R.string.words_save), action = { save.launch(WordsWriter.FILE_NAME) })
        }
        Label(status ?: stringResource(R.string.words_export_detail), muted = true, size = 13, lines = Int.MAX_VALUE,
            mod = Modifier.padding(horizontal = 16.dp))
    }
    if (canUndo) Card { Entry(stringResource(R.string.words_undo_last), action = { store.undo() }) }
}

/** Learned fixes as the export sees them: none unless Labs › Learn from my fixes is on, since only then are they
 * applied; each in use unless switched off in Settings. */
private fun learned(context: Context, speech: SpeechPreferences): List<LearnedFix> {
    if (!speech.learnFixes) return emptyList()
    val fixes = DictationFixStore(context)
    val off = fixes.disabled()
    return fixes.learned().map { LearnedFix(it.heard, it.meant, it.id !in off) }
}

private fun write(context: Context, speech: SpeechPreferences, includeLearned: Boolean): WordsWritten {
    val version = context.packageManager.getPackageInfo(context.packageName, 0).versionName.orEmpty()
    return WordsWriter.phone(version, speech.vocabularyTerms, speech.wordReplacements, learned(context, speech), includeLearned)
}

/** Writes the file atomically in Hex's cache (write, then rename) and offers it through the FileProvider. */
private fun share(context: Context, written: WordsWritten) {
    val folder = File(context.cacheDir, "words-out").apply { mkdirs() }
    val part = File(folder, WordsWriter.FILE_NAME + ".part")
    part.writeBytes(written.bytes)
    val file = File(folder, WordsWriter.FILE_NAME)
    if (!part.renameTo(file)) throw IOException()
    val uri: Uri = FileProvider.getUriForFile(context, context.packageName + ".words", file)
    val send = Intent(Intent.ACTION_SEND).setType(WordsWriter.MIME_TYPE).putExtra(Intent.EXTRA_STREAM, uri)
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    send.clipData = ClipData.newRawUri(null, uri)
    context.startActivity(Intent.createChooser(send, null))
}

/** Custom vocabulary's own rows for Word replacements, the words file and the last import's Undo. */
@Composable
internal fun WordsRows(speech: SpeechPreferences, store: WordsStore, revision: Int, on: Boolean,
                       replacements: () -> Unit, file: () -> Unit) {
    val count = remember(revision) { speech.wordReplacements.size }
    val canUndo = remember(revision) { store.canUndo }
    if (on) {
        Rule(); Entry(stringResource(R.string.words_replacements),
            if (count == 0) stringResource(R.string.words_replacements_none)
            else pluralStringResource(R.plurals.words_replacement_count, count, count), action = replacements)
        Rule(); Entry(stringResource(R.string.words_file), stringResource(R.string.words_file_detail), action = file)
    }
    if (canUndo) { Rule(); Entry(stringResource(R.string.words_undo_last), value = "", action = { store.undo() }) }
}
