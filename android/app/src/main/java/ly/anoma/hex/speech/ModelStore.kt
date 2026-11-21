package ly.anoma.hex.speech

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.io.File
import java.net.URL
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.security.MessageDigest
import javax.net.ssl.HttpsURLConnection

/** The only network operation in Hex: an explicit, fixed model GET from setup or Dictation settings. */
internal object ModelStore {
    private const val size = 190085487L
    private const val hash = "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb"
    private const val url = "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small-q5_1.bin"

    sealed interface State {
        data object Available : State
        data object Ready : State
        data object Retry : State
        data class Loading(val percent: Int) : State
    }
    private val progress = MutableStateFlow<State>(State.Available)
    val state = progress.asStateFlow()
    private fun path(context: Context) = File(context.noBackupFilesDir, "whisper-small-$hash.bin")
    fun installed(context: Context) = path(context).takeIf { it.isFile && it.length() == size }
    fun refresh(context: Context) { if (installed(context) != null) progress.value = State.Ready }

    @Synchronized fun download(context: Context) {
        val app = context.applicationContext
        if (installed(app) != null) { progress.value = State.Ready; return }
        if (progress.value is State.Loading) return
        progress.value = State.Loading(0)
        Thread({
            val part = File(app.noBackupFilesDir, "whisper-small.part")
            val result = try {
                check(app.noBackupFilesDir.usableSpace > size - part.length() + 8_000_000)
                val connection = connect()
                try {
                    val digest = MessageDigest.getInstance("SHA-256")
                    var total = 0L
                    connection.inputStream.use { input ->
                        part.outputStream().use { output ->
                            val buffer = ByteArray(128 * 1024)
                            while (true) {
                                val count = input.read(buffer)
                                if (count == -1) break
                                total += count
                                check(total <= size)
                                digest.update(buffer, 0, count); output.write(buffer, 0, count)
                                progress.value = State.Loading((total * 100 / size).toInt().coerceAtMost(99))
                            }
                            output.fd.sync()
                        }
                    }
                    check(total == size && digest.digest().joinToString("") { "%02x".format(it) } == hash)
                    Files.move(part.toPath(), path(app).toPath(), StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
                    State.Ready
                } finally { connection.disconnect() }
            } catch (_: Exception) { State.Retry }
            finally { part.delete() }
            progress.value = result
        }, "Hex model download").start()
    }

    private fun connect(): HttpsURLConnection {
        var target = URL(url)
        repeat(6) {
            val host = target.host.lowercase()
            check(target.protocol == "https" && target.userInfo == null && target.port in listOf(-1, 443) &&
                (host == "huggingface.co" || host.endsWith(".cdn.hf.co") || host == "cas-bridge.xethub.hf.co"))
            val http = target.openConnection() as HttpsURLConnection
            try {
                http.instanceFollowRedirects = false
                http.connectTimeout = 15_000; http.readTimeout = 45_000
                // Android's default user agent includes the phone model and OS build.
                http.setRequestProperty("User-Agent", "HexModelDownloader/1")
                http.setRequestProperty("Accept-Encoding", "identity")
                when (http.responseCode) {
                    200 -> return http
                    301, 302, 303, 307, 308 -> target = URL(target, checkNotNull(http.getHeaderField("Location")))
                    else -> error("Model host unavailable")
                }
            } catch (e: Exception) { http.disconnect(); throw e }
            http.disconnect()
        }
        error("Unexpected model redirect")
    }
}
