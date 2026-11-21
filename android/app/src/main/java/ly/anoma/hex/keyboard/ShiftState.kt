package ly.anoma.hex.keyboard

/** Shift as a small state machine, after iPhone Hex's ShiftStateMachine. Shift acts on touch-down, so
 * a held shift and a letter (a chord) type a capital and shift goes off when it lifts; two taps
 * within [DOUBLE_TAP_MS] lock it (Gboard on a Pixel 8 emulator: 0.22 s locked, 0.37 s didn't); a tap
 * while locked unlocks. A long press is one shift, as on Gboard. */
internal class ShiftState {
    enum class Mode { Off, On, Locked }
    companion object { const val DOUBLE_TAP_MS = 300L }
    var mode = Mode.Off
        private set
    var held = false
        private set
    private var typedWhileHeld = false
    private var lastTapDown = -1L

    val uppercase get() = mode != Mode.Off || held
    val locked get() = mode == Mode.Locked

    fun down(now: Long) {
        held = true; typedWhileHeld = false
        when {
            mode == Mode.Locked -> { mode = Mode.Off; lastTapDown = -1 }
            lastTapDown >= 0 && now - lastTapDown < DOUBLE_TAP_MS -> { mode = Mode.Locked; lastTapDown = -1 }
            else -> { mode = if (mode == Mode.Off) Mode.On else Mode.Off; lastTapDown = now }
        }
    }

    /** A chord (a letter typed while held) neither stays on nor counts towards a double tap. */
    fun up() {
        held = false
        if (typedWhileHeld) { typedWhileHeld = false; lastTapDown = -1; if (mode != Mode.Locked) mode = Mode.Off }
    }

    /** A character went in. */
    fun typed() {
        if (held) typedWhileHeld = true else if (mode == Mode.On) mode = Mode.Off
        lastTapDown = -1
    }

    /** Auto-capitalisation after an edit or a field change; leaves caps lock and a held shift alone. */
    fun auto(capitalize: Boolean) { if (mode != Mode.Locked && !held) mode = if (capitalize) Mode.On else Mode.Off }

    fun reset() { mode = Mode.Off; held = false; typedWhileHeld = false; lastTapDown = -1 }
    fun forgetTap() { lastTapDown = -1 }
}
