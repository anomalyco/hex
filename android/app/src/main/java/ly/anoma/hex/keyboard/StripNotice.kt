package ly.anoma.hex.keyboard

/** One line the strip shows in place of the hint and suggestions until the person types, taps it or its ✕.
 * Hex Preview's updater (src/preview) and Labs › Diagnostics' keyboard speed test (Hex Preview and debug builds) use it;
 * Play builds never show one. */
internal class StripNotice(val text: String, val dismissLabel: String, val tap: () -> Unit, val dismiss: () -> Unit)
