package ly.anoma.hex

import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import ly.anoma.hex.speech.DictationSounds
import ly.anoma.hex.speech.SoundSet

/** A sound set's name, desktop Hex's English names. */
internal val SoundSet.label get() = when (this) {
    SoundSet.Classic -> R.string.sound_set_classic
    SoundSet.Signature -> R.string.sound_set_signature
    SoundSet.Click -> R.string.sound_set_click
    SoundSet.Soft -> R.string.sound_set_soft
    SoundSet.Wood -> R.string.sound_set_wood
    SoundSet.Analog -> R.string.sound_set_analog
    SoundSet.Thock -> R.string.sound_set_thock
}

/** What a set sounds like, as desktop Hex's sound picker says. */
private val SoundSet.character get() = when (this) {
    SoundSet.Classic -> R.string.sound_set_classic_detail
    SoundSet.Signature -> R.string.sound_set_signature_detail
    SoundSet.Click -> R.string.sound_set_click_detail
    SoundSet.Soft -> R.string.sound_set_soft_detail
    SoundSet.Wood -> R.string.sound_set_wood_detail
    SoundSet.Analog -> R.string.sound_set_analog_detail
    SoundSet.Thock -> R.string.sound_set_thock_detail
}

/** Labs › Dictation sounds and, while it is on, its Sound row. Off frees the loaded sounds. */
@Composable internal fun DictationSoundRows(on: Boolean, set: SoundSet, turn: (Boolean) -> Unit, pick: () -> Unit) {
    Toggle(stringResource(R.string.dictation_sounds), stringResource(R.string.dictation_sounds_detail), on,
        noteLines = Int.MAX_VALUE) {
        turn(it)
        if (!it) DictationSounds.release()
    }
    if (on) {
        Rule()
        Entry(stringResource(R.string.dictation_sound), value = stringResource(set.label), action = pick)
    }
}

/** The Sound page: desktop Hex's seven sets in its order. A tap chooses the set and plays a short dictation in it, and
 * the page stays open to try another, as desktop's picker does. */
@Composable internal fun SoundSetPage(chosen: SoundSet, choose: (SoundSet) -> Unit) {
    val context = LocalContext.current
    Card {
        SoundSet.entries.forEachIndexed { i, it ->
            if (i != 0) Rule()
            Entry(stringResource(it.label), stringResource(it.character), value = if (it == chosen) "✓" else "",
                action = { choose(it); DictationSounds.audition(context, it) })
        }
    }
}
