package ly.anoma.hex.feedback

import android.app.Activity
import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.ImageDecoder
import android.graphics.Paint
import android.graphics.RectF
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.PixelCopy
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import java.io.ByteArrayOutputStream
import java.util.UUID
import kotlin.concurrent.thread
import kotlin.math.max
import kotlin.math.roundToInt
import ly.anoma.hex.BuildConfig
import ly.anoma.hex.speech.ModelStore
import ly.anoma.hex.speech.SpeechPreferences

/** The feedback form's state, kept for the process so turning the phone mid-send loses nothing. The form is
 * offered only in a build with a report key (Gradle property hexFeedbackKey, surfaced as BuildConfig.FEEDBACK_KEY). */
internal object Feedback {
    private const val TAG = "HexFeedback"
    val available get() = BuildConfig.FEEDBACK_KEY.isNotEmpty()
    /** Debug builds only, from the launch intent: the first answer is lost, to show a retry gets the same issue. */
    var dropFirstResponse = false

    class Picture(val jpeg: ByteArray, val thumbnail: ImageBitmap, val width: Int, val height: Int, val window: Boolean)
    sealed interface Phase {
        data object Editing : Phase
        data class Sending(val attempt: Int, val fraction: Float) : Phase
        data class Sent(val number: Int, val emailPending: Boolean, val email: String) : Phase
        data class Failed(val failure: FeedbackFailure) : Phase
    }

    var kind by mutableStateOf(FeedbackKind.Other)
    var subject by mutableStateOf("")
    var message by mutableStateOf("")
    var email by mutableStateOf("")
    val pictures = mutableStateListOf<Picture>()
    var phase by mutableStateOf<Phase>(Phase.Editing)
    /** Send was tapped: from then on the form says what stops it. */
    var tried by mutableStateOf(false)
    /** Where the form was opened, for the team (metadata.screen). */
    private var screen = "Home"
    private val main = Handler(Looper.getMainLooper())

    val draft get() = FeedbackDraft(message, subject, kind, email, pictures.map { it.jpeg })
    val sending get() = phase is Phase.Sending

    /** A new form, with Hex's own window as it was just now when there is one. */
    fun start(screen: String, window: Picture?) {
        this.screen = screen
        kind = FeedbackKind.Other; subject = ""; message = ""; email = ""; tried = false
        pictures.clear(); window?.let(pictures::add)
        phase = Phase.Editing
    }
    /** Leaving the form drops it, unless it is still sending. */
    fun close() { if (!sending) start(screen, null) }
    /** An edit after a failed send clears its message; the next Send is a new send. */
    fun edited() { if (phase is Phase.Failed) phase = Phase.Editing }

    fun send(context: Context) {
        tried = true
        val draft = draft
        if (sending || draft.problem != null) return
        // The Idempotency-Key is made here, at the tap, and every retry of this send reuses it.
        val request = FeedbackRequest(draft, details(context), installId = { installId(context) })
        if (BuildConfig.DEBUG) Log.d(TAG, describe(request))
        val email = draft.email.trim()
        phase = Phase.Sending(1, 0f)
        val sender = FeedbackSender(BuildConfig.FEEDBACK_KEY, log = { if (BuildConfig.DEBUG) Log.d(TAG, it) },
            dropFirstResponse = BuildConfig.DEBUG && dropFirstResponse)
        thread(name = "hex-feedback") {
            val outcome = sender.send(request) { attempt, fraction ->
                main.post { if (sending) phase = Phase.Sending(attempt, fraction) }
            }
            if (outcome is FeedbackOutcome.Failed && outcome.failure == FeedbackFailure.Unavailable)
                // The report key's setup (revoked, or not allowed to send feedback): the team's to fix, not the person's.
                Log.w(TAG, "Bellhop refused the report key; feedback is unavailable in this build")
            if (BuildConfig.DEBUG) Log.d(TAG, "outcome: $outcome")
            main.post {
                phase = when (outcome) {
                    is FeedbackOutcome.Sent -> Phase.Sent(outcome.number, outcome.emailPending, email)
                    is FeedbackOutcome.Failed -> Phase.Failed(outcome.failure)
                }
            }
        }
    }

    /** The app and phone, read at the tap. Nothing identifying in metadata: it is kept even when anonymous. */
    private fun details(context: Context): FeedbackContext {
        val info = context.packageManager.getPackageInfo(context.packageName, 0)
        val maker = Build.MANUFACTURER.orEmpty()
        val model = Build.MODEL.orEmpty().let { if (it.startsWith(maker, ignoreCase = true)) it else "$maker $it".trim() }
        val metadata = linkedMapOf(
            "screen" to screen,
            "channel" to if (BuildConfig.BUILD_TYPE == "preview") "preview" else "play",
            "speech_model" to (if (ModelStore.small.installed(context) != null) "whisper-small-q5_1" else "whisper-tiny-q5_1") +
                // Labs › Faster English dictation, when it is on and downloaded.
                if (SpeechPreferences(context).fastEnglish && ModelStore.english.installed(context) != null) "+parakeet-tdt_ctc-110m-q8_0" else "",
            "dictation_language" to SpeechPreferences(context).spoken.code)
        if (BuildConfig.DEBUG) metadata["build_type"] = BuildConfig.BUILD_TYPE
        return FeedbackContext(info.versionName.orEmpty(), info.longVersionCode.toString(), "Android ${Build.VERSION.RELEASE}",
            model, context.resources.configuration.locales[0].toLanguageTag(), metadata)
    }

