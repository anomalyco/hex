package ly.anoma.hex.updates

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.text.format.Formatter
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.colorResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import ly.anoma.hex.Card
import ly.anoma.hex.Entry
import ly.anoma.hex.Label
import ly.anoma.hex.MainActivity
import ly.anoma.hex.R
import ly.anoma.hex.Rule
import ly.anoma.hex.Toggle
import ly.anoma.hex.keyboard.StripNotice
import java.io.File
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.concurrent.Executors

/** Hex Preview's update check, from its channel on GitHub (Play builds have none: src/play). It looks when the
 * app opens and at most once a day from the keyboard. Hex Preview installs nothing itself: Update opens the
 * release's APK on GitHub in the browser, which downloads it, and Android's own installer takes it from there,
 * asking the person and accepting only an APK signed with Hex Preview's key. */
internal object Updates {
    enum class Problem { Connection, Server }
    sealed interface State {
        data object Idle : State
        data object Checking : State
        data object Current : State
        data class CheckFailed(val problem: Problem) : State
        data class Available(val release: Release) : State
    }
    private val flow = MutableStateFlow<State>(State.Idle)
    val state = flow.asStateFlow()
    private val worker = Executors.newSingleThreadExecutor { Thread(it, "Hex Preview updates") }
    private val main = Handler(Looper.getMainLooper())
    private const val HOUR = 3_600_000L
    private const val DAY = 24 * HOUR
    private var loaded = false
    private var checking = false
    private var checkedThisRun = false
    /** The keyboard's field now: where a check's answer goes, unless it is a password field. */
    private var keyboard: ((StripNotice?) -> Unit)? = null

    private fun prefs(c: Context) = c.getSharedPreferences("updates", Context.MODE_PRIVATE)
    private fun automatic(c: Context) = prefs(c).getBoolean("automatic", true)
    private fun installed(c: Context) = c.packageManager.getPackageInfo(c.packageName, 0)

    @Synchronized private fun load(c: Context) {
        if (loaded) return
        loaded = true
        val saved = prefs(c).getString("release", null)?.let { Release.parse(it, c.getString(R.string.update_channel)) }
        if (saved != null && saved.code > installed(c).longVersionCode) flow.value = State.Available(saved)
        else prefs(c).edit().remove("release").apply()
        // Builds before the browser hand-off downloaded updates into the cache themselves.
        worker.execute { File(c.cacheDir, "updates").deleteRecursively() }
    }

    fun appOpened(context: Context) {
        val c = context.applicationContext
        load(c)
        if (automatic(c) && (!checkedThisRun || System.currentTimeMillis() - prefs(c).getLong("checked", 0) > HOUR)) check(c, false)
    }

    fun keyboardShown(context: Context, secure: Boolean, show: (StripNotice?) -> Unit) {
        if (secure) { keyboard = null; show(null); return }
        val c = context.applicationContext
        load(c)
        keyboard = show
        show(notice(c))
        if (automatic(c) && System.currentTimeMillis() - prefs(c).getLong("checked", 0) >= DAY) check(c, false)
    }

    private fun notice(c: Context): StripNotice? {
        val r = (flow.value as? State.Available)?.release ?: return null
        if (prefs(c).getLong("stripDismissed", 0) >= r.code) return null
        val gone = { prefs(c).edit().putLong("stripDismissed", r.code).apply() }
        return StripNotice(c.getString(R.string.updates_strip), c.getString(R.string.updates_strip_dismiss), tap = {
            gone()
            c.startActivity(Intent(c, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP))
        }, dismiss = gone)
    }

    fun checkNow(context: Context) = check(context.applicationContext, true)

    private fun check(c: Context, manual: Boolean) {
        synchronized(this) {
            if (checking) return
            checking = true
        }
        if (manual) flow.value = State.Checking
        worker.execute {
            val outcome = try { Result.success(Channel.latest(c)) } catch (e: Channel.Failure) { Result.failure(e) }
            prefs(c).edit().putLong("checked", System.currentTimeMillis()).apply()
            main.post {
                checking = false; checkedThisRun = true
                val r = outcome.getOrNull()
                when {
                    r != null && r.code > installed(c).longVersionCode && r.minSdk <= Build.VERSION.SDK_INT -> {
                        prefs(c).edit().putString("release", r.json()).apply()
                        flow.value = State.Available(r)
                    }
                    r != null -> { prefs(c).edit().remove("release").apply(); flow.value = State.Current }
                    manual || flow.value == State.Checking ->
                        flow.value = State.CheckFailed((outcome.exceptionOrNull() as Channel.Failure).problem)
                }
                keyboard?.invoke(notice(c))
            }
        }
    }

    /** The person tapped Update: the browser downloads the release's APK from GitHub (Release only accepts a GitHub
     * HTTPS address), and opening the download hands it to Android's installer. */
    fun update(context: Context) {
        val r = (flow.value as? State.Available)?.release ?: return
        val open = Intent(Intent.ACTION_VIEW, Uri.parse(r.url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        try { context.startActivity(open) } catch (e: ActivityNotFoundException) { }
    }

    @Composable fun HomeCard() {
        val c = LocalContext.current
        val r = (state.collectAsState().value as? State.Available)?.release ?: return
        Card {
            Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Label(stringResource(R.string.updates_available, r.version, r.code), weight = FontWeight.SemiBold, lines = 2)
                Label(listOfNotNull(Formatter.formatShortFileSize(c, r.size), date(r.date)).joinToString(" · "), muted = true, size = 14)
                r.notes.forEach { Label(it, size = 14, lines = 4) }
                Spacer(Modifier.height(4.dp))
                Primary(stringResource(R.string.updates_update)) { update(c) }
                Label(stringResource(R.string.updates_browser_note), muted = true, size = 13, lines = 5)
                // Android closes an open keyboard while it replaces the app; Hex Preview stays the chosen one.
                Label(stringResource(R.string.updates_keyboard_note), muted = true, size = 13, lines = 3)
            }
        }
    }

    @Composable fun SettingsCard() {
        val c = LocalContext.current
        val s = state.collectAsState().value
        var auto by remember { mutableStateOf(automatic(c)) }
        val info = remember { installed(c) }
        Card {
            Toggle(stringResource(R.string.updates_auto), stringResource(R.string.updates_auto_detail), auto) {
                auto = it; prefs(c).edit().putBoolean("automatic", it).apply()
            }
            Rule()
            Entry(stringResource(R.string.updates_check_now), stringResource(R.string.updates_installed, info.versionName.orEmpty().substringBefore("-preview."), info.longVersionCode),
                when (s) {
                    State.Checking -> stringResource(R.string.updates_checking)
                    State.Current -> stringResource(R.string.updates_current)
                    is State.CheckFailed -> stringResource(R.string.updates_check_failed)
                    State.Idle -> ""
                    is State.Available -> stringResource(R.string.updates_ready)
                }, { checkNow(c) })
        }
    }

    private fun date(iso: String) = runCatching {
        LocalDate.parse(iso.take(10)).format(DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM))
    }.getOrNull()

    @Composable private fun Primary(text: String, onClick: () -> Unit) {
        Box(Modifier.fillMaxWidth().height(48.dp).clip(RoundedCornerShape(12.dp)).background(colorResource(R.color.ink))
            .clickable(role = Role.Button, onClick = onClick), contentAlignment = Alignment.Center) {
            Label(text, inverted = true, weight = FontWeight.SemiBold)
        }
    }
}
