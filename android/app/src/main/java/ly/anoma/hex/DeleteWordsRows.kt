package ly.anoma.hex

import androidx.compose.foundation.layout.*
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.colorResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt
import ly.anoma.hex.keyboard.HeldDeleteWords
import ly.anoma.hex.typing.TypingPreferences

/** Labs › Keyboard › Delete words when held (#3007), inside the Keyboard group's card: the toggle and, while it is on,
 * its Initial delay (200–1000 ms) and Acceleration (0–50%) sliders (#3032). Both are kept while the lab is off. */
@Composable
internal fun DeleteWordsRows(typing: TypingPreferences, revision: Int) {
    val on = remember(revision) { typing.deleteWords }
    // Saved when the drag ends, so the settings listener doesn't recompose the page at every step.
    var delay by remember(revision) { mutableIntStateOf(typing.deleteWordsDelay) }
    var speedup by remember(revision) { mutableIntStateOf(typing.deleteWordsSpeedup) }
    Toggle(stringResource(R.string.delete_words), stringResource(R.string.delete_words_detail), on) {
        typing.deleteWords = it
    }
    if (!on) return
    StepSlider(stringResource(R.string.delete_words_delay), stringResource(R.string.delete_words_delay_ms, delay), delay,
        HeldDeleteWords.DELAY_MIN, HeldDeleteWords.DELAY_MAX, HeldDeleteWords.DELAY_STEP,
        change = { delay = it }, done = { typing.deleteWordsDelay = delay })
    StepSlider(stringResource(R.string.delete_words_speedup), stringResource(R.string.delete_words_speedup_percent, speedup),
        speedup, HeldDeleteWords.SPEEDUP_MIN, HeldDeleteWords.SPEEDUP_MAX, HeldDeleteWords.SPEEDUP_STEP,
        change = { speedup = it }, done = { typing.deleteWordsSpeedup = speedup })
}

/** A titled slider from [min] to [max] in steps of [step], with its value shown at the end. */
@Composable
private fun StepSlider(name: String, shown: String, value: Int, min: Int, max: Int, step: Int,
                       change: (Int) -> Unit, done: () -> Unit) {
    val ink = colorResource(R.color.ink)
    val track = colorResource(R.color.modifier)
    Label(name, size = 14, mod = Modifier.padding(start = 16.dp, end = 16.dp, top = 4.dp))
    Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Slider(value = value.toFloat(),
            onValueChange = { v ->
                val n = ((v - min) / step).roundToInt() * step + min
                if (n != value) change(n.coerceIn(min, max))
            },
            onValueChangeFinished = done,
            valueRange = min.toFloat()..max.toFloat(),
            steps = (max - min) / step - 1,
            colors = SliderDefaults.colors(thumbColor = ink, activeTrackColor = ink, inactiveTrackColor = track,
                activeTickColor = track, inactiveTickColor = ink),
            modifier = Modifier.weight(1f).semantics { contentDescription = name })
        Label(shown, size = 14, mod = Modifier.widthIn(min = 64.dp))
    }
}
