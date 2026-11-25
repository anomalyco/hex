package ly.anoma.hex.speech

import android.app.Activity
import android.app.Application
import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.os.Bundle
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.io.InputStream
import java.net.URL
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.security.MessageDigest
import javax.net.ssl.HttpsURLConnection

/** Whether the phone's network costs nothing per byte, as Android judges it: true on most Wi-Fi, false on mobile data or a
 * metered Wi-Fi (a phone's hotspot), null with no network. Android leaves the network's name and other location details
 * out of what it returns here; this is all Hex asks about the network (ACCESS_NETWORK_STATE). */
internal fun unmeteredNetwork(context: Context): Boolean? {
    val connectivity = context.getSystemService(ConnectivityManager::class.java) ?: return null
    return connectivity.getNetworkCapabilities(connectivity.activeNetwork ?: return null)?.let(::unmetered)
}
private fun unmetered(capabilities: NetworkCapabilities) = capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED) ||
    capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_TEMPORARILY_NOT_METERED)

/** A Wi-Fi-first download waits on a network that charges for data, unless the person chose mobile data for it. With
 * no network at all it goes ahead and fails as before, saying there's no connection. */
internal fun waitsForWifi(mobileDataChosen: Boolean, unmetered: Boolean?) = !mobileDataChosen && unmetered == false

/** A Hex screen in front of another app that must make no network request: a model download in progress does not
 * resume while it shows (Labs › Dictate from Quick Settings' sheet). */
internal interface OfflineScreen

/** Hex's network operation: an explicit, fixed model GET from Hex's own screens, each file pinned by revision, size and
 * SHA-256 ([small] from setup or Dictation settings, [english] from Labs). The keyboard never calls it. A wifiFirst
 * download waits for an unmetered network ([State.WaitingForWifi]) unless the person chooses mobile data. */