    /** A random UUID in Hex's own preferences, made the first time a send with an email needs it. Never a
     * hardware or advertising id; not backed up (allowBackup is off), and uninstalling Hex removes it. */
    private fun installId(context: Context): String {
        val prefs = context.getSharedPreferences("feedback", Context.MODE_PRIVATE)
        prefs.getString("install_id", null)?.let { return it }
        return UUID.randomUUID().toString().also { prefs.edit().putString("install_id", it).apply() }
    }

    /** Debug builds: what goes out, without the report key. */
    private fun describe(request: FeedbackRequest) = buildString {
        appendLine("POST ${FeedbackLimits.ENDPOINT} (${request.body.size} bytes)")
        appendLine("Authorization: Bearer <report key>")
        request.headers.forEach { (name, value) -> appendLine("$name: $value") }
        appendLine("part payload (application/json, ${request.payload.toByteArray().size} bytes): ${request.payload}")
        pictures.forEachIndexed { n, it -> appendLine("part screenshot hex-${n + 1}.jpg (image/jpeg, ${it.jpeg.size} bytes, ${it.width}×${it.height}${if (it.window) ", Hex's window" else ""})") }
    }

    /** Hex's own window, drawn just before the form opens: never the whole screen, the keyboard or another app.
     * [hide] is painted over with an empty card first (Home's dictation result), so no transcript leaves the phone. */
    fun captureWindow(activity: Activity, hide: RectF?, hideRadius: Float, done: (Picture?) -> Unit) {
        val view = activity.window.decorView
        if (view.width <= 0 || view.height <= 0) return done(null)
        val bitmap = Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888)
        val sheet = activity.getColor(ly.anoma.hex.R.color.sheet)
        PixelCopy.request(activity.window, bitmap, { result ->
            if (result != PixelCopy.SUCCESS) { bitmap.recycle(); return@request done(null) }
            if (hide != null) Canvas(bitmap).drawRoundRect(hide, hideRadius, hideRadius, Paint().apply { color = sheet })
            thread(name = "hex-feedback-picture") {
                val picture = picture(bitmap, window = true)
                main.post { done(picture) }
            }
        }, main)
    }

    /** Photos from Android's photo picker (no permission), each re-encoded like the window's picture: HEIC and
     * anything else Android decodes becomes a JPEG. Calls [done] with how many couldn't be read. */
    fun addPicked(context: Context, uris: List<Uri>, done: (unreadable: Int) -> Unit) {
        val room = FeedbackLimits.PICTURES - pictures.size
        val resolver = context.applicationContext.contentResolver
        thread(name = "hex-feedback-picked") {
            var unreadable = 0
            val read = uris.take(room).mapNotNull { uri ->
                runCatching {
                    ImageDecoder.decodeBitmap(ImageDecoder.createSource(resolver, uri)) { decoder, info, _ ->
                        decoder.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
                        val scale = minOf(1f, FeedbackLimits.SIDE.toFloat() / max(info.size.width, info.size.height))
                        if (scale < 1f) decoder.setTargetSize((info.size.width * scale).roundToInt().coerceAtLeast(1),
                            (info.size.height * scale).roundToInt().coerceAtLeast(1))
                    }
                }.getOrNull()?.let { picture(it, window = false) } ?: run { unreadable++; null }
            }
            main.post {
                read.take(FeedbackLimits.PICTURES - pictures.size).forEach(pictures::add)
                edited(); done(unreadable)
            }
        }
    }

    /** JPEG at about 80%, the longest side at most 2048 px; transparency over white. */
    private fun picture(source: Bitmap, window: Boolean): Picture? {
        val scale = minOf(1f, FeedbackLimits.SIDE.toFloat() / max(source.width, source.height))
        val width = (source.width * scale).roundToInt().coerceAtLeast(1)
        val height = (source.height * scale).roundToInt().coerceAtLeast(1)
        val flat = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
        Canvas(flat).apply {
            drawColor(Color.WHITE)
            drawBitmap(source, null, android.graphics.Rect(0, 0, width, height), Paint(Paint.FILTER_BITMAP_FLAG))
        }
        source.recycle()
        val jpeg = ByteArrayOutputStream().use { out ->
            if (!flat.compress(Bitmap.CompressFormat.JPEG, FeedbackLimits.QUALITY, out)) return null
            out.toByteArray()
        }
        flat.recycle()
        val thumbnail = BitmapFactory.decodeByteArray(jpeg, 0, jpeg.size, BitmapFactory.Options().apply {
            inSampleSize = max(1, max(width, height) / 480)
        }) ?: return null
        return Picture(jpeg, thumbnail.asImageBitmap(), width, height, window)
    }
}
