package ly.anoma.hex.updates

import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.net.ConnectivityManager
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.text.format.Formatter
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.LinearProgressIndicator
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
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import ly.anoma.hex.Action
import ly.anoma.hex.Card
import ly.anoma.hex.Entry
import ly.anoma.hex.Label
import ly.anoma.hex.MainActivity
import ly.anoma.hex.R
import ly.anoma.hex.Rule
import ly.anoma.hex.Toggle
import ly.anoma.hex.keyboard.StripNotice
import java.io.File
import java.lang.ref.WeakReference
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.concurrent.Executors

/** Hex Preview's own updates, from its channel on GitHub (Play builds have none: src/play). It looks when the
 * app opens and at most once a day from the keyboard; nothing downloads or installs until the person taps
 * Update, and then only an APK whose size, SHA-256 and signing key are right goes to Android's installer. */
internal object Updates {
    enum class Problem { Connection, Server, Space, Mismatch, Signature, Install }
    sealed interface State {
        data object Idle : State
        data object Checking : State
        data object Current : State
        data class CheckFailed(val problem: Problem) : State
        data class Available(val release: Release) : State
        /** On mobile data the size comes first. */
        data class Confirm(val release: Release) : State
        data class Downloading(val release: Release, val percent: Int) : State
        data class Verifying(val release: Release) : State
        /** Android 14+: downloaded and checked while Hex Preview was out of sight; Install waits for a tap. */
        data class Downloaded(val release: Release, val file: File) : State
        /** Android's one-time "Install unknown apps" switch for Hex Preview is off. */
        data class NeedsPermission(val release: Release, val file: File) : State
        data class Installing(val release: Release) : State
        data class Failed(val release: Release, val problem: Problem) : State
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
    /** Hex Preview's window, last resumed: an update downloaded by UpdateJob installs at once only while it shows. */
    private var window: WeakReference<LifecycleOwner>? = null

    private fun prefs(c: Context) = c.getSharedPreferences("updates", Context.MODE_PRIVATE)
    private fun automatic(c: Context) = prefs(c).getBoolean("automatic", true)
    private fun installed(c: Context) = c.packageManager.getPackageInfo(c.packageName, 0)
    private fun release(s: State) = when (s) {
        is State.Available -> s.release; is State.Confirm -> s.release; is State.Downloading -> s.release
        is State.Verifying -> s.release; is State.NeedsPermission -> s.release; is State.Installing -> s.release
        is State.Failed -> s.release; is State.Downloaded -> s.release; else -> null
    }
    private fun busy(s: State) = s is State.Confirm || s is State.Downloading || s is State.Verifying ||
        s is State.NeedsPermission || s is State.Installing || s is State.Downloaded

    @Synchronized private fun load(c: Context) {
        if (loaded) return
        loaded = true
        val saved = prefs(c).getString("release", null)?.let(Release::parse)
        if (saved != null && saved.code > installed(c).longVersionCode) {
            val file = File(File(c.cacheDir, "updates"), "${saved.code}.apk")
            val downloaded = prefs(c).getLong("downloaded", 0) == saved.code && file.length() == saved.size
            flow.value = if (downloaded) State.Downloaded(saved, file) else State.Available(saved)
        } else { prefs(c).edit().remove("release").apply(); worker.execute { File(c.cacheDir, "updates").deleteRecursively() } }
    }

    fun appOpened(context: Context) {
        val c = context.applicationContext
        (context as? LifecycleOwner)?.let { window = WeakReference(it) }
        load(c)
        c.getSystemService(NotificationManager::class.java).cancel(UpdateJob.READY)
        // Back from Android's "Install unknown apps" page: carry on with the update the person asked for.
        (flow.value as? State.NeedsPermission)?.let { if (c.packageManager.canRequestPackageInstalls()) install(c, it.release, it.file) }
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
            if (checking || busy(flow.value)) return
            checking = true
        }
        if (manual) flow.value = State.Checking
        worker.execute {
            val outcome = try { Result.success(Channel.latest(c)) } catch (e: Channel.Failure) { Result.failure(e) }
            prefs(c).edit().putLong("checked", System.currentTimeMillis()).apply()
            main.post {
                checking = false; checkedThisRun = true
                val r = outcome.getOrNull()
                val now = flow.value
                when {
                    busy(now) -> Unit
                    r != null && r.code > installed(c).longVersionCode && r.minSdk <= Build.VERSION.SDK_INT -> {
                        prefs(c).edit().putString("release", r.json()).apply()
                        if (now !is State.Failed || now.release.code != r.code) flow.value = State.Available(r)
                    }
                    r != null -> { prefs(c).edit().remove("release").apply(); flow.value = State.Current }
                    manual || now == State.Checking -> flow.value = State.CheckFailed((outcome.exceptionOrNull() as Channel.Failure).problem)
                }
                keyboard?.invoke(notice(c))
            }
        }
    }

