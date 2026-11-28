package ly.anoma.hex

import androidx.compose.foundation.layout.*
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.colorResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt
import ly.anoma.hex.keyboard.KeyClick
import ly.anoma.hex.typing.TypingPreferences

/** Labs › Keyboard › Key click volume (#3005), inside the Keyboard group's card: the toggle and, while it is on, a
 * 5–100% slider that clicks at the level shown as it moves. Off, the keys click at the phone's own level. */
@Composable
internal fun KeyClickRows(typing: TypingPreferences, revision: Int) {
    val context = LocalContext.current
    val on = remember(revision) { typing.keyClickVolume }
    // Saved when the drag ends, so the settings listener doesn't recompose the page at every step.
    var percent by remember(revision) { mutableIntStateOf(typing.keyClickPercent) }
    Toggle(stringResource(R.string.key_click_volume), stringResource(R.string.key_click_volume_detail), on) {
        typing.keyClickVolume = it
    }
    if (!on) return
    val name = stringResource(R.string.key_click_volume)
    val ink = colorResource(R.color.ink)
    val track = colorResource(R.color.modifier)
    Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Slider(value = percent.toFloat(),
            onValueChange = { v ->
                val p = KeyClick.clamp((v / KeyClick.STEP).roundToInt() * KeyClick.STEP)
                if (p != percent) { percent = p; KeyClick.preview(context, p) }
            },
            onValueChangeFinished = { typing.keyClickPercent = percent },
            valueRange = KeyClick.MIN.toFloat()..KeyClick.MAX.toFloat(),
            steps = (KeyClick.MAX - KeyClick.MIN) / KeyClick.STEP - 1,
            colors = SliderDefaults.colors(thumbColor = ink, activeTrackColor = ink, inactiveTrackColor = track,
                activeTickColor = track, inactiveTickColor = ink),
            modifier = Modifier.weight(1f).semantics { contentDescription = name })
        Label(stringResource(R.string.key_click_volume_percent, percent), size = 14,
            mod = Modifier.widthIn(min = 44.dp))
    }
}
