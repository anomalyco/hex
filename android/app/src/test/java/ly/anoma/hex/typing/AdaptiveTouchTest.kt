package ly.anoma.hex.typing

import java.io.File
import kotlin.random.Random
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Labs › Keyboard › Adaptive touch: what teaches it, when the decoder reads your own centres, the grip fallback, the
 * table it keeps, and that it never corrects a word the drawn keys would leave alone. */
class AdaptiveTouchTest {
    private val rows = listOf("qwertyuiop", "asdfghjkl", "zxcvbnm")
    private val pitch = 39.5
    private val capWidth = pitch - 6
    private val capHeight = 45.0
    // The replay's keys (Replay.originalGeometry): keycaps 6 pt narrower than the pitch, touch cells tiling the rows.
    private val grid: KeyGrid = run {
        val centres = LinkedHashMap<Char, KeyPoint>()
        val caps = ArrayList<KeyRect>(); val cells = ArrayList<KeyRect>(); val row = ArrayList<Int>()
        for ((r, letters) in rows.withIndex()) for ((i, c) in letters.withIndex()) {
            val x = 201 + (i - (letters.length - 1) / 2.0) * pitch; val y = 66.5 + 56 * r
            centres[c] = KeyPoint(x, y)
            caps.add(KeyRect(x - capWidth / 2, y - capHeight / 2, x + capWidth / 2, y + capHeight / 2))
            cells.add(KeyRect(x - pitch / 2, y - 28, x + pitch / 2, y + 28))
            row.add(r)
        }
        val plane = TouchPlane("test", centres.keys.toCharArray(), caps.toTypedArray(), cells.toTypedArray(), row.toIntArray(), pitch, 56.0)
        KeyGrid(centres, pitch, 56.0, KeyRect(64.76, 212.0, 303.1, 257.0), plane)
    }
    private var now = 0.0
    private fun touch() = AdaptiveTouch().also { it.clock = { now } }
    private val words = "the quick brown fox jumps over a lazy dog while people type their words".split(' ')

    private fun AdaptiveTouch.tap(c: Char, dx: Double, dy: Double, aimed: Boolean = true) {
        val p = grid.centres.getValue(c)
        typed(c, p.x + dx * capWidth, p.y + dy * capHeight, grid, aimed)
        now += .2
    }
    // Types [typed] one word after another, each tapped [dx], [dy] keycaps off its keys' centres and left as typed.
    private fun AdaptiveTouch.type(typed: List<String>, dx: Double = 0.0, dy: Double = 0.0, before: String = ""): String {
        var text = before
        for (word in typed) {
            for (c in word) tap(c, dx, dy)
            text += word
            finish(word, text)
            text += " "
        }
        return text
    }
    private val AdaptiveTouch.weak get() = model.contexts.sumOf { it.weak }
    private val AdaptiveTouch.picked get() = model.contexts.sumOf { it.picked }
    private val AdaptiveTouch.strong get() = model.contexts.sumOf { it.strong }
    // Sessions of typing aimed [dx] right of every key until the decoder reads your own centres, or [limit] sessions.
    private fun AdaptiveTouch.learn(dx: Double, sessions: Int = 3) {
        repeat(sessions) {
            beginSession()
            repeat(12) { type(words, dx) }
        }
    }

    @Test fun aWordTeachesOnlyOnceTheNextShowsItStood() {
        val touch = touch()
        touch.type(listOf("hello"))
        assertEquals("a word waits for the next one", 0, touch.weak)
        touch.type(listOf("world"), before = "hello ")
        assertEquals(5, touch.weak)
        // The text changed elsewhere: "world" teaches nothing.
        touch.hostChanged("something else ")
        touch.type(listOf("again"), before = "something else ")
        assertEquals(5, touch.weak)
    }

    @Test fun anAutocorrectionTeachesNothingButAWordKeptOverItTeachesStrongly() {
        val touch = touch()
        for (c in "teh") touch.tap(c, 0.0, 0.0)
        touch.finish("teh", "teh", corrected = "the")
        touch.type(listOf("cat"), before = "the ")
        assertEquals("an autocorrection left in place can't confirm itself", 0, touch.weak + touch.strong)
        touch.forget()
        for (c in "teh") touch.tap(c, 0.0, 0.0)
        touch.finish("teh", "teh", corrected = "the")
        touch.undone("teh")
        touch.finish("teh", "teh")
        touch.type(listOf("dog"), before = "teh ")
        assertEquals("\"teh\", kept over its fix", 3, touch.strong)
    }

