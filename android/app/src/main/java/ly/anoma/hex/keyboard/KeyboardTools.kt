package ly.anoma.hex.keyboard

/** A button the strip shows in its empty line (Labs › Keyboard › Keyboard tools), left to right. */
internal enum class StripTool { Emoji, Paste, Settings, Undo }

/** Which tools the strip shows: none in a password field, while it has suggestions, Undo dictation, a
 * notice, the menu or a dictation to show instead, or with the lab off. Emoji needs the letters page,
 * where the keys' own emoji key is; Paste needs a clip and Undo an autocorrection still to revert. The
 * field, the page, the clipboard and the editor are asked only once the lab is on and the strip is free,
 * so off, nothing but the setting is read. */
internal object KeyboardTools {
    fun tools(enabled: Boolean, busy: Boolean, secure: () -> Boolean, emoji: () -> Boolean,
              canUndo: () -> Boolean, clipboard: () -> Boolean): List<StripTool> {
        if (!enabled || busy || secure()) return emptyList()
        val letters = emoji()
        val clip = clipboard()
        val undo = canUndo()
        return StripTool.entries.filter {
            (it != StripTool.Emoji || letters) && (it != StripTool.Paste || clip) && (it != StripTool.Undo || undo)
        }
    }

    /** Whether an empty field's line goes to the tools instead of the sentence starters it would
     * otherwise offer: the lab is on and the field has no text before the cursor or after it. The
     * text after the cursor is read only once the lab is on and nothing comes before. */
    fun showsInEmptyField(enabled: Boolean, before: String, after: () -> String?): Boolean =
        enabled && before.isEmpty() && after().isNullOrEmpty()
}
