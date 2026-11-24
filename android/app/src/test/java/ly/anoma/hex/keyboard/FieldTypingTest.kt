package ly.anoma.hex.keyboard

import android.text.InputType
import android.view.inputmethod.EditorInfo
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class FieldTypingTest {
    private fun text(variation: Int = InputType.TYPE_TEXT_VARIATION_NORMAL, flags: Int = 0,
                     options: Int = EditorInfo.IME_ACTION_NONE) =
        FieldTyping.of(InputType.TYPE_CLASS_TEXT or variation or flags, options)

    @Test fun aPlainTextFieldGetsSuggestionsAndIsLearnedFrom() {
        val plain = text()
        assertTrue(plain.ordinary)
        assertFalse(plain.secure)
        assertTrue(plain.learns)
    }

    @Test fun noLearningKeepsSuggestionsButRemembersNothing() {
        // A browser's incognito tab: the flag says only that nothing typed there is learned.
        val private = text(options = EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING)
        assertTrue(private.ordinary)
        assertFalse(private.secure)
        assertFalse(private.learns)
    }

    @Test fun addressesFiltersAndEditorsThatDisableSuggestionsStayLiteral() {
        for (variation in listOf(InputType.TYPE_TEXT_VARIATION_URI, InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS,
            InputType.TYPE_TEXT_VARIATION_WEB_EMAIL_ADDRESS, InputType.TYPE_TEXT_VARIATION_FILTER)) {
            assertFalse("variation $variation", text(variation).ordinary)
        }
        assertFalse(text(flags = InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS).ordinary)
        assertFalse(text(flags = InputType.TYPE_TEXT_FLAG_AUTO_COMPLETE).ordinary)
        // The flag exempts none of them: an incognito tab's URL field is still a URL field.
        assertFalse(text(InputType.TYPE_TEXT_VARIATION_URI,
            options = EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING).ordinary)
    }

    @Test fun editorsThatKeepTheirOwnTextTakeKeyEvents() {
        // TYPE_NULL (a terminal): the field's text stays on its own side, so only key presses reach it.
        val terminal = FieldTyping.of(InputType.TYPE_NULL, 0)
        assertTrue(terminal.keyEvents)
        assertFalse(terminal.secure)
        assertFalse(terminal.ordinary)
        // A flag only adds to the class: TYPE_NULL | MULTI_LINE is still TYPE_NULL.
        assertTrue(FieldTyping.of(InputType.TYPE_NULL or InputType.TYPE_TEXT_FLAG_MULTI_LINE, 0).keyEvents)
        assertFalse(text().keyEvents)
    }

    @Test fun passwordFieldsAreSecureAndNeverOrdinary() {
        for (variation in listOf(InputType.TYPE_TEXT_VARIATION_PASSWORD, InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD,
            InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD)) {
            assertTrue("variation $variation", text(variation).secure)
            assertFalse("variation $variation", text(variation).ordinary)
        }
        val pin = FieldTyping.of(InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_VARIATION_PASSWORD, 0)
        assertTrue(pin.secure)
        assertFalse(pin.ordinary)
    }
}
