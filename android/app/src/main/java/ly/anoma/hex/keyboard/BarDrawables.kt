package ly.anoma.hex.keyboard

import android.content.Context
import android.content.res.ColorStateList
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.StateListDrawable
import android.widget.ImageView
import ly.anoma.hex.R

/** The pieces the strip and the voice bar share: an ink-tinted icon, a filled rounded panel, and the round
 * press under a key. */
internal fun barIcon(context: Context, res: Int, tint: Int) =
    ImageView(context).apply { setImageResource(res); imageTintList = ColorStateList.valueOf(tint) }

internal fun barShape(fill: Int, radius: Float) =
    GradientDrawable().apply { setColor(fill); cornerRadius = radius }

internal fun barPressable(context: Context, fill: Int) = StateListDrawable().apply {
    addState(intArrayOf(android.R.attr.state_pressed),
        GradientDrawable().apply { shape = GradientDrawable.OVAL; setColor(context.getColor(R.color.pressed)) })
    addState(intArrayOf(), GradientDrawable().apply { shape = GradientDrawable.OVAL; setColor(fill) })
}
