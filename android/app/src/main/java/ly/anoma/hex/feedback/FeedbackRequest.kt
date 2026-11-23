package ly.anoma.hex.feedback

import java.util.UUID

/** Send feedback: one POST to Bellhop, Anomaly's feedback inbox (https://blhp.app/docs/feedback).
 * Only the app sends it, only when the person taps Send; the keyboard has none of this.
 * Plain Kotlin, so the request and the retry rules run in unit tests. */
internal object FeedbackLimits {
    const val ENDPOINT = "https://api.blhp.app/v1/feedback"
    const val MESSAGE = 20_000
    const val SUBJECT = 200
    const val PICTURES = 5
    const val PICTURE_BYTES = 10 * 1024 * 1024
    const val PICTURES_BYTES = 20 * 1024 * 1024
    /** app, device and install_id text. */
    const val TEXT = 200
    /** A picture's longest side as sent (Bellhop's hard limit is 8,192 px), and its JPEG quality. */
    const val SIDE = 2048
    const val QUALITY = 80
}

/** The kind, sent as category; Other sends none. */
internal enum class FeedbackKind(val category: String?) { Problem("bug"), Idea("idea"), Question("question"), Other(null) }

/** What the person wrote and chose. Pictures are JPEGs, already shrunk. */
internal data class FeedbackDraft(val message: String, val subject: String = "", val kind: FeedbackKind = FeedbackKind.Other,
                                  val email: String = "", val pictures: List<ByteArray> = emptyList()) {
    enum class Problem { EmptyMessage, MessageTooLong, SubjectTooLong, TooManyPictures, PicturesTooLarge, InvalidEmail }

    /** What stops it being sent, checked before anything goes out. An email that isn't an address is refused
     * here, never sent as anonymous. */
    val problem: Problem? get() = when {
        message.isBlank() -> Problem.EmptyMessage
        message.trim().length > FeedbackLimits.MESSAGE -> Problem.MessageTooLong
        subject.trim().length > FeedbackLimits.SUBJECT -> Problem.SubjectTooLong
        pictures.size > FeedbackLimits.PICTURES -> Problem.TooManyPictures
        pictures.any { it.size > FeedbackLimits.PICTURE_BYTES } || pictures.sumOf { it.size } > FeedbackLimits.PICTURES_BYTES ->
            Problem.PicturesTooLarge
        email.isNotBlank() && !looksLikeEmail(email.trim()) -> Problem.InvalidEmail
        else -> null
    }
    /** With an email it isn't anonymous; without one it is. There is no separate switch. */
    val anonymous get() = email.isBlank()

    companion object {
        fun looksLikeEmail(text: String): Boolean {
            val at = text.indexOf('@')
            if (at <= 0 || at != text.lastIndexOf('@') || text.length > 254) return false
            val domain = text.substring(at + 1)
            return text.none { it.isWhitespace() || it.isISOControl() } && '.' in domain &&
                !domain.startsWith('.') && !domain.endsWith('.') && ".." !in domain
        }
    }
}

/** The app and phone details a request carries, read when Send is tapped. */
internal data class FeedbackContext(val version: String, val build: String, val os: String, val model: String,
                                    val locale: String, val metadata: Map<String, String>)

/** The request as Bellhop's checklist asks for it: a payload JSON part and up to five screenshot JPEG parts,
 * with an Idempotency-Key made when the person tapped Send and reused for every retry of it. */
