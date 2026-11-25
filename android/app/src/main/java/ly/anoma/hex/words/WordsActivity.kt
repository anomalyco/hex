package ly.anoma.hex.words

import android.content.ContentResolver
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import ly.anoma.hex.HexTheme
import java.io.File
import kotlin.concurrent.thread

/** Where a Hex words file arrives: Android's VIEW (Files, a mail app) and SEND (the share sheet), and Settings'
 * Open a words file…. It reads the file through a private copy and shows the review sheet; nothing changes until
 * Add. No persistable permission is taken, and only a content address is read, never a file path, which could
 * name Hex's own files. */
class WordsActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val model = ViewModelProvider(this)[WordsModel::class.java]
        model.open(applicationContext, source(intent))
        setContent { HexTheme { WordsSheet(model, done = ::finish) } }
    }

    private fun source(intent: Intent): Uri? {
        val uri = if (intent.action == Intent.ACTION_SEND) {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
            else @Suppress("DEPRECATION") intent.getParcelableExtra(Intent.EXTRA_STREAM)
        } else intent.data
        return uri?.takeIf { it.scheme == ContentResolver.SCHEME_CONTENT }
    }
}

/** The review sheet's state, kept across rotation. Logs nothing about the file. */
internal class WordsModel : ViewModel() {
    sealed interface State {
        data object Reading : State
        /** [reason] is one of the reader's refusals, or "cantOpen" when the file couldn't be read at all. */
        data class Refused(val reason: String) : State
        data class Review(val read: WordsRead.Read, val before: WordsLists, val customVocabulary: Boolean) : State
        data class Added(val merged: WordsMerged) : State
        data object Undone : State
    }

    var state by mutableStateOf<State?>(null)
        private set
    var vocabulary by mutableStateOf(true)
    var replacements by mutableStateOf(true)
    /** The sheet's own Custom vocabulary switch, shown while it is off; off until the person turns it on. */
    var turnOn by mutableStateOf(false)
    var useTheirs by mutableStateOf(emptySet<String>())
    private var store: WordsStore? = null
    @Volatile private var copy: File? = null

    fun open(context: Context, uri: Uri?) {
        if (state != null) return
        state = State.Reading
        val lists = WordsStore(context)
        store = lists
        val main = Handler(Looper.getMainLooper())
        thread(name = "hex-words-read") {
            val result = try { if (uri == null) null else read(context, uri) } catch (_: Exception) { null }
            val next = when (result) {
                is WordsRead.Read -> State.Review(result, lists.lists(), lists.customVocabulary)
                is WordsRead.Refused -> State.Refused(result.reason)
                null -> State.Refused("cantOpen")
            }
            if (next !is State.Review) forget()
            main.post { state = next }
        }
    }

    /** Copies the file into Hex's private cache, at most 8 MiB and one byte, then reads the copy. */
    private fun read(context: Context, uri: Uri): WordsRead? {
        val folder = File(context.cacheDir, "words-in").apply { mkdirs() }
        // A copy left by a sheet the system closed without asking us.
        folder.listFiles()?.forEach { if (it.lastModified() < System.currentTimeMillis() - 86_400_000L) it.delete() }
        val file = File.createTempFile("words", ".tmp", folder)
        copy = file
        var size = 0
        val opened = context.contentResolver.openInputStream(uri) ?: return null
        opened.use { input ->
            file.outputStream().use { output ->
                val buffer = ByteArray(64 * 1024)
                while (true) {
                    val count = input.read(buffer, 0, minOf(buffer.size, WordsReader.MAX_BYTES + 1 - size))
                    if (count < 0) break
                    size += count
                    if (size > WordsReader.MAX_BYTES) return WordsRead.Refused("tooLarge")
                    output.write(buffer, 0, count)
                }
            }
        }
        return WordsReader.read(file.readBytes())
    }

    /** What Add would do with the person's choices, against the lists as they are now. */
    fun preview(review: State.Review) = WordsMerge.phone(review.before, review.read, vocabulary, replacements, useTheirs)

    fun add() {
        val review = state as? State.Review ?: return
        val store = store ?: return
        val before = store.lists()
        val merged = WordsMerge.phone(before, review.read, vocabulary, replacements, useTheirs)
        store.add(before, merged, turnOn && !review.customVocabulary)
        forget()
        state = State.Added(merged)
    }

    fun undo() {
        store?.undo()
        state = State.Undone
    }

    /** Deletes the private copy: after Add, Cancel or a refusal. */
    fun forget() { copy?.delete(); copy = null }

    override fun onCleared() = forget()
}
