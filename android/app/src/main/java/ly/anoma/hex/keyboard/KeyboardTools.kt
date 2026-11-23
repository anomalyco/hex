package ly.anoma.hex.keyboard

/** A button the strip shows in its empty line (Labs › Keyboard › Keyboard tools), left to right. */
internal enum class StripTool { Emoji, Paste, Settings, Undo }

/** Which tools the strip shows: none in a password field, while it has suggestions, Undo dictation, a
 * notice, the menu or a dictation to show instead, or with the lab off. Paste needs a clip and Undo an
 * autocorrection still to revert; the field, the clipboard and the editor are asked only once the lab is
 * on and the strip is free, so off, nothing but the setting is read. */
internal object KeyboardTools {
    fun tools(enabled: Boolean, busy: Boolean, secure: () -> Boolean, canUndo: () -> Boolean,
              clipboard: () -> Boolean): List<StripTool> {
        if (!enabled || busy || secure()) return emptyList()
        val clip = clipboard()
        val undo = canUndo()
        return StripTool.entries.filter { (it != StripTool.Paste || clip) && (it != StripTool.Undo || undo) }
    }
}