internal class ModelStore private constructor(private val name: String, private val size: Long, private val hash: String,
                                              private val url: String, private val extension: String,
                                              private val wifiFirst: Boolean = false) {
    companion object {
        /** Whisper small, the default speech model. */
        val small = ModelStore("whisper-small", 190085487L, "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small-q5_1.bin", "bin",
            wifiFirst = true)
        /** Labs › Faster English dictation: NVIDIA's Parakeet TDT-CTC 110M, as transcribe.cpp's authors converted it. */
        val english = ModelStore("parakeet-tdt_ctc-110m", 135373280L, "7dd44c74a331d788a4e5f8b16913b3feb29ced22cf5613aad0e0f6cd30516296",
            "https://huggingface.co/handy-computer/parakeet-tdt_ctc-110m-gguf/resolve/1c3f0d5378fdd358905cc410a93ed9e64f52d170/parakeet-tdt_ctc-110m-Q8_0.gguf",
            "gguf")
    }

    enum class Problem { Connection, Server, Space }
    sealed interface State {
        data object Available : State
        data object Ready : State
        data class Retry(val problem: Problem) : State
        data class Loading(val percent: Int) : State
        /** On a network that charges for data: it starts by itself on Wi-Fi, or now with [useMobileData]. */
        data object WaitingForWifi : State
    }
    private class Failure(val problem: Problem) : Exception()
    private class Deleted : Exception()
    private val progress = MutableStateFlow<State>(State.Available)
    val state = progress.asStateFlow()
    private var running = false
    private var waiting = false
    private var watching = false
    private var started = 0
    private var stops = 0
    /** Bumped by [delete], so a transfer in progress stops and leaves nothing behind. */
    @Volatile private var generation = 0
    /** The installed file's name, after its SHA-256. */
    internal val fileName get() = "$name-$hash.$extension"
    private fun path(context: Context) = File(context.noBackupFilesDir, fileName)
    private fun part(context: Context) = File(context.noBackupFilesDir, "$name-$hash.part")
    fun installed(context: Context) = installedIn(context.noBackupFilesDir)
    internal fun installedIn(dir: File) = File(dir, fileName).takeIf { it.isFile && it.length() == size }
    /** The download's size, said before it starts. */
    val megabytes get() = ((size + 500_000) / 1_000_000).toInt()
    /** The file checked against its SHA-256 in this process, so it is hashed once, not at every load. */
    @Volatile private var checked: File? = null

    /** [installed], once its contents match the pinned SHA-256 (hashed the first time in each process, on the caller's
     * thread). A file that doesn't is deleted, so its row offers the download again. */
    fun verified(context: Context) = verifiedIn(context.noBackupFilesDir)
    internal fun verifiedIn(dir: File): File? {
        val file = installedIn(dir) ?: return null
        if (checked == file) return file
        val digest = MessageDigest.getInstance("SHA-256")
        val ok = runCatching { file.inputStream().use { input -> pump(input) { buffer, count -> digest.update(buffer, 0, count) } } }.isSuccess &&
            digest.digest().joinToString("") { "%02x".format(it) } == hash
        if (ok) { checked = file; return file }
        synchronized(this) { file.delete(); if (!running) progress.value = State.Available }
        return null
    }

    /** Removes the file and any partial download, stopping a transfer in progress; [state] is Available again. */
    @Synchronized fun delete(context: Context) {
        val app = context.applicationContext
        generation++
        waiting = false
        checked = null
        path(app).delete(); part(app).delete()
        forgetMobileData(app)
        progress.value = State.Available
    }

    /** The person's choice to download on mobile data, kept until this file is installed or deleted, so a download that
     * resumes later doesn't wait for Wi-Fi again. */
    private fun choices(app: Context) = app.getSharedPreferences("models", Context.MODE_PRIVATE)
    private fun mobileDataChosen(app: Context) = choices(app).getBoolean("$name.mobileData", false)
    private fun forgetMobileData(app: Context) = choices(app).edit().remove("$name.mobileData").apply()

    /** Downloads now on mobile data too (the waiting row's action). */
    @Synchronized fun useMobileData(context: Context) {
        val app = context.applicationContext
        choices(app).edit().putBoolean("$name.mobileData", true).apply()
        download(app)
    }

    private var network: ConnectivityManager.NetworkCallback? = null
    /** While it waits for Wi-Fi, an unmetered network starts it if a Hex screen is in front (Android closes a backgrounded
     * app's connections; a screen coming to the front checks again). */
    private fun watchNetwork(app: Context) {
        if (network != null) return
        val connectivity = app.getSystemService(ConnectivityManager::class.java) ?: return
        network = object : ConnectivityManager.NetworkCallback() {
            override fun onCapabilitiesChanged(changed: Network, capabilities: NetworkCapabilities) {
                if (unmetered(capabilities)) synchronized(this@ModelStore) {
                    if (progress.value == State.WaitingForWifi && started > 0) download(app)
                }
            }
        }.also { connectivity.registerDefaultNetworkCallback(it) }
    }

    @Synchronized fun refresh(context: Context) {
        val app = context.applicationContext as Application
        if (!watching) { watching = true; app.registerActivityLifecycleCallbacks(foreground) }
        if (installed(app) != null) progress.value = State.Ready
        else if (progress.value == State.Ready) progress.value = State.Available
    }

    // Android closes a backgrounded app's connections, and a swipe from recents or a reboot ends the process.
    // A started download keeps its partial file and carries on whenever a Hex screen is in front again.
    private val foreground = object : Application.ActivityLifecycleCallbacks {
        override fun onActivityStarted(activity: Activity) {
            if (activity is OfflineScreen) return
            synchronized(this@ModelStore) {
                if (started++ == 0 && !running && (waiting || part(activity).exists()) && installed(activity) == null) download(activity)
            }
        }
        override fun onActivityStopped(activity: Activity) {
            if (activity is OfflineScreen) return
            synchronized(this@ModelStore) { if (--started == 0) stops++ }
        }
        override fun onActivityCreated(activity: Activity, state: Bundle?) = Unit
        override fun onActivityResumed(activity: Activity) = Unit
        override fun onActivityPaused(activity: Activity) = Unit
        override fun onActivitySaveInstanceState(activity: Activity, state: Bundle) = Unit
        override fun onActivityDestroyed(activity: Activity) = Unit
    }

    @Synchronized fun download(context: Context) {
        val app = context.applicationContext
        if (installed(app) != null) { progress.value = State.Ready; return }
        if (running) return
        if (wifiFirst && waitsForWifi(mobileDataChosen(app), unmeteredNetwork(app))) {
            waiting = true; progress.value = State.WaitingForWifi; watchNetwork(app); return
        }
        running = true; waiting = false
        val part = part(app)
        progress.value = State.Loading((part.length().coerceIn(0, size) * 100 / size).toInt().coerceAtMost(99))
        val since = stops
        val mine = generation
        Thread({
            val result = try {
                val resumed = part.length() > 0
                // A resumed file that fails the checksum is discarded and fetched once more from the start.
                if (transfer(app, part, mine) || resumed && transfer(app, part, mine)) State.Ready else State.Retry(Problem.Server)
            } catch (_: Deleted) { null }
            catch (e: Failure) { State.Retry(e.problem) }
            catch (_: IOException) { State.Retry(if (app.noBackupFilesDir.usableSpace < 8_000_000) Problem.Space else Problem.Connection) }
            catch (_: Exception) { State.Retry(Problem.Server) }
            synchronized(this) {
                running = false
                if (result == null || generation != mine) {
                    // Deleted while it ran: whatever the transfer wrote last goes too.
                    part.delete(); path(app).delete(); progress.value = State.Available
                // A Wi-Fi-first download whose Wi-Fi went away waits for it again rather than saying there's no connection.
                } else if (result == State.Retry(Problem.Connection) && (started == 0 || stops != since ||
                        wifiFirst && waitsForWifi(mobileDataChosen(app), unmeteredNetwork(app)))) {
                    waiting = true
                    if (started > 0) download(app)
                } else progress.value = result
            }
        }, "Hex $name download").start()
    }

    /** Returns false, after deleting the partial file, when the finished file fails the checksum. */
    private fun transfer(app: Context, part: File, mine: Int): Boolean {
        if (part.length() > size) part.delete()
        if (app.noBackupFilesDir.usableSpace <= size - part.length() + 8_000_000) throw Failure(Problem.Space)
        val digest = MessageDigest.getInstance("SHA-256")
        var total = part.length()
        if (total > 0) part.inputStream().use { input -> pump(input) { buffer, count -> digest.update(buffer, 0, count) } }
        var http = open(total)
        if (total > 0 && if (http == null) total < size else http.responseCode == 206 && !resumes(http, total)) {
            http?.disconnect(); part.delete(); total = 0; digest.reset()
            http = open(0) ?: throw Failure(Problem.Server)
        }
        if (http != null) try {
            if (http.responseCode == 200) { total = 0; digest.reset() }
            http.inputStream.use { input ->
                FileOutputStream(part, total > 0).use { output ->
                    pump(input) { buffer, count ->
                        if (generation != mine) throw Deleted()
                        total += count
                        if (total > size) throw Failure(Problem.Server)
                        digest.update(buffer, 0, count); output.write(buffer, 0, count)
                        progress.value = State.Loading((total * 100 / size).toInt().coerceAtMost(99))
                    }
                    output.fd.sync()
                }
            }
        } finally { http.disconnect() }
        if (total < size) throw IOException("Model download ended early")
        if (digest.digest().joinToString("") { "%02x".format(it) } != hash) { part.delete(); return false }
        synchronized(this) {
            if (generation != mine) throw Deleted()
            Files.move(part.toPath(), path(app).toPath(), StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
            forgetMobileData(app)
        }
        return true
    }

    private inline fun pump(input: InputStream, each: (ByteArray, Int) -> Unit) {
        val buffer = ByteArray(128 * 1024)
        while (true) {
            val count = input.read(buffer)
            if (count == -1) break
            each(buffer, count)
        }
    }

    private fun resumes(http: HttpsURLConnection, from: Long) =
        http.getHeaderField("Content-Range") == "bytes $from-${size - 1}/$size"

    /** Opens the model at [from] bytes; null when the server says nothing is left to send. */
    private fun open(from: Long): HttpsURLConnection? {
        var target = URL(url)
        repeat(6) {
            val host = target.host.lowercase()
            if (target.protocol != "https" || target.userInfo != null || target.port !in listOf(-1, 443) ||
                !(host == "huggingface.co" || host.endsWith(".cdn.hf.co") || host == "cas-bridge.xethub.hf.co")) throw Failure(Problem.Server)
            val http = target.openConnection() as HttpsURLConnection
            try {
                http.instanceFollowRedirects = false
                http.connectTimeout = 15_000; http.readTimeout = 45_000
                // Android's default user agent includes the phone model and OS build.
                http.setRequestProperty("User-Agent", "HexModelDownloader/1")
                http.setRequestProperty("Accept-Encoding", "identity")
                if (from > 0) http.setRequestProperty("Range", "bytes=$from-")
                when (http.responseCode) {
                    200, 206 -> return http
                    416 -> { http.disconnect(); return null }
                    301, 302, 303, 307, 308 -> target = URL(target, http.getHeaderField("Location") ?: throw Failure(Problem.Server))
                    else -> throw Failure(Problem.Server)
                }
            } catch (e: Exception) { http.disconnect(); throw e }
            http.disconnect()
        }
        throw Failure(Problem.Server)
    }
}
