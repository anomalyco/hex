package ly.anoma.hex.updates

import android.content.Context
import android.content.pm.PackageManager
import org.json.JSONArray
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.IOException
import java.io.InputStream
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import javax.net.ssl.HttpsURLConnection
import ly.anoma.hex.R

/** One published Hex Preview build, as latest.json describes it. */
internal data class Release(val code: Long, val name: String, val version: String, val url: String, val size: Long,
                            val sha256: String, val minSdk: Int, val date: String, val notes: List<String>) {
    fun json(): String = JSONObject().put("versionCode", code).put("versionName", name).put("version", version)
        .put("minSdk", minSdk).put("published", date).put("notes", JSONArray(notes))
        .put("apk", JSONObject().put("url", url).put("size", size).put("sha256", sha256)).toString()
    companion object {
        /** Null for anything that isn't a sane Hex Preview release: a GitHub HTTPS address, a SHA-256, a size. */
        fun parse(text: String): Release? = runCatching {
            val o = JSONObject(text)
            val apk = o.getJSONObject("apk")
            val notes = o.optJSONArray("notes")
            Release(o.getLong("versionCode"), o.getString("versionName"), o.optString("version", o.getString("versionName")),
                apk.getString("url"), apk.getLong("size"), apk.getString("sha256").lowercase(), o.optInt("minSdk", 31),
                o.optString("published"), List(minOf(notes?.length() ?: 0, 8)) { notes!!.getString(it).take(200) })
        }.getOrNull()?.takeIf {
            it.code > 0 && it.size in 1..400_000_000 && it.sha256.matches(Regex("[0-9a-f]{64}")) && allowed(URL(it.url))
        }
    }
}

/** What Hex Preview fetches: latest.json from its channel and the APK it names, only from GitHub over HTTPS.
 * The requests carry nothing about the person: no identifiers, no settings, a fixed user agent. */
internal object Channel {
    class Failure(val problem: Updates.Problem) : Exception()

    fun latest(context: Context): Release {
        val text = open(URL(context.getString(R.string.update_channel))).use { http ->
            http.inputStream.use { it.readLimited(64 * 1024) }
        }
        return Release.parse(text) ?: throw Failure(Updates.Problem.Server)
    }

    /** Downloads [release] into the cache, checking its size and SHA-256 as it arrives; anything else is deleted. */
    fun download(context: Context, release: Release, progress: (Int) -> Unit): File {
        val dir = File(context.cacheDir, "updates").apply { mkdirs() }
        val done = File(dir, "${release.code}.apk")
        dir.listFiles()?.forEach { if (it != done) it.delete() }
        if (done.length() == release.size && sha256(done) == release.sha256) return done
        val part = File(dir, "${release.code}.part")
        try {
            if (dir.usableSpace < release.size + 20_000_000) throw Failure(Updates.Problem.Space)
            val digest = MessageDigest.getInstance("SHA-256")
            var total = 0L
            open(URL(release.url)).use { http ->
                http.inputStream.use { input ->
                    part.outputStream().use { out ->
                        val buffer = ByteArray(128 * 1024)
                        while (true) {
                            val n = input.read(buffer)
                            if (n < 0) break
                            total += n
                            if (total > release.size) throw Failure(Updates.Problem.Mismatch)
                            digest.update(buffer, 0, n); out.write(buffer, 0, n)
                            progress((total * 100 / release.size).toInt())
                        }
                        out.fd.sync()
                    }
                }
            }
            if (total != release.size || hex(digest.digest()) != release.sha256) throw Failure(Updates.Problem.Mismatch)
            if (!part.renameTo(done)) throw Failure(Updates.Problem.Space)
            return done
        } catch (e: Failure) { part.delete(); throw e
        } catch (e: IOException) { part.delete(); throw Failure(Updates.Problem.Connection) }
    }

    /** The file [download] left for [release], if its size and SHA-256 still match; null if it's gone or changed. */
    fun downloaded(context: Context, release: Release): File? =
        File(File(context.cacheDir, "updates"), "${release.code}.apk").takeIf { it.length() == release.size && sha256(it) == release.sha256 }

    /** The APK must be Hex Preview itself, the build latest.json named, newer than what runs, and signed with
     * exactly the running app's key. */
    fun verify(context: Context, file: File, release: Release) {
        val pm = context.packageManager
        val flags = PackageManager.GET_SIGNING_CERTIFICATES
        val archive = pm.getPackageArchiveInfo(file.path, flags) ?: throw Failure(Updates.Problem.Mismatch)
        val running = pm.getPackageInfo(context.packageName, flags)
        if (archive.packageName != context.packageName || archive.longVersionCode != release.code ||
            archive.longVersionCode <= running.longVersionCode) throw Failure(Updates.Problem.Mismatch)
        val theirs = signers(archive.signingInfo)
        if (theirs.isEmpty() || theirs != signers(running.signingInfo)) throw Failure(Updates.Problem.Signature)
    }

    private fun signers(info: android.content.pm.SigningInfo?): Set<String> {
        info ?: return emptySet()
        val certs = if (info.hasMultipleSigners()) info.apkContentsSigners.toList() else info.signingCertificateHistory.orEmpty().takeLast(1)
        return certs.map { hex(MessageDigest.getInstance("SHA-256").digest(it.toByteArray())) }.toSet()
    }

    private fun sha256(file: File): String = file.inputStream().use { input ->
        val digest = MessageDigest.getInstance("SHA-256")
        val buffer = ByteArray(128 * 1024)
        while (true) { val n = input.read(buffer); if (n < 0) break; digest.update(buffer, 0, n) }
        hex(digest.digest())
    }
    private fun hex(bytes: ByteArray) = bytes.joinToString("") { "%02x".format(it) }

    private fun open(start: URL): HttpsURLConnection {
        var target = start
        repeat(6) {
            if (!allowed(target)) throw Failure(Updates.Problem.Server)
            val http = target.openConnection() as HttpsURLConnection
            try {
                http.instanceFollowRedirects = false
                http.connectTimeout = 15_000; http.readTimeout = 45_000
                http.useCaches = false
                // Android's default user agent names the phone and its build.
                http.setRequestProperty("User-Agent", "HexPreviewUpdater/1")
                http.setRequestProperty("Accept-Encoding", "identity")
                when (http.responseCode) {
                    200 -> return http
                    301, 302, 303, 307, 308 -> target = URL(target, http.getHeaderField("Location") ?: throw Failure(Updates.Problem.Server))
                    else -> throw Failure(Updates.Problem.Server)
                }
            } catch (e: IOException) { http.disconnect(); throw Failure(Updates.Problem.Connection)
            } catch (e: Failure) { http.disconnect(); throw e }
            http.disconnect()
        }
        throw Failure(Updates.Problem.Server)
    }

    private fun InputStream.readLimited(limit: Int): String {
        val out = ByteArrayOutputStream()
        val buffer = ByteArray(8192)
        while (true) {
            val n = read(buffer); if (n < 0) break
            out.write(buffer, 0, n); if (out.size() > limit) throw Failure(Updates.Problem.Server)
        }
        return out.toString("UTF-8")
    }
}

private val hosts = setOf("github.com", "raw.githubusercontent.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com")
private fun allowed(url: URL) = url.protocol == "https" && url.userInfo == null && url.port in listOf(-1, 443) && url.host.lowercase() in hosts

private inline fun <T : HttpURLConnection, R> T.use(block: (T) -> R): R = try { block(this) } finally { disconnect() }