    /** The person tapped Update (or Download after the mobile-data note, or Try again). */
    fun update(context: Context, confirmed: Boolean = false) {
        val c = context.applicationContext
        val r = release(flow.value) ?: return
        if (flow.value is State.Downloading || flow.value is State.Verifying || flow.value is State.Installing) return
        val network = c.getSystemService(ConnectivityManager::class.java)
        if (!confirmed && network.isActiveNetworkMetered) { flow.value = State.Confirm(r); return }
        flow.value = State.Downloading(r, 0)
        // Android 14+ cuts off a plain download seconds after the person leaves Hex Preview; a user-initiated job
        // keeps it going. With no network at all it runs here instead and stops at once, as before.
        if (Build.VERSION.SDK_INT >= 34 && network.activeNetwork != null && UpdateJob.schedule(c, r)) return
        fetch(c, r) { file -> file?.let { install(c, r, it) } }
    }
    fun notNow() { (flow.value as? State.Confirm)?.let { flow.value = State.Available(it.release) } }

    /** Downloads [r] and checks it (size, SHA-256, then package, build and signing key) on the worker, then hands
     * the file to [then] on the main thread, or null after a failure, which the Home card shows. [stop] ends the
     * download at its next read and deletes the partial file, as a lost connection does. */
    internal fun fetch(c: Context, r: Release, stop: () -> Boolean = { false }, progress: (Int) -> Unit = {}, then: (File?) -> Unit) {
        load(c)
        flow.value = State.Downloading(r, 0)
        worker.execute {
            val file = try {
                var shown = -1
                val file = Channel.download(c, r) {
                    if (stop()) throw Channel.Failure(Problem.Connection)
                    if (it != shown) { shown = it; flow.value = State.Downloading(r, it); progress(it) }
                }
                flow.value = State.Verifying(r)
                try { Channel.verify(c, file, r) } catch (e: Channel.Failure) { file.delete(); throw e }
                file
            } catch (e: Channel.Failure) { flow.value = State.Failed(r, e.problem); null }
            main.post { then(file) }
        }
    }

    /** UpdateJob's checked download: installs as an in-process one does while Hex Preview shows, else waits for
     * the Home card's Install. True when it waits. */
    internal fun ready(c: Context, r: Release, file: File): Boolean {
        if (window?.get()?.lifecycle?.currentState?.isAtLeast(Lifecycle.State.STARTED) == true) { install(c, r, file); return false }
        prefs(c).edit().putLong("downloaded", r.code).apply()
        flow.value = State.Downloaded(r, file)
        return true
    }

    /** The person tapped Install on an update downloaded out of sight: its size, SHA-256 and signing key are
     * checked again first. If Android cleared it from the cache meanwhile, it downloads again. */
    private fun installDownloaded(context: Context) {
        val c = context.applicationContext
        val s = flow.value as? State.Downloaded ?: return
        val r = s.release
        flow.value = State.Verifying(r)
        worker.execute {
            val file = Channel.downloaded(c, r)
            if (file == null) { main.post { flow.value = State.Available(r); update(c) }; return@execute }
            try { Channel.verify(c, file, r); main.post { install(c, r, file) } }
            catch (e: Channel.Failure) { file.delete(); flow.value = State.Failed(r, e.problem) }
        }
    }

