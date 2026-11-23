package ly.anoma.hex.keyboard

/** A button the strip shows in its empty line (Labs › Keyboard › Keyboard tools), left to right. */
internal enum class StripTool { Emoji, Paste, Settings, Undo }

/** Which tools the strip shows: none in a password field, while it has suggestions, Undo dictation, a
 * notice, the menu or a dictation to show instead, or with the lab off. Paste needs a clip and Undo an
 * autocorrection still to revert; both are asked only once the tools can show, so nothing reads the
 * clipboard or the editor when they can't. */
internal object KeyboardTools {
    fun tools(enabled: Boolean, secure: Boolean, busy: Boolean, canUndo: () -> Boolean,
              clipboard: () -> Boolean): List<StripTool> {
        if (!enabled || secure || busy) return emptyList()
        val clip = clipboard()
        val undo = canUndo()
        return StripTool.entries.filter { (it != StripTool.Paste || clip) && (it != StripTool.Undo || undo) }
    }
}
