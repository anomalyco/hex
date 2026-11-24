package ly.anoma.hex.keyboard

import android.text.InputType
import android.view.inputmethod.EditorInfo
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class FieldRulesTest {
    private fun of(inputType: Int, imeOptions: Int = 0) = FieldRules.of(inputType, imeOptions)

    @Test fun plainTextKeepsAssistanceAndLearns() {
        val plain = of(InputType.TYPE_CLASS_TEXT)
        assertTrue(plain.ordinary)
        assertTrue(plain.learns)
        assertFalse(plain.secure)
        // A capitalisation flag is still a plain field.
        val sentences = of(InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES)
        assertTrue(sentences.ordinary)
        assertTrue(sentences.learns)
    }

    @Test fun aPrivateFieldKeepsAssistanceButLearnsNothing() {
        val private = of(InputType.TYPE_CLASS_TEXT, EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING)
        assertTrue(private.ordinary)
        assertFalse(private.learns)
        assertFalse(private.secure)
        // Other ime options beside the flag change neither.
        val search = of(InputType.TYPE_CLASS_TEXT, EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING or EditorInfo.IME_ACTION_SEARCH)
        assertTrue(search.ordinary)
        assertFalse(search.learns)
    }

    @Test fun addressesAndFiltersAreNotOrdinary() {
        for (variation in listOf(InputType.TYPE_TEXT_VARIATION_URI, InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS,
            InputType.TYPE_TEXT_VARIATION_WEB_EMAIL_ADDRESS, InputType.TYPE_TEXT_VARIATION_FILTER)) {
            val rules = of(InputType.TYPE_CLASS_TEXT or variation)
            assertFalse(rules.ordinary)
            assertFalse(rules.secure)
            // Not private either: the flag alone decides that.
            assertTrue(rules.learns)
        }
    }

    @Test fun editorsThatDisableSuggestionsAreNotOrdinary() {
        assertFalse(of(InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS).ordinary)
        assertFalse(of(InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_AUTO_COMPLETE).ordinary)
    }

    @Test fun passwordsAreSecureAndNotOrdinary() {
        for (variation in listOf(InputType.TYPE_TEXT_VARIATION_PASSWORD, InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD,
            InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD)) {
            val rules = of(InputType.TYPE_CLASS_TEXT or variation)
            assertTrue(rules.secure)
            assertFalse(rules.ordinary)
        }
        val pin = of(InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_VARIATION_PASSWORD)
        assertTrue(pin.secure)
        assertFalse(pin.ordinary)
    }

    @Test fun numbersAndNullFieldsAreNotOrdinary() {
        for (inputType in listOf(InputType.TYPE_CLASS_NUMBER, InputType.TYPE_CLASS_PHONE,
            InputType.TYPE_CLASS_DATETIME, InputType.TYPE_NULL)) {
            val rules = of(inputType)
            assertFalse(rules.ordinary)
            assertFalse(rules.secure)
        }
    }
}
