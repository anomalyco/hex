package ly.anoma.hex

import org.junit.Assert.assertEquals
import org.junit.Test

class SpeechRequestsTest {
    @Test fun theKeyboardShowingSoonWaitsTheSettleTime() {
        assertEquals(SpeechRequests.SETTLE_MS, SpeechRequests.settleMs(0))
        assertEquals(SpeechRequests.SETTLE_MS, SpeechRequests.settleMs(120))
    }

    @Test fun theHandBackNeverWaitsPastItsLimit() {
        assertEquals(20L, SpeechRequests.settleMs(SpeechRequests.HAND_BACK_LIMIT_MS - 20))
        assertEquals(0L, SpeechRequests.settleMs(SpeechRequests.HAND_BACK_LIMIT_MS))
        assertEquals(0L, SpeechRequests.settleMs(SpeechRequests.HAND_BACK_LIMIT_MS + 300))
    }
}
