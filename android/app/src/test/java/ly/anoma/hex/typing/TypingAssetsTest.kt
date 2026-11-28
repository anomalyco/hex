package ly.anoma.hex.typing

import java.io.File
import org.junit.Assert.assertTrue
import org.junit.Test

class TypingAssetsTest {
    /** TypingModel.asset maps every model file with openFd, which throws on a compressed asset, and the keyboard
     * then quietly types letters as tapped (#2998). Every file it maps must be stored uncompressed. */
    @Test fun everyMappedTypingFileIsStoredUncompressed() {
        val line = File("build.gradle.kts").readLines().single { "noCompress" in it }
        val stored = Regex("\"([a-z0-9]+)\"").findAll(line).map { it.groupValues[1] }.toSet()
        val read = setOf("never-suggest.tsv", "SHA256SUMS") // Streamed with open(), or not read at all.
        val mapped = File("src/main/assets/typing").listFiles().orEmpty().map { it.name }.filter { it !in read }
        assertTrue(mapped.isNotEmpty())
        for (name in mapped) assertTrue("$name would be compressed; add its extension to noCompress",
            name.substringAfterLast('.') in stored)
    }
}
