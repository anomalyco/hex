package ly.anoma.hex.typing

import android.util.Log
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/** The swipe fixtures through the JNI entry points on the device, with bionic's libm ([SwipeParity],
 * [SwipeRules]): the models are the APK's own assets, opened from their descriptors as the IME does. */
@RunWith(AndroidJUnit4::class)
class SwipeParityDeviceTest {
    @Test
    fun readsEveryFixtureSwipeAsTheFixtureSays() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val app = instrumentation.targetContext.assets
        val result = instrumentation.context.assets.open("swipe-parity.jsonl").bufferedReader().useLines { lines ->
            SwipeParity.check(lines) { language ->
                val name = "typing/$language.hextm"
                val handle = app.openFd(name).use { KeyboardCore.modelOpenFd(it.parcelFileDescriptor.fd, it.startOffset, it.length) }
                SwipeParity.Model(handle, TypingModel.asset(app, language), app.open(name).use { it.readBytes() })
            }
        }
        Log.i("SwipeParity", "swipe parity (device): $result")
        assertTrue(result.cases > 0)
        assertEquals(result.toString(), 0, result.differences.size)
    }

    /** The swipe rules fixture on the device ([SwipeRules]). */
    @Test
    fun typesEveryFixtureSwipeAsTheFixtureSays() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val app = instrumentation.targetContext.assets
        val result = instrumentation.context.assets.open("swipe-rules.jsonl").bufferedReader().useLines { lines ->
            SwipeRules.check(lines) { language ->
                app.openFd("typing/$language.hextm").use { KeyboardCore.modelOpenFd(it.parcelFileDescriptor.fd, it.startOffset, it.length) }
            }
        }
        Log.i("SwipeParity", "swipe rules (device): $result")
        assertTrue(result.cases.values.sum() > 0)
        assertEquals(result.toString(), 0, result.differences.size)
    }
}
