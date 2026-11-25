package ly.anoma.hex.words

import android.net.Uri
import android.os.Handler
import android.os.Looper
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import java.text.NumberFormat
import kotlin.concurrent.thread
import ly.anoma.hex.Action
import ly.anoma.hex.Entry
import ly.anoma.hex.Label
import ly.anoma.hex.R
import ly.anoma.hex.Rule
import ly.anoma.hex.Toggle
import ly.anoma.hex.typing.ImportedWords
import ly.anoma.hex.typing.TypingPreferences

/** Labs › Keyboard › SwiftKey words (#2802), inside the Keyboard group's card: the toggle, Import words…
 * while it is on, and Remove imported words while any are kept, even with the toggle off. */
@Composable
internal fun ImportedWordsRows(typing: TypingPreferences, revision: Int) {
    val context = LocalContext.current
    val on = remember(revision) { typing.importedWords }
    // The count is saved with the list, so recomposing never reads the list itself.
    val kept = remember(revision) { ImportedWords.count(context) }
    var status by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    var removing by remember { mutableStateOf(false) }
    val res = context.resources
    val number = remember { NumberFormat.getIntegerInstance() }
    fun importFrom(uri: Uri) {
        busy = true; status = null
        val app = context.applicationContext
        val main = Handler(Looper.getMainLooper())
        thread(name = "hex-imported-words") {
            val result = try {
                val bytes = app.contentResolver.openInputStream(uri)?.use(ImportedWords::bounded)
                bytes?.let(ImportedWords::parse)?.let { ImportedWords.merge(ImportedWords.read(app), it) }
                    ?.also { if (it.added > 0) ImportedWords.save(app, it.words) }
            } catch (_: Exception) { null } catch (_: OutOfMemoryError) { null }
            val text = when {
                result == null -> res.getString(R.string.imported_words_none)
                else -> buildString {
                    append(res.getQuantityString(R.plurals.imported_words_added, result.added, number.format(result.added)))
                    if (result.already > 0)
                        append(" ").append(res.getQuantityString(R.plurals.imported_words_already, result.already, number.format(result.already)))
                    if (result.full > 0)
                        append(" ").append(res.getQuantityString(R.plurals.imported_words_full, result.full, number.format(result.full)))
                }
            }
            main.post { busy = false; status = text }
        }
    }
    val open = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri -> if (uri != null) importFrom(uri) }
    LaunchedEffect(on) { if (!on) status = null }
    Toggle(stringResource(R.string.imported_words), stringResource(R.string.imported_words_detail), on,
        noteLines = Int.MAX_VALUE) { typing.importedWords = it }
    if (on) {
        Rule()
        Entry(stringResource(R.string.imported_words_import),
            if (kept > 0) res.getQuantityString(R.plurals.imported_words_kept, kept, number.format(kept))
            else stringResource(R.string.imported_words_import_detail),
            value = if (busy) "…" else "›", action = { if (!busy) open.launch(arrayOf("*/*")) })
        // An import's result can run to three sentences: shown in full under the row, not cut to the note's two lines.
        status?.let {
            Label(it, size = 13, muted = true, lines = Int.MAX_VALUE,
                mod = Modifier.padding(start = 16.dp, end = 16.dp, bottom = 12.dp))
        }
    }
    if (kept > 0) {
        Rule()
        if (removing) {
            Row(Modifier.fillMaxWidth().heightIn(min = 52.dp).padding(start = 16.dp, end = 4.dp),
                verticalAlignment = Alignment.CenterVertically) {
                Label(stringResource(R.string.imported_words_remove_confirm), size = 14, lines = 2, mod = Modifier.weight(1f))
                Action(stringResource(R.string.learn_fixes_cancel)) { removing = false }
                Action(stringResource(R.string.imported_words_remove_action)) {
                    ImportedWords.clear(context); removing = false; status = null
                }
            }
        } else {
            Entry(stringResource(R.string.imported_words_remove), value = "", action = { removing = true })
        }
    }
}
