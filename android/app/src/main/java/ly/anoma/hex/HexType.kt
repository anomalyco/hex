package ly.anoma.hex

import android.graphics.Typeface
import android.graphics.fonts.FontFamily as NativeFontFamily
import android.graphics.fonts.Font as NativeFont
import android.widget.TextView
import androidx.compose.material3.Typography
import androidx.compose.ui.text.ExperimentalTextApi
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontVariation
import androidx.compose.ui.text.font.FontWeight

@OptIn(ExperimentalTextApi::class)
internal val inter = FontFamily(*listOf(400, 500, 600, 700).map { w ->
    Font(R.font.inter_variable, weight = FontWeight(w),
        variationSettings = FontVariation.Settings(FontVariation.weight(w)))
}.toTypedArray())

private fun TextStyle.inInter() = copy(fontFamily = inter, fontFeatureSettings = "tnum")
internal val hexTypography = Typography().run {
    copy(
        displayLarge = displayLarge.inInter(),
        displayMedium = displayMedium.inInter(),
        displaySmall = displaySmall.inInter(),
        headlineLarge = headlineLarge.inInter(),
        headlineMedium = headlineMedium.inInter(),
        headlineSmall = headlineSmall.inInter(),
        titleLarge = titleLarge.inInter(),
        titleMedium = titleMedium.inInter(),
        titleSmall = titleSmall.inInter(),
        bodyLarge = bodyLarge.inInter(),
        bodyMedium = bodyMedium.inInter(),
        bodySmall = bodySmall.inInter(),
        labelLarge = labelLarge.inInter(),
        labelMedium = labelMedium.inInter(),
        labelSmall = labelSmall.inInter(),
    )
}

// Android supplies missing script/emoji glyphs; all Hex-owned Latin labels use the same resource.
// One typeface per weight: fontVariationSettings on each label made a new native typeface every time the keys
// were drawn, and the keyboard's native memory grew with each field it opened in.
private val interWeights = HashMap<Int, Typeface>()
internal fun TextView.useInter(weight: Int) {
    typeface = interWeights.getOrPut(weight) {
        val font = NativeFont.Builder(resources, R.font.inter_variable).setWeight(weight)
            .setFontVariationSettings("'wght' $weight, 'opsz' 14").build()
        Typeface.CustomFallbackBuilder(NativeFontFamily.Builder(font).build()).setSystemFallback("sans-serif").build()
    }
    fontFeatureSettings = "tnum"
}