internal class FeedbackRequest(draft: FeedbackDraft, context: FeedbackContext, installId: () -> String,
                               val idempotencyKey: String = UUID.randomUUID().toString(),
                               val boundary: String = "hex-feedback-" + UUID.randomUUID()) {
    val payload: String = json(payload(draft, context, installId))
    val pictures: List<Int> = draft.pictures.map { it.size }
    val contentType = "multipart/form-data; boundary=$boundary"
    val body: ByteArray = java.io.ByteArrayOutputStream().apply {
        fun line(s: String = "") = write((s + "\r\n").toByteArray())
        line("--$boundary")
        line("Content-Disposition: form-data; name=\"payload\"")
        line("Content-Type: application/json")
        line()
        write(payload.toByteArray())
        line()
        draft.pictures.forEachIndexed { n, jpeg ->
            line("--$boundary")
            line("Content-Disposition: form-data; name=\"screenshot\"; filename=\"hex-${n + 1}.jpg\"")
            line("Content-Type: image/jpeg")
            line()
            write(jpeg)
            line()
        }
        line("--$boundary--")
    }.toByteArray()

    /** Every header but Authorization, which carries the report key. */
    val headers get() = mapOf("Content-Type" to contentType, "Idempotency-Key" to idempotencyKey, "Accept" to "application/json")

    companion object {
        /** Nothing Hex promises stays on the phone (dictations, transcripts, recordings, vocabulary, learned
         * corrections), no logs and no user (Hex has no accounts). Without an email: anonymous, so no email and
         * no install id (which is made only for a send with an email). */
        fun payload(draft: FeedbackDraft, context: FeedbackContext, installId: () -> String): Map<String, Any> {
            fun clip(s: String) = s.take(FeedbackLimits.TEXT)
            val out = linkedMapOf<String, Any>()
            draft.subject.trim().takeIf { it.isNotEmpty() }?.let { out["subject"] = it }
            out["message"] = draft.message.trim()
            draft.kind.category?.let { out["category"] = it }
            if (draft.anonymous) out["anonymous"] = true
            else {
                out["email"] = draft.email.trim()
                out["install_id"] = installId()
            }
            out["app"] = linkedMapOf("name" to "Hex", "version" to clip(context.version), "build" to clip(context.build))
            out["device"] = linkedMapOf("platform" to "android", "os" to clip(context.os), "model" to clip(context.model),
                "locale" to clip(context.locale))
            out["metadata"] = context.metadata
            return out
        }

        fun json(value: Any?): String = StringBuilder().also { write(it, value) }.toString()
        private fun write(out: StringBuilder, value: Any?) {
            when (value) {
                null -> out.append("null")
                is Boolean, is Int, is Long -> out.append(value.toString())
                is String -> {
                    out.append('"')
                    for (c in value) when {
                        c == '"' -> out.append("\\\"")
                        c == '\\' -> out.append("\\\\")
                        c == '\n' -> out.append("\\n")
                        c == '\r' -> out.append("\\r")
                        c == '\t' -> out.append("\\t")
                        c < ' ' || c == ' ' || c == ' ' -> out.append("\\u%04x".format(c.code))
                        else -> out.append(c)
                    }
                    out.append('"')
                }
                is Map<*, *> -> {
                    out.append('{')
                    value.entries.forEachIndexed { i, (k, v) -> if (i > 0) out.append(','); write(out, k.toString()); out.append(':'); write(out, v) }
                    out.append('}')
                }
                else -> error("Unsupported JSON value: $value")
            }
        }
    }
}

/** What came back. */
internal sealed interface FeedbackOutcome {
    /** 201: the issue's number, and whether an email waits for the team's first reply. */
    data class Sent(val number: Int, val emailPending: Boolean) : FeedbackOutcome
    data class Failed(val failure: FeedbackFailure) : FeedbackOutcome
}

internal enum class FeedbackFailure {
    /** No connection, or it kept dropping: retried, then this. */
    Offline,
    /** 413: a picture or the whole is too large. */
    TooLarge,
    /** 415: a picture isn't a complete PNG, JPEG or WebP image. */
    UnsupportedImage,
    /** 429 with a long wait, or after its retries. */
    TryLater,
    /** 401, 403 feedback_disabled or feedback_unavailable: the app's setup, not the person's. Logged. */
    Unavailable,
    /** 400, 403 blocked, 422, 5xx after retries and anything else. */
    Other,
}

/** When to send again: network failures and 5xx a few times with backoff; 409 request_pending and 429 after
 * Retry-After; no other 4xx. Always the same body and Idempotency-Key. */
internal object FeedbackRetry {
    const val ATTEMPTS = 4
    /** A Retry-After longer than this isn't worth holding the form for. */
    private const val LONGEST_WAIT_MS = 60_000L

    /** Milliseconds to wait before the next attempt after attempt [attempt] (1-based) ended with [status]
     * (null: no answer), or null to stop. */
    fun delay(status: Int?, code: String?, retryAfter: String?, attempt: Int): Long? {
        if (attempt >= ATTEMPTS) return null
        val backoff = 1000L shl (attempt - 1)
        return when {
            status == null || status >= 500 -> backoff
            status == 409 && code == "request_pending" || status == 429 -> {
                val wait = retryAfter?.trim()?.toLongOrNull()?.times(1000) ?: backoff
                wait.takeIf { it <= LONGEST_WAIT_MS }?.coerceAtLeast(500)
            }
            else -> null
        }
    }

    fun failure(status: Int?, code: String?): FeedbackFailure = when (status) {
        null -> FeedbackFailure.Offline
        413 -> FeedbackFailure.TooLarge
        415 -> FeedbackFailure.UnsupportedImage
        429 -> FeedbackFailure.TryLater
        401 -> FeedbackFailure.Unavailable
        403 -> if (code == "blocked") FeedbackFailure.Other else FeedbackFailure.Unavailable
        else -> FeedbackFailure.Other
    }
}
