package ly.anoma.hex.typing

import java.io.File
import java.io.FileDescriptor
import java.io.FileInputStream
import org.junit.Assert.*
import org.junit.Test

/** The Rust core's JNI entry points, on a host build of it (build.gradle.kts, buildHostKeyboardCore). */
class KeyboardCoreTest {
    private val english = File("src/main/assets/typing/en.hextm")

    /** The descriptor's number, as Android's ParcelFileDescriptor.fd gives it. */
    private fun FileInputStream.number(): Int =
        FileDescriptor::class.java.getDeclaredField("fd").apply { isAccessible = true }.getInt(fd)

    @Test
    fun theAbiVersionIsTheHeaders() {
        val header = File("../../crates/keyboard/ffi/include/hex_keyboard.h").readText()
        val version = Regex("#define HK_ABI_VERSION (\\d+)").find(header)!!.groupValues[1].toInt()
        assertEquals(version, KeyboardCore.abiVersion())
    }

    @Test
    fun opensTheEnglishModelByPath() {
        val model = KeyboardCore.modelOpen(english.path)
        assertNotEquals(0L, model)
        assertEquals(86_247L, KeyboardCore.modelWordCount(model))
        KeyboardCore.modelFree(model)
        assertEquals(0L, KeyboardCore.modelOpen("src/main/assets/typing/missing.hextm"))
        assertEquals(0L, KeyboardCore.modelOpen("src/main/assets/typing/SHA256SUMS"))
        assertEquals(0L, KeyboardCore.modelWordCount(0))
        KeyboardCore.modelFree(0)
    }

    @Test
    fun opensTheEnglishModelFromADescriptor() {
        val model = FileInputStream(english).use { KeyboardCore.modelOpenFd(it.number(), 0, english.length()) }
        assertNotEquals(0L, model)
        assertEquals(86_247L, KeyboardCore.modelWordCount(model))
        KeyboardCore.modelFree(model)
        FileInputStream(english).use {
            assertEquals(0L, KeyboardCore.modelOpenFd(it.number(), 1, english.length()))
            assertEquals(0L, KeyboardCore.modelOpenFd(it.number(), 0, english.length() + 1))
        }
        assertEquals(0L, KeyboardCore.modelOpenFd(-1, 0, english.length()))
    }

    @Test
    fun aDecoderNeedsAModel() {
        assertEquals(0L, KeyboardCore.swipeNew(0, IntArray(0), IntArray(0), emptyArray(), DoubleArray(0), BooleanArray(0)))
        assertNull(KeyboardCore.swipeRead(0, DoubleArray(0), null, IntArray(0), DoubleArray(0), DoubleArray(0), 40.0, 50.0, null, 5))
        KeyboardCore.swipeFree(0)
    }

    /** A clean swipe of "hello" on the 390 pt iPhone's English keys (pitch 38.3 × 54 pt) reads "hello" first. */
    @Test
    fun readsASwipe() {
        val model = KeyboardCore.modelOpen(english.path)
        val decoder = KeyboardCore.swipeNew(model, IntArray(0), IntArray(0), arrayOf("hexkb"), doubleArrayOf(11.0), booleanArrayOf(false))
        KeyboardCore.modelFree(model) // the decoder keeps its own reference
        assertNotEquals(0L, decoder)
        val centres = buildMap {
            for ((letters, first, y) in listOf(Triple("qwertyuiop", 22.65, 67.5), Triple("asdfghjkl", 41.8, 121.5), Triple("zxcvbnm", 80.1, 175.5)))
                for ((i, c) in letters.withIndex()) put(c, first + 38.3 * i to y)
        }
        val stops = "hello".map { centres.getValue(it) }
        val points = buildList {
            for ((a, b) in stops.zipWithNext()) for (step in 0 until 8) {
                val t = step / 8.0
                add(a.first + (b.first - a.first) * t)
                add(a.second + (b.second - a.second) * t)
            }
            add(stops.last().first)
            add(stops.last().second)
        }.toDoubleArray()
        val letters = centres.keys.map { it.code }.toIntArray()
        val xs = centres.values.map { it.first }.toDoubleArray()
        val ys = centres.values.map { it.second }.toDoubleArray()
        val reading = KeyboardCore.swipeRead(decoder, points, null, letters, xs, ys, 38.3, 54.0, null, 5)!!
        assertEquals(reading.texts.contentToString(), "hello", reading.texts.first())
        assertTrue(reading.costs.toList().zipWithNext().all { (a, b) -> a <= b })
        assertEquals(reading.texts.size, reading.costs.size)
        assertTrue(reading.texts.size in 1..5 && reading.scored >= reading.texts.size)
        val times = DoubleArray(points.size / 2) { it * 0.01 }
        assertEquals("hello", KeyboardCore.swipeRead(decoder, points, times, letters, xs, ys, 38.3, 54.0, "say", 3)!!.texts.first())
        assertNull(KeyboardCore.swipeRead(decoder, points, DoubleArray(1), letters, xs, ys, 38.3, 54.0, null, 3))
        KeyboardCore.swipeFree(decoder)
    }
}