    private fun install(c: Context, r: Release, file: File) {
        if (!c.packageManager.canRequestPackageInstalls()) { flow.value = State.NeedsPermission(r, file); return }
        flow.value = State.Installing(r)
        worker.execute {
            try {
                val installer = c.packageManager.packageInstaller
                val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL).apply {
                    setAppPackageName(c.packageName); setSize(file.length())
                    setInstallReason(PackageManager.INSTALL_REASON_USER)
                    // Android 12+: a later update by the app that installed the last one may go without its dialog.
                    setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED)
                }
                val id = installer.createSession(params)
                installer.openSession(id).use { session ->
                    file.inputStream().use { input ->
                        session.openWrite("hex-preview.apk", 0, file.length()).use { out -> input.copyTo(out); session.fsync(out) }
                    }
                    val result = Intent(c, UpdateReceiver::class.java).putExtra("code", r.code)
                    val flags = PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_MUTABLE
                    session.commit(PendingIntent.getBroadcast(c, id, result, flags).intentSender)
                }
                record(c, "committed")
            } catch (e: Exception) { record(c, "session failed"); flow.value = State.Failed(r, Problem.Install) }
        }
    }

    /** Android's answer for the session: its confirmation to show, or how it ended. */
    fun installStatus(c: Context, intent: Intent) {
        val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)
        record(c, "status $status " + intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE).orEmpty())
        when (status) {
            PackageInstaller.STATUS_PENDING_USER_ACTION -> {
                @Suppress("DEPRECATION")
                val confirm = if (Build.VERSION.SDK_INT >= 33) intent.getParcelableExtra(Intent.EXTRA_INTENT, Intent::class.java)
                    else intent.getParcelableExtra(Intent.EXTRA_INTENT)
                confirm?.let { c.startActivity(it.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }
            }
            PackageInstaller.STATUS_SUCCESS -> {
                prefs(c).edit().remove("release").remove("downloaded").apply(); flow.value = State.Current
            }
            else -> release(flow.value)?.let { flow.value = State.Failed(it, Problem.Install) }
        }
    }
    /** The last few installer outcomes, kept on the phone only, for anyone debugging an update. */
    private fun record(c: Context, line: String) {
        val p = prefs(c)
        val log = (p.getString("log", "").orEmpty().lines().filter { it.isNotBlank() } + "${System.currentTimeMillis()} $line").takeLast(10)
        p.edit().putString("log", log.joinToString("\n")).apply()
    }

    @Composable fun HomeCard() {
        val c = LocalContext.current
        val s = state.collectAsState().value
        val r = release(s) ?: return
        Card {
            Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Label(stringResource(R.string.updates_available, r.version, r.code), weight = FontWeight.SemiBold, lines = 2)
                Label(listOfNotNull(Formatter.formatShortFileSize(c, r.size), date(r.date)).joinToString(" · "), muted = true, size = 14)
                r.notes.forEach { Label(it, size = 14, lines = 4) }
                Spacer(Modifier.height(4.dp))
                when (s) {
                    is State.Available -> {
                        Primary(stringResource(R.string.updates_update)) { update(c) }
                        // Android closes an open keyboard while it replaces the app; Hex Preview stays the chosen one.
                        Label(stringResource(R.string.updates_keyboard_note), muted = true, size = 13, lines = 3)
                    }
                    is State.Confirm -> {
                        Label(stringResource(R.string.updates_metered, Formatter.formatShortFileSize(c, r.size)), size = 14, lines = 3)
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Box(Modifier.weight(1f)) { Primary(stringResource(R.string.updates_download)) { update(c, confirmed = true) } }
                            Action(stringResource(R.string.updates_not_now), ::notNow)
                        }
                    }
                    is State.Downloading -> {
                        Label(stringResource(R.string.updates_downloading, s.percent), muted = true, size = 14)
                        LinearProgressIndicator(progress = { s.percent / 100f }, modifier = Modifier.fillMaxWidth(),
                            color = colorResource(R.color.ink), trackColor = colorResource(R.color.rule))
                    }
                    is State.Downloaded -> {
                        Primary(stringResource(R.string.updates_install)) { installDownloaded(c) }
                        Label(stringResource(R.string.updates_keyboard_note), muted = true, size = 13, lines = 3)
                    }
                    is State.Verifying -> Label(stringResource(R.string.updates_verifying), muted = true, size = 14)
                    is State.Installing -> {
                        Label(stringResource(R.string.updates_installing), muted = true, size = 14)
                        Label(stringResource(R.string.updates_keyboard_note), muted = true, size = 13, lines = 3)
                    }
                    is State.NeedsPermission -> {
                        Label(stringResource(R.string.updates_permission), size = 14, lines = 6)
                        Primary(stringResource(R.string.updates_open_settings)) {
                            c.startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:" + c.packageName)))
                        }
                    }
                    is State.Failed -> {
                        Label(stringResource(problem(s.problem)), size = 14, lines = 3)
                        Primary(stringResource(R.string.updates_retry)) { update(c, confirmed = true) }
                    }
                    else -> Unit
                }
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
                    else -> stringResource(R.string.updates_ready)
                }, { checkNow(c) })
        }
    }

    private fun problem(p: Problem) = when (p) {
        Problem.Connection -> R.string.updates_failed_connection
        Problem.Server -> R.string.updates_failed_server
        Problem.Space -> R.string.updates_failed_space
        Problem.Mismatch -> R.string.updates_failed_mismatch
        Problem.Signature -> R.string.updates_failed_signature
        Problem.Install -> R.string.updates_failed_install
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

/** Where Android reports an update session's progress: the confirmation to show, or how it ended. */
class UpdateReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) = Updates.installStatus(context.applicationContext, intent)
}
