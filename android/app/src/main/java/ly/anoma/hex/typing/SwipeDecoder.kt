package ly.anoma.hex.typing

import android.content.res.AssetManager
import java.io.Reader

/**
 * Swipe typing's reader for one layout language: the Rust core's decoder ([KeyboardCore]) on the
 * APK's model, which never reads the language's slurs (nor its swear words unless Suggest swear
 * words is on), and what a swipe types from your text replacements and words. The core's handles
 * are made again only when one of those changed ([sync], between swipes); the ones they replace
 * are freed through `retire`, after the reads already queued there.
 */
internal class SwipeDecoder(private val model: Long, val words: TypingModel, private val blocked: List<Pair<String, Boolean>>) {
    /** The handles one swipe reads with, and what they were made from. */
    class Handles(val decoder: Long, val words: Long, val swears: Boolean, val personal: PersonalWords?)

    var handles: Handles? = null
        private set

    /** Handles for these settings: the ones in use while nothing changed, else new ones; null when
     * the core can't make them. */
    fun sync(swears: Boolean, personal: PersonalWords?, retire: (Runnable) -> Unit): Handles? {
        handles?.let { if (it.swears == swears && it.personal === personal) return it }
        val never = ArrayList<Int>(); val pairs = ArrayList<Int>()
        // TypingDecoder.NeverSuggest: a word, or a two-word expression, the model has.
        for ((spelling, swear) in blocked) {
            if (swear && swears) continue
            val parts = spelling.split(' ').filter { it.isNotEmpty() }
            when (parts.size) {
                1 -> words.index(spelling).takeIf { it >= 0 }?.let(never::add)
                2 -> {
                    val a = words.index(parts[0]); val b = words.index(parts[1])
                    if (a >= 0 && b >= 0) { pairs.add(a); pairs.add(b) }
                }
            }
        }
        val own = personal?.let { p -> List(p.size) { p.text(it) } }.orEmpty()
        val shortcuts = personal?.shortcuts.orEmpty()
        // Read beside the model's words, as iOS's SwipeTyping.syncWords: English's contractions, then your own
        // words (your personal dictionary's, and the one-word outputs of your text replacements).
        val contractions = if (words.language == "en") englishContractions else emptyList()
        val yours = own + shortcuts.values.filter { ' ' !in it }
        val extra = contractions.map { it.first } + yours
        val costs = contractions.map { it.second } + yours.map { ownWordCost }
        val decoder = KeyboardCore.swipeNew(model, never.toIntArray(), pairs.toIntArray(), extra.toTypedArray(), costs.toDoubleArray(),
            BooleanArray(extra.size) { it < contractions.size })
        // Android has no vocabulary list of its own yet.
        val typing = KeyboardCore.swipeWordsNew(words.language, true, shortcuts.keys.toTypedArray(), shortcuts.values.toTypedArray(),
            own.toTypedArray(), emptyArray())
        if (decoder == 0L || typing == 0L) {
            KeyboardCore.swipeFree(decoder); KeyboardCore.swipeWordsFree(typing)
            return null
        }
        val old = handles
        if (old != null) retire(Runnable { KeyboardCore.swipeFree(old.decoder); KeyboardCore.swipeWordsFree(old.words) })
        return Handles(decoder, typing, swears, personal).also { handles = it }
    }

    /** The readings of a swipe through [points] (x, y interleaved, in [grid]'s coordinates) with [times] (seconds,
     * one per point), best first once rare names cost more; empty when it reads as none. */
    fun read(handles: Handles, points: DoubleArray, times: DoubleArray?, grid: KeyGrid, previous: String?, limit: Int): List<String> {
        val keys = grid.centres.entries.toList()
        val reading = KeyboardCore.swipeRead(handles.decoder, points, times, IntArray(keys.size) { keys[it].key.code },
            DoubleArray(keys.size) { keys[it].value.x }, DoubleArray(keys.size) { keys[it].value.y }, grid.pitchX, grid.pitchY,
            previous, limit) ?: return emptyList()
        val order = KeyboardCore.swipeRank(handles.decoder, handles.words, reading.texts, reading.costs, checker) ?: return reading.texts.toList()
        return order.map { reading.texts[it] }
    }

    /**
     * Whether the dictionary knows a word, from Android's lists, for the core's casing and name
     * rules: a lowercase word when the model or `<lang>.valid` has it and the books write it with
     * a capital less than 95% of the time; any other when the books write its lowercase form with
     * a capital at least 95% of the time (`<lang>.capitals`, the threshold the typing engine's
     * name casing uses).
     */
    val checker = WordChecker { word ->
        val lower = word.lowercase()
        val capitalised = (words.capitals?.percent(lower) ?: -1) >= NAME_PERCENT
        if (word == lower) (words.index(word) >= 0 || words.validLiteral(word)) && !capitalised else capitalised
    }

    companion object {
        private const val NAME_PERCENT = 95
        /** The core's English contractions, each with its cost. */
        private val englishContractions by lazy { KeyboardCore.swipeEnglishContractions().map { it to KeyboardCore.swipeCostOfContraction(it) } }
        private val ownWordCost by lazy { KeyboardCore.swipeOwnWordCost() }

        /** The language's decoder, its model opened from the APK's uncompressed asset; null when it can't. */
        fun open(assets: AssetManager, words: TypingModel): SwipeDecoder? {
            val model = assets.openFd("typing/${words.language}.hextm").use {
                KeyboardCore.modelOpenFd(it.parcelFileDescriptor.fd, it.startOffset, it.length)
            }
            if (model == 0L) return null
            return SwipeDecoder(model, words, assets.open("typing/never-suggest.tsv").bufferedReader().use { blocked(it, words.language) })
        }

        /** never-suggest.tsv's entries for `language`, each with whether it is a swear word (else a slur). */
        fun blocked(list: Reader, language: String) = list.readLines().mapNotNull { line ->
            if (line.isBlank() || line.startsWith('#')) return@mapNotNull null
            val (lang, kind, spelling) = line.split('\t')
            if (lang != "*" && lang != language) null else spelling to (kind == "swear")
        }
    }
}
