package ly.anoma.hex.speech

import android.content.Context
import android.content.res.AssetManager

internal interface Recognizer {
    fun decode(pcm: ShortArray, count: Int, language: String, translate: Boolean): String?
}

internal class LocalSpeech(private val app: Context) : Recognizer {
    override fun decode(pcm: ShortArray, count: Int, language: String, translate: Boolean): String? {
        val threads = Runtime.getRuntime().availableProcessors().coerceIn(1, 4)
        val preferred = ModelStore.installed(app)
        val bytes = preferred?.let { Whisper.decode(app.assets, it.absolutePath, pcm, count, threads, language, translate) }
            ?: Whisper.decode(app.assets, null, pcm, count, threads, language, translate)
        return bytes?.decodeToString()?.trim()
    }
}

internal object Whisper {
    init { System.loadLibrary("hexspeech") }
    external fun decode(assets: AssetManager, file: String?, pcm: ShortArray, count: Int, threads: Int,
                        language: String, translate: Boolean): ByteArray?
}
