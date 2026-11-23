package ly.anoma.hex.keyboard

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class DictationContinuationTest {
    @Test fun aSignedDictationAndTheOneAfterItStandAlone() {
        val continuation = DictationContinuation()
        assertFalse(continuation.joinable(signature = true))
        continuation.record(signature = true)
        assertFalse(continuation.joinable(signature = false))
        continuation.record(signature = false)
        assertTrue(continuation.joinable(signature = false))
    }
    @Test fun anEditNewFieldOrLeavingTheEditorEndsIt() {
        val continuation = DictationContinuation()
        continuation.record(signature = true)
        continuation.clear()
        assertTrue(continuation.joinable(signature = false))
    }
    @Test fun aDictationThatNeverWentInLeavesTheNextStandingAlone() {
        val continuation = DictationContinuation()
        continuation.record(signature = true)
        // joinable only reads: a cancelled dictation records nothing, so the next still stands alone.
        assertFalse(continuation.joinable(signature = false))
        assertFalse(continuation.joinable(signature = false))
    }
}
