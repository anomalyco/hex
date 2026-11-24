package ly.anoma.hex.keyboard

import android.text.InputType
import android.view.inputmethod.EditorInfo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class FieldTypingTest {
    private fun text(variation: Int = InputType.TYPE_TEXT_VARIATION_NORMAL, flags: Int = 0,
                     options: Int = EditorInfo.IME_ACTION_NONE) =
        FieldTyping.of(InputType.TYPE_CLASS_TEXT or variation or flags, options)

    @Test fun aPlainTextFieldGetsWordsAndAutocorrection() {
        val plain = text()
        assertTrue(plain.words)
        assertTrue(plain.corrects)
        assertFalse(plain.secure)
        assertTrue(plain.learns)
    }

    @Test fun noLearningKeepsWordsAndAutocorrectionButRemembersNothing() {
        // A browser's incognito tab: the flag says only that nothing typed there is learned.
        val private = text(options = EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING)
        assertTrue(private.words)
        assertTrue(private.corrects)
        assertFalse(private.secure)
        assertFalse(private.learns)
    }

    @Test fun addressesStayLiteral() {
        for (variation in listOf(InputType.TYPE_TEXT_VARIATION_URI, InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS,
            InputType.TYPE_TEXT_VARIATION_WEB_EMAIL_ADDRESS)) {
            assertFalse("variation $variation", text(variation).words)
            assertFalse("variation $variation", text(variation).corrects)
        }
        // The flag exempts none of them: an incognito tab's URL field is still a URL field.
        assertFalse(text(InputType.TYPE_TEXT_VARIATION_URI,
            options = EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING).words)
    }

    @Test fun fieldsThatAskForNoSuggestionsKeepWordsWithoutAutocorrection() {
        // A search box or filter: the app offers no suggestions of its own, but Hex still reads words there.
        val filtered = text(InputType.TYPE_TEXT_VARIATION_FILTER)
        assertTrue(filtered.words)
        assertFalse(filtered.corrects)
        for (flags in listOf(InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS, InputType.TYPE_TEXT_FLAG_AUTO_COMPLETE)) {
            val field = text(flags = flags)
            assertTrue("flags $flags", field.words)
            assertFalse("flags $flags", field.corrects)
        }
        // The Google app's search box: TEXT|MULTI_LINE|NO_SUGGESTIONS, whose suggestions and swipes Gboard keeps.
        val search = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE or
            InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
        assertEquals(0xa0001, search)
        val google = FieldTyping.of(search, 0)
        assertTrue(google.words)
        assertFalse(google.corrects)
    }

    @Test fun editorsThatKeepTheirOwnTextTakeKeyEvents() {
        // TYPE_NULL (a terminal): the field's text stays on its own side, so only key presses reach it.
        val terminal = FieldTyping.of(InputType.TYPE_NULL, 0)
        assertTrue(terminal.keyEvents)
        assertFalse(terminal.secure)
        assertFalse(terminal.words)
        assertFalse(terminal.corrects)
        // A flag only adds to the class: TYPE_NULL | MULTI_LINE is still TYPE_NULL.
        assertTrue(FieldTyping.of(InputType.TYPE_NULL or InputType.TYPE_TEXT_FLAG_MULTI_LINE, 0).keyEvents)
        assertFalse(text().keyEvents)
    }

    @Test fun passwordFieldsAreSecureAndNeverWords() {
        for (variation in listOf(InputType.TYPE_TEXT_VARIATION_PASSWORD, InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD,
            InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD)) {
            assertTrue("variation $variation", text(variation).secure)
            assertFalse("variation $variation", text(variation).words)
            assertFalse("variation $variation", text(variation).corrects)
        }
        val pin = FieldTyping.of(InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_VARIATION_PASSWORD, 0)
        assertTrue(pin.secure)
        assertFalse(pin.words)
        assertFalse(pin.corrects)
    }

    @Test fun numberFieldsAreNeverWords() {
        val number = FieldTyping.of(InputType.TYPE_CLASS_NUMBER, 0)
        assertFalse(number.secure)
        assertFalse(number.words)
        assertFalse(number.corrects)
    }
}