    @Test fun anErasedTapIsLabelledWithTheNeighbourThatReplacedIt() {
        val touch = touch()
        touch.tap('c', 0.0, 0.0); touch.tap('a', 0.0, 0.0); touch.tap('r', .3, 0.0)
        touch.willDelete("car")
        touch.tap('t', 0.0, 0.0)
        touch.finish("cat", "cat")
        touch.type(listOf("is"), before = "cat ")
        assertEquals("the r tap meant t", 1, touch.strong)
        assertEquals(3, touch.weak)
    }

    @Test fun aPickOfTheSameLengthTeachesTheLettersItChanged() {
        val touch = touch()
        for (c in "hwllo") touch.tap(c, 0.0, 0.0)
        touch.picked("hwllo", "hello", overCorrection = false, before = "hwllo")
        touch.type(listOf("there"), before = "hello ")
        assertEquals("the w tap meant e", 5, touch.picked)
        // A completion is no evidence of what each tap meant.
        touch.forget()
        for (c in "hel") touch.tap(c, 0.0, 0.0)
        touch.picked("hel", "hello", overCorrection = false, before = "hel")
        touch.type(listOf("you"), before = "hello ")
        assertEquals(5, touch.picked)
    }

    @Test fun aClaimedLetterAHeldAccentAndASlidTouchTeachNothing() {
        val touch = touch()
        // The likelier next letter claimed a tap inside d's cell for e: the label would be the keyboard's own guess.
        touch.tap('t', 0.0, 0.0); touch.tap('h', 0.0, 0.0)
        val d = grid.centres.getValue('d')
        touch.typed('e', d.x, d.y - 10, grid, true)
        touch.finish("the", "the")
        // A slide onto a key says nothing of where you aim.
        touch.tap('w', 0.0, 0.0, aimed = false); touch.tap('i', 0.0, 0.0); touch.tap('n', 0.0, 0.0)
        touch.finish("win", "the win")
        touch.type(listOf("it"), before = "the win ")
        assertEquals("t and h, then i and n", 2 + 2, touch.weak)
        // A letter that went in without a tap leaves its word out.
        touch.forget()
        touch.tap('c', 0.0, 0.0); touch.untapped(); touch.tap('t', 0.0, 0.0)
        touch.finish("cat", "cat")
        touch.type(listOf("is"), before = "cat ")
        assertEquals(4, touch.weak)
    }

    @Test fun theDecoderReadsYourCentresOnlyAfterFourHundredTapsOverThreeSessions() {
        val touch = touch()
        touch.learn(.2, sessions = 2)
        assertTrue(touch.model.contexts.maxOf { it.accepted } >= AdaptiveTouchModel.ACTIVE_TAPS)
        assertNull("two sessions", touch.personal(grid))
        touch.learn(.2, sessions = 1)
        val personal = touch.personal(grid)
        assertNotNull("three sessions", personal)
        val e = grid.centres.getValue('e'); val moved = personal!!.centres.getValue('e')
        assertTrue("toward where you land", moved.x - e.x > .1 * capWidth)
        assertTrue("never past the bound", moved.x - e.x <= AdaptiveTouchModel.BOUND * capWidth + 1e-6)
        assertEquals("the keycaps stay where they are", e, grid.centres.getValue('e'))
        // Taps on the centres: the drawn centres already fit, so nothing is in use.
        val accurate = touch()
        accurate.learn(0.0, sessions = 4)
        assertNull(accurate.personal(grid))
    }

    @Test fun aNewGripFallsBackToTheDrawnCentresAtOnceAndKeepsBothGrips() {
        val touch = touch()
        touch.learn(.25)
        assertNotNull(touch.personal(grid))
        touch.beginSession()
        touch.type(words.take(8), dx = -.25)
        assertNull("the centres fit worse than the drawn ones", touch.personal(grid))
        touch.learn(-.25, sessions = 4)
        val left = touch.personal(grid)
        assertNotNull("the new grip learned its own", left)
        assertTrue(left!!.centres.getValue('e').x < grid.centres.getValue('e').x)
        assertEquals(2, touch.model.contexts.count { it.geometry != 0L })
    }

