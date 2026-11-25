package ly.anoma.hex.typing

import java.io.ByteArrayOutputStream
import java.io.File
import java.util.zip.ZipEntry
import java.util.zip.ZipOutputStream
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Labs › Keyboard › SwiftKey words (#2802): reading SwiftKey's exports and merging them into Hex's list. */
class ImportedWordsTest {
    private val portal = """{"terms": ["Zorbex", "hello", "https://example.com", "me@mail.com", "2024", "New York", "don’t", "x"]}"""

    private fun zip(vararg files: Pair<String, String>): ByteArray {
        val out = ByteArrayOutputStream()
        ZipOutputStream(out).use { zip ->
            for ((name, text) in files) { zip.putNextEntry(ZipEntry(name)); zip.write(text.toByteArray()); zip.closeEntry() }
        }
        return out.toByteArray()
    }

    @Test fun thePortalsJsonKeepsOnlySingleWords() {
        assertEquals(listOf("Zorbex", "hello", "don't"), ImportedWords.parse(portal.toByteArray()))
    }

    @Test fun thePortalsZipReadsItsWordList() {
        val bytes = zip("demographics.json" to """{"age": "25-34"}""", "services/sync_words.json" to portal)
        assertEquals(listOf("Zorbex", "hello", "don't"), ImportedWords.parse(bytes))
    }

    @Test fun aZipInsideTheExportGivesNoWords() {
        val inner = zip("services/sync_words.json" to portal)
        val out = ByteArrayOutputStream()
        ZipOutputStream(out).use { it.putNextEntry(ZipEntry("services/sync_words.json")); it.write(inner); it.closeEntry() }
        assertNull(ImportedWords.parse(out.toByteArray()))
    }

    @Test fun aPlainListDropsItsCounts() {
        assertEquals(listOf("Zorbex", "hello", "Wellington"), ImportedWords.parse("Zorbex\t12\nhello 3\r\nWellington\n".toByteArray()))
    }

    @Test fun aFileWithoutWordsReadsAsNone() {
        assertNull(ImportedWords.parse("""{"terms": []}""".toByteArray()))
        assertNull(ImportedWords.parse("""{"terms": [""".toByteArray()))
        assertNull(ImportedWords.parse(zip("demographics.json" to "{}")))
        assertNull(ImportedWords.parse("123 456".toByteArray()))
    }

    @Test fun mergingKeepsWhatIsThereAndAddsEachWordOnce() {
        val first = ImportedWords.merge(listOf("GitHub"), listOf("Hello", "zorbex", "hello", "github", "Zorbex"))
        assertEquals(listOf("GitHub", "hello", "zorbex"), first.words)
        assertEquals(2, first.added)
        assertEquals(1, first.already)
        val again = ImportedWords.merge(first.words, listOf("Hello", "zorbex", "hello", "github", "Zorbex"))
        assertEquals(first.words, again.words)
        assertEquals(0, again.added)
        assertEquals(3, again.already)
    }

    @Test fun aSwiftKeySizedListImportsAtOnce() {
        val words = (0 until 10_043).map { i -> "w" + Integer.toString(i, 26).map { 'a' + Character.digit(it, 26) }.joinToString("") }
        val result = ImportedWords.merge(emptyList(), words)
        assertEquals(10_043, result.added)
        assertEquals(0, result.full)
    }

    @Test fun aFullListLeavesTheRestOut() {
        val existing = (0 until ImportedWords.LIMIT - 1).map { "x$it" }
        val result = ImportedWords.merge(existing, listOf("alpha", "beta"))
        assertEquals(1, result.added)
        assertEquals(1, result.full)
    }

    @Test fun wordsTheModelHasAreLeftToIt() {
        val model = TypingModel.file(File("src/main/assets/typing/en.hextm"), "en")
        assertEquals(listOf("Zorbex"), PersonalWords.newTo(model, listOf("Zorbex", "hello", "The")))
    }
}
