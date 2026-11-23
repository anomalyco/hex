package ly.anoma.hex.typing

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** The swipe rules fixture through the JNI entry points, on a host build of the core ([SwipeRules]). */
class SwipeRulesTest {
    @Test
    fun typesEveryFixtureSwipeAsTheFixtureSays() {
        val result = File("../../crates/keyboard/core/tests/fixtures/swipe-rules.jsonl").useLines { lines ->
            SwipeRules.check(lines) { KeyboardCore.modelOpen("src/main/assets/typing/$it.hextm") }
        }
        println("swipe rules (host): $result")
        assertTrue(result.cases.values.sum() > 0)
        assertEquals(result.toString(), 0, result.differences.size)
    }
}
