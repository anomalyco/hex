package ly.anoma.hex.speech

import java.util.Locale

/** French typography's spaces before ? ! ; : and inside « », the pipeline's last step in every
 * style but Verbatim (iOS FrenchSpacing). France: a narrow no-break space (U+202F) before ? ! ;
 * and a no-break space (U+00A0) before : and inside « ». Canada (OQLF): none before ? ! ;.
 * Switzerland: narrow ones inside « » too. Whatever space was there becomes exactly that one, and
 * only a mark that ends a word is spaced, so 10:30, URLs, addresses, emoticons, code in backticks
 * and a mark at a line's start stay as they are. */
internal object FrenchSpacing {
    enum class Convention { France, Canada, Switzerland }
    const val narrow = '\u202F'
    const val noBreak = '\u00A0'

    fun convention(language: String?): Convention? {
        val tag = Locale.forLanguageTag((language ?: return null).replace('_', '-'))
        if (tag.language != "fr") return null
        return when (tag.country) { "CA" -> Convention.Canada; "CH" -> Convention.Switzerland; else -> Convention.France }
    }

    fun apply(text: String, language: String?): String = convention(language)?.let { apply(text, it) } ?: text

    fun apply(text: String, convention: Convention): String {
        if (text.none { it in "?!;:«»" }) return text
        val question: Char? = if (convention == Convention.Canada) null else narrow
        val inside = if (convention == Convention.Switzerland) narrow else noBreak
        val out = StringBuilder(text.length + 8)
        val pending = StringBuilder()
        var code = false
        var afterOpening = false
        fun flush() { out.append(pending); pending.clear() }
        for ((i, c) in text.withIndex()) {
            val next = text.getOrNull(i + 1)
            if (c == '\u0060') { code = !code; afterOpening = false; flush(); out.append(c); continue }
            if (code) { flush(); out.append(c); continue }
            if (horizontalSpace(c)) { pending.append(c); continue }
            if (afterOpening) {
                afterOpening = false
                if (c != '\n' && c != '»') { pending.clear(); pending.append(inside) }
            }
            when (c) {
                '?', '!', ';', ':' -> {
                    val space = if (endsWord(c, next)) spaceBefore(c, out, pending, question) else null
                    if (space != null) { pending.clear(); space.value?.let(out::append) } else flush()
                    out.append(c)
                }
                '«' -> { flush(); out.append(c); afterOpening = true }
                '»' -> {
                    val last = out.lastOrNull()
                    if (last != null && last != '\n' && last != '«') { pending.clear(); pending.append(inside) }
                    flush(); out.append(c)
                }
                else -> { flush(); out.append(c) }
            }
        }
        if (!afterOpening) flush()
        return out.toString()
    }

    private fun horizontalSpace(c: Char) = c == ' ' || c == '\t' || c == noBreak || c == narrow || c == '\u2009'

    private fun endsWord(mark: Char, next: Char?): Boolean {
        if (next == null) return true
        if ((mark == ':' || mark == ';') && next in ")(][|") return false
        return next.isWhitespace() || next in "?!.…,»”\"’')]}"
    }

    private class Space(val value: Char?)

    /** The space to write before [mark] (a Space holding null for none), or null to leave what is there. */
    private fun spaceBefore(mark: Char, out: StringBuilder, pending: StringBuilder, question: Char?): Space? {
        val last = out.lastOrNull() ?: return null
        if (last == '\n' || last in "(«“[{") return null
        if (mark in "?!" && last in "?!") return Space(null)
        if (last in ";:") return null
        if (pending.isEmpty()) {
            val word = out.takeLastWhile { !it.isWhitespace() }.toString()
            if ("://" in word || '@' in word || word.lowercase(Locale.ROOT).startsWith("www.")) return null
        }
        return Space(if (mark == ':') noBreak else question)
    }
}
