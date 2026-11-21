package ly.anoma.hex.speech

import java.util.Locale

/** The four iOS TranscriptFormatter styles, followed by no learned or remote rewriting. */
internal object SpokenText {
    fun format(raw: String, options: SpeechOptions): String {
        val language = if (options.translate) "en" else options.spoken.code
        val trimmed = spaces(raw)
        if (options.style == Style.Verbatim || trimmed.isEmpty()) return trimmed
        val input = if (options.clean) SpokenCleanup.clean(trimmed, language) else trimmed
        val sounds = SpokenCleanup.sounds.getValue(language)
        val text = spaces(input.replace(Regex("(?iu)(?:^|(?<=[\\s,]))(?:$sounds)(?=[\\s,.!?]|$),?\\s*"), " "))
        val spaced = text.replace(Regex("\\s+([,.;:!?])"), "$1").trimStart(',', ';', ':', ' ')
        if (spaced.isEmpty()) return ""
        return when (options.style) {
            Style.Polished -> buildString {
                var start = true
                for (c in spaced) {
                    append(if (start && c.isLetter()) c.uppercaseChar() else c)
                    if (c in ".!?\n") start = true
                    else if (!c.isWhitespace() && c !in "\"“(¿¡") start = false
                }
                if (last() !in ".!?…\"”)]") append('.')
            }
            Style.Casual -> spaced.lowercase(Locale.ROOT).let {
                if (it.endsWith('.') && it.dropLast(1).none { c -> c in ".!?" }) it.dropLast(1) else it
            }
            Style.Notes -> spaced.replace(Regex("(?i)\\bnew paragraph\\b[,.]?\\s*"), "\n\n")
                .replace(Regex("(?i)\\bnew line\\b[,.]?\\s*"), "\n")
                .replace(Regex("(?i)(?:^|\\s+)(?:bullet point|next bullet|bullet)\\b[:,-]?\\s*"), "\n- ")
                .split('\n').joinToString("\n") { line ->
                    val plain = spaces(line)
                    val first = plain.indexOfFirst(Char::isLetter)
                    if (first < 0) plain else plain.replaceRange(first, first + 1, plain[first].uppercase())
                }.trim()
            Style.Verbatim -> trimmed
        }
    }
    private fun spaces(s: String) = s.lines().joinToString("\n") { it.trim().replace(Regex("[ \\t]+"), " ") }.trim()
}
