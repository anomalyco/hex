package ly.anoma.hex.keyboard

/** Rollover, as Gboard's (real two-finger touches on a Pixel 8 emulator, API 36): a key that types as
 * it lifts types at once when another key goes down while it is still held, so overlapping taps type
 * in the order the fingers went down ("a" down, "s" down, "s" up, "a" up types "as"; space, comma
 * and delete likewise). Keys that act as they go down (shift, ?123) only make the held key type. */
internal class Rollover {
    private var held: Any? = null
    private var commit: (() -> Unit)? = null

    /** [key] went down: the key still held types now; [typeNow] types [key] early if another goes down. */
    fun down(key: Any, typeNow: (() -> Unit)? = null) {
        val earlier = commit; val other = held
        held = key; commit = typeNow
        if (other != null && other !== key) earlier?.invoke()
    }

    /** [key] lifted: true when it types now, false when a later key already made it type. */
    fun up(key: Any): Boolean {
        if (held !== key) return false
        held = null; commit = null
        return true
    }

    fun clear() { held = null; commit = null }
}
