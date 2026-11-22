package ly.anoma.hex.typing

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** The swipe parity fixture through the JNI entry points, on a host build of the core ([SwipeParity]). */
class SwipeParityTest {
    @Test
    fun readsEveryFixtureSwipeAsTheFixtureSays() {
        val result = File("../../crates/keyboard/core/tests/fixtures/swipe-parity.jsonl").useLines { lines ->
            SwipeParity.check(lines) { language ->
                val file = File("src/main/assets/typing/$language.hextm")
                SwipeParity.Model(KeyboardCore.modelOpen(file.path), TypingModel.file(file, language), file.readBytes())
            }
        }
        println("swipe parity (host): $result")
        assertTrue(result.cases > 0)
        assertEquals(result.toString(), 0, result.differences.size)
    }
}
