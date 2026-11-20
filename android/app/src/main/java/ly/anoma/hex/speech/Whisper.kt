package ly.anoma.hex.speech

import android.content.res.AssetManager

internal object Whisper {
    init { System.loadLibrary("hexspeech") }
    external fun decode(assets: AssetManager, pcm: ShortArray, count: Int, threads: Int): ByteArray?
}
