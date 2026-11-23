package ly.anoma.hex.keyboard

/** One line the strip shows in place of the hint and suggestions until the person types, taps it or its ✕.
 * Only Hex Preview's updater uses it (src/preview); Play builds never show one. */
internal class StripNotice(val text: String, val dismissLabel: String, val tap: () -> Unit, val dismiss: () -> Unit)
