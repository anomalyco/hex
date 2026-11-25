package ly.anoma.hex.words

import java.text.Normalizer
import java.util.Locale

/** The Hex words file's text rules (docs/specs/hex-words.md, "Cleaning text"), the same on every platform:
 * Unicode NFC, the exact White_Space set (never Java's own test, which leaves out U+00A0 and adds
 * U+001C–U+001F), code points rather than UTF-16 units, and a key lowercased with [Locale.ROOT] then with
 * every final sigma made σ. */
internal object WordsText {
    const val ELLIPSIS = "…"
    private const val shownLength = 64

    private fun white(cp: Int) = cp in 0x09..0x0D || cp == 0x20 || cp == 0x85 || cp == 0xA0 || cp == 0x1680 ||
        cp in 0x2000..0x200A || cp == 0x2028 || cp == 0x2029 || cp == 0x202F || cp == 0x205F || cp == 0x3000

    private fun control(cp: Int) = cp <= 0x1F || cp in 0x7F..0x9F

    private fun invisible(cp: Int) = cp == 0xAD || cp == 0x61C || cp == 0x180E || cp == 0x200B || cp == 0x200E ||
        cp == 0x200F || cp in 0x202A..0x202E || cp in 0x2060..0x2064 || cp in 0x2066..0x2069 || cp == 0xFEFF ||
        cp in 0xFFF9..0xFFFB

    /** A term, spelling, heard phrase or madeBy value: composed, every run of whitespace one space, none at
     * either end. */
    fun line(text: String): String {
        val composed = Normalizer.normalize(text, Normalizer.Form.NFC)
        val out = StringBuilder(composed.length)
        var gap = false
        var i = 0
        while (i < composed.length) {
            val cp = composed.codePointAt(i)
            i += Character.charCount(cp)
            if (white(cp)) { gap = true; continue }
            if (gap && out.isNotEmpty()) out.append(' ')
            gap = false
            out.appendCodePoint(cp)
        }
        return out.toString()
    }

    /** A replacement's output keeps its spaces: composed, and CR LF or a lone CR made LF. */
    fun output(text: String) = Normalizer.normalize(text, Normalizer.Form.NFC).replace("\r\n", "\n").replace('\r', '\n')

    /** Hidden characters in a cleaned line: any control character, or one of the invisible ones. */
    fun lineHidden(text: String) = text.codePoints().anyMatch { control(it) || invisible(it) }

    /** Hidden characters in a cleaned output, which may keep line feeds and tabs. */
    fun outputHidden(text: String) = text.codePoints().anyMatch { (control(it) && it != 0x0A && it != 0x09) || invisible(it) }

    /** At least one code point in general category L or N. */
    fun hasLetterOrDigit(text: String) = text.codePoints().anyMatch { letterOrDigit(it) }

    fun letterOrDigit(cp: Int) = when (Character.getType(cp).toByte()) {
        Character.UPPERCASE_LETTER, Character.LOWERCASE_LETTER, Character.TITLECASE_LETTER, Character.MODIFIER_LETTER,
        Character.OTHER_LETTER, Character.DECIMAL_DIGIT_NUMBER, Character.LETTER_NUMBER, Character.OTHER_NUMBER -> true
        else -> false
    }

    fun length(text: String) = text.codePointCount(0, text.length)

    /** Pieces between single spaces of a cleaned line. */
    fun words(text: String) = if (text.isEmpty()) 0 else text.count { it == ' ' } + 1

    /** The key two cleaned strings compare by: full, locale-independent lowercase, then σ for every ς. */
    fun key(text: String) = text.lowercase(Locale.ROOT).replace('ς', 'σ')

    /** Empty, or nothing but whitespace: a deletion, which phones can't keep yet. */
    fun blank(text: String) = text.codePoints().allMatch(::white)

    /** A phone's term key: composed, lowercased as [key], letters and digits only, so "Next.js" is "NextJS". */
    fun termKey(text: String): String {
        val lowered = key(Normalizer.normalize(text, Normalizer.Form.NFC))
        val out = StringBuilder(lowered.length)
        lowered.codePoints().forEach { if (letterOrDigit(it)) out.appendCodePoint(it) }
        return out.toString()
    }

    /** A file's string for the screen: its first 64 code points and "…" when longer, in its own direction-isolated
     * run (FSI … PDI), so right-to-left text can't reorder the words around it. */
    fun shown(text: String): String {
        val cut = if (length(text) > shownLength) text.substring(0, text.offsetByCodePoints(0, shownLength)) + ELLIPSIS else text
        return "\u2068" + cut + "\u2069"
    }
}