    @Test fun theTableIsAlwaysOneSizeReadsBackAndRefusesADamagedOne() {
        val touch = touch()
        assertEquals(23_588, AdaptiveTouchModel.BYTES)
        assertEquals(AdaptiveTouchModel.BYTES, touch.model.serialized().size)
        touch.learn(.2)
        val bytes = touch.model.serialized()
        assertEquals("averages only, the same size whatever was learned", AdaptiveTouchModel.BYTES, bytes.size)
        val read = AdaptiveTouchModel.from(bytes)
        assertNotNull(read)
        assertTrue(read!!.serialized().contentEquals(bytes))
        assertNotNull("the next process reads your centres", AdaptiveTouch(read).personal(grid))
        assertNull(AdaptiveTouchModel.from(bytes.copyOf().also { it[100] = (it[100] + 1).toByte() }))
        assertNull(AdaptiveTouchModel.from(bytes.copyOf(bytes.size - 1)))
        assertNull(AdaptiveTouchModel.from(null))
        // Reset: everything goes at once.
        val version = touch.version
        touch.reset()
        assertTrue(touch.version > version)
        assertNull(touch.personal(grid))
        assertTrue(touch.model.contexts.all { it.geometry == 0L })
    }

    @Test fun eachKeyPlaneLearnsApart() {
        val touch = touch()
        touch.learn(.2)
        assertNotNull(touch.personal(grid))
        val plane = grid.plane!!
        val wider = TouchPlane("wider", plane.letters, plane.caps, plane.cells, plane.rows, plane.pitchX, plane.pitchY)
        val other = KeyGrid(grid.centres, grid.pitchX, grid.pitchY, grid.space, wider)
        assertNull("another keyboard width starts from the drawn centres", touch.personal(other))
        assertNotNull(touch.personal(grid))
    }

    // The keyboard's own decoder, with and without Adaptive touch.
    private fun word(adaptive: AdaptiveTouch?): TypingWord {
        val model = TypingModel.file(File("src/main/assets/typing/en.hextm"), "en")
        return TypingWord(SpatialDecoder(model), spatial = true).also { it.adaptive = adaptive }
    }
    // Each word typed with [jitter] pt of noise per tap, finished with a space: the strip, then what space did.
    private fun typing(adaptive: AdaptiveTouch?, text: String, jitter: Double, x: Double = 0.0): List<String> {
        val w = word(adaptive); val random = Random(2026)
        val out = ArrayList<String>(); var before = ""
        for (token in text.split(' ')) {
            w.reset(before)
            fun noise() = if (jitter > 0) random.nextDouble(-jitter, jitter) else 0.0
            for (c in token) {
                val p = grid.centres.getValue(c)
                w.letter(c.toString(), p.x + x * capWidth + noise(), p.y + noise(), grid)
            }
            out.add(w.strip().joinToString("|") { it.kind.name + ":" + it.text })
            val fix = w.finish(' ', KeyPoint(184.0, 234.5), grid)
            out.add(fix?.let { it.original + ">" + it.replacement } ?: "-")
            val typed = before + token
            before = (if (fix == null) typed else typed.dropLast(fix.original.length) + fix.replacement) + " "
        }
        return out
    }
    private val sentences = listOf("the quick brown fox jumps over the lazy dog", "i think we shoud meet tomorow at noon",
        "please send me the documnet when you are ready", "mary said she would call on monday")

    @Test fun untrainedItTypesExactlyAsWithItOff() {
        for (s in sentences) assertEquals(s, typing(null, s, 9.0), typing(touch(), s, 9.0))
    }

    @Test fun yourCentresNeverCorrectAWordTheDrawnCentresLeaveAlone() {
        // Centres pushed to the bound, right of every key, in use.
        val touch = touch()
        val plane = grid.plane!!
        val c = touch.model.ensureContext(plane.key, 0)
        touch.model.contexts[c].hasSnapshot = true
        touch.model.contexts[c].inUse = true
        for (s in plane.letters.indices) touch.model.snapshot[(c * AdaptiveTouchModel.SLOTS + s) * 2] = AdaptiveTouchModel.BOUND
        assertNotNull(touch.personal(grid))
        // Words typed right, on the drawn centres and on the aim your centres expect: whatever space does with them
        // without Adaptive touch, it does with it, and nothing more.
        for (aim in listOf(0.0, .3)) for (s in sentences) {
            val off = typing(null, s, 0.0, aim).filterIndexed { i, _ -> i % 2 == 1 }
            val on = typing(touch, s, 0.0, aim).filterIndexed { i, _ -> i % 2 == 1 }
            for ((i, a) in off.withIndex()) if (a == "-") assertEquals("$s at $aim, word $i", "-", on[i])
        }
    }
}
