package ly.anoma.hex.feedback

import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL

/** Sends one [FeedbackRequest] with HttpURLConnection, reporting the upload's progress, and retries as
 * [FeedbackRetry] says with the same body and Idempotency-Key, so a retry never makes a second issue.
 * [log] gets the outcome of each attempt (never the key, the message or the email). */
internal class FeedbackSender(private val key: String, private val endpoint: String = FeedbackLimits.ENDPOINT,
                              private val sleep: (Long) -> Unit = Thread::sleep,
                              private val log: (String) -> Unit = {},
                              /** Debug builds only: the first answer is read, then lost as a dropped connection is. */
                              private val dropFirstResponse: Boolean = false) {
    /** [progress] gets the attempt (from 1) and how much of its body has gone out (0 to 1). */
    fun send(request: FeedbackRequest, progress: (attempt: Int, fraction: Float) -> Unit): FeedbackOutcome {
        var attempt = 0
        while (true) {
            attempt++
            progress(attempt, 0f)
            var status: Int? = null
            var code: String? = null
            var retryAfter: String? = null
            try {
                val answer = post(request) { progress(attempt, it) }
                log("attempt $attempt: ${answer.status} ${answer.body.take(300)}")
                if (dropFirstResponse && attempt == 1) throw IOException("first answer dropped (debug)")
                status = answer.status
                retryAfter = answer.retryAfter
                code = Regex("\"code\"\\s*:\\s*\"([a-z_]+)\"").find(answer.body)?.groupValues?.get(1)
                val number = Regex("\"number\"\\s*:\\s*(\\d+)").find(answer.body)?.groupValues?.get(1)?.toIntOrNull()
                if (status == 201 && number != null) {
                    progress(attempt, 1f)
                    return FeedbackOutcome.Sent(number, Regex("\"email\"\\s*:\\s*\"pending\"").containsMatchIn(answer.body))
                }
            } catch (e: IOException) {
                log("attempt $attempt: no answer (${e.javaClass.simpleName})")
            }
            val wait = FeedbackRetry.delay(status, code, retryAfter, attempt)
            if (wait != null) { sleep(wait); continue }
            return FeedbackOutcome.Failed(FeedbackRetry.failure(status, code))
        }
    }

    private class Answer(val status: Int, val retryAfter: String?, val body: String)

    private fun post(request: FeedbackRequest, progress: (Float) -> Unit): Answer {
        val connection = URL(endpoint).openConnection() as HttpURLConnection
        try {
            connection.requestMethod = "POST"
            connection.connectTimeout = 15_000
            connection.readTimeout = 60_000
            connection.doOutput = true
            connection.useCaches = false
            connection.instanceFollowRedirects = false
            connection.setRequestProperty("Authorization", "Bearer $key")
            request.headers.forEach { (name, value) -> connection.setRequestProperty(name, value) }
            // Streamed in fixed-length chunks, so the progress is what has actually gone out.
            connection.setFixedLengthStreamingMode(request.body.size)
            connection.outputStream.use { out ->
                var sent = 0
                while (sent < request.body.size) {
                    val n = minOf(16 * 1024, request.body.size - sent)
                    out.write(request.body, sent, n)
                    sent += n
                    progress(sent.toFloat() / request.body.size)
                }
            }
            // A streamed body can't be resent for authentication, so some HttpURLConnections report a 401 this way.
            val status = try { connection.responseCode } catch (e: java.net.HttpRetryException) { return Answer(e.responseCode(), null, "") }
            val stream = if (status >= 400) connection.errorStream else connection.inputStream
            val body = stream?.use { it.readBytes().decodeToString() }.orEmpty()
            return Answer(status, connection.getHeaderField("Retry-After"), body)
        } finally {
            connection.disconnect()
        }
    }
}
