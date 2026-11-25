package ly.anoma.hex.speech

import android.app.LocaleManager
import android.content.Context
import android.content.res.Resources
import android.os.Build
import java.util.Locale

internal enum class Style { Polished, Casual, Notes, Verbatim }
/** A keyboard layout's language: its keys, accents and typing model. Dictation languages are
 * [DictationLanguage]; a language can be dictated without a layout of its own. */
internal enum class Spoken(val code: String, val label: String) {
    English("en", "English"), Spanish("es", "Español"), German("de", "Deutsch"), Norwegian("no", "Norsk"),
    French("fr", "Français"), Italian("it", "Italiano"), Portuguese("pt", "Português");

    // Whisper calls Bokmål "no"; the keyboard and its model use BCP 47 "nb".
    val layoutCode get() = if (this == Norwegian) "nb" else code
    companion object {
        fun matching(code: String?): Spoken? = when (code) {
            "no", "nb", "nn" -> Norwegian
            else -> entries.firstOrNull { it.code == code }
        }
    }
}

/** What Whisper is asked to transcribe ([code], Whisper's name for it) and the layout the
 * keyboard follows while it is the dictation language, when there is one. */
internal enum class DictationLanguage(val code: String, val label: String, val layout: Spoken?) {
    English("en", "English", Spoken.English), Spanish("es", "Español", Spoken.Spanish),
    German("de", "Deutsch", Spoken.German), Norwegian("no", "Norsk", Spoken.Norwegian),
    French("fr", "Français", Spoken.French), Italian("it", "Italiano", Spoken.Italian), Dutch("nl", "Nederlands", null),
    // One entry for Brazil and Portugal: Whisper has a single "pt", and Hex has no regional rules. Its typing
    // model is Brazil's, with Portugal's spellings among its real words (benchmark/typing/README.md).
    Portuguese("pt", "Português", Spoken.Portuguese),
    // No layouts of their own yet: Polish's letters are held on the English keys, and Russian has no
    // Cyrillic keys, so a Russian speaker dictates here and types on another keyboard.
    Polish("pl", "Polski", null), Russian("ru", "Русский", null);

    companion object {
        fun matching(code: String?): DictationLanguage? = when (code) {
            "no", "nb", "nn" -> Norwegian
            else -> entries.firstOrNull { it.code == code }
        }
        /** Automatic: the keyboard's layout when one is chosen there, as iPhone dictation follows the
         * keyboard's language, else the first of the phone's languages Hex dictates, else English. */
        fun automatic(keyboard: Spoken?, system: List<String>): DictationLanguage =
            entries.firstOrNull { keyboard != null && it.layout == keyboard }
                ?: system.firstNotNullOfOrNull(::matching) ?: English
        /** Labs › Automatic listens for the language: the languages Automatic chooses among, at most three. Its own
         * language ([keyboard], [automatic]'s) comes first and is kept unless the speech is clearly another; then the
         * phone's other languages Hex dictates, in the phone's order; then the last one picked by hand ([picked]). Bokmål
         * and Nynorsk are one ([matching]); Danish and Swedish, which Hex doesn't dictate, never count. */
        fun candidates(keyboard: DictationLanguage, system: List<String>, picked: DictationLanguage?): List<DictationLanguage> =
            (listOf(keyboard) + system.mapNotNull(::matching) + listOfNotNull(picked)).distinct().take(3)
    }
}

/** What setup says about the dictation language: [spoken], the phone's first language in its own name when Hex can't
 * dictate it ([unsupported]), and the phone's other languages Hex dictates, a hold of the space bar away ([others]).
 * A language chosen in Settings or a pinned layout is the reason itself ([automatic] false), so neither carries the rest. */
internal data class PhoneLanguage(val spoken: DictationLanguage, val unsupported: String? = null,
                                  val others: List<DictationLanguage> = emptyList(), val automatic: Boolean = true,
                                  /** Labs › Automatic listens for the language: the languages it chooses among, when several. */
                                  val listensAmong: List<DictationLanguage> = emptyList())

/** The phone's languages, in the person's order: Android's system list, never Hex's own App language. From Android 13
 * an app language is folded into the process's configuration, Resources.getSystem()'s included, so only
 * LocaleManager still has the system's own list. */
internal fun systemLocales(context: Context): List<Locale> {
    val locales = (if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU)
        context.getSystemService(LocaleManager::class.java)?.systemLocales else null) ?: Resources.getSystem().configuration.locales
    return (0 until locales.size()).map { locales[it] }
}

internal fun phoneLanguage(chosen: DictationLanguage?, pinned: Spoken?, system: List<Locale>): PhoneLanguage {
    val spoken = chosen ?: DictationLanguage.automatic(pinned, system.map { it.language })
    if (chosen != null || pinned != null) return PhoneLanguage(spoken, automatic = false)
    val unsupported = system.firstOrNull()?.takeIf { DictationLanguage.matching(it.language) == null }
        ?.let { it.getDisplayLanguage(it).replaceFirstChar { c -> c.titlecase(it) } }?.takeIf { it.isNotEmpty() }
    if (unsupported != null) return PhoneLanguage(spoken, unsupported)
    return PhoneLanguage(spoken, others = system.mapNotNull { DictationLanguage.matching(it.language) }.distinct() - spoken)
}

/** The layout whose typing model the keyboard reads: the one pinned in Keyboard › Keyboard layout, else the
 * dictation language's own. Null when the keys are only English's stand-in for a language with no layout
 * (Nederlands, Polski, Русский): English's next words and autocorrections are not that language's. */
internal fun typingLayout(pinned: Spoken?, spoken: DictationLanguage): Spoken? = pinned ?: spoken.layout

internal data class SpeechOptions(val style: Style, val spoken: DictationLanguage, val clean: Boolean, val translate: Boolean,
                                   val lowercase: Boolean = false, val endWithPeriod: Boolean = true,
                                   val formatLists: Boolean = false, val spokenPunctuation: Boolean = false,
                                   /** Custom terms plus any learned fixed word with a capital: formatting forces
                                    * exactly these spellings, never the plain learned words. */
                                   val vocabulary: List<String> = emptyList(), val fastEnglish: Boolean = false,
                                   /** Labs › Learn from my fixes' active fixes, applied to the raw transcript. */
                                   val fixes: List<DictationFix> = emptyList(),
                                   /** Whether the lab was on when the dictation started; its tracking is keyboard-only. */
                                   val learnFixes: Boolean = false,
                                   /** What the recognizers hear: [vocabulary] unless the lab adds learned words.
                                    * Never used as forced spellings. */
                                   val hints: List<String> = emptyList(),
                                   /** Custom vocabulary's Word replacements, applied before learned fixes. */
                                   val replacements: List<WordReplacement> = emptyList(),
                                   /** Labs › Automatic listens for the language's candidates, [spoken] first: empty unless
                                    * Automatic is chosen with the lab on. One candidate decodes as the lab off does. */
                                   val candidates: List<DictationLanguage> = emptyList()) {
    /** The languages the standard model may write in, [spoken] first. */
    val languages get() = candidates.ifEmpty { listOf(spoken) }
    /** These options once the decode chose [heard] among [candidates]: the text is formatted and inserted in it, and it
     * was translated only when it was Spanish (whisper-language-choice.patch transcribes any other). */
    fun writtenIn(heard: DictationLanguage) = copy(spoken = heard, translate = translate && heard == DictationLanguage.Spanish)
    /** Whisper's prompt and Parakeet's vocabulary. A translated dictation gets only the custom terms, and off
     * the lab's hints are the terms exactly as they always were. */
    val recognitionHints get() = if (!learnFixes || translate) vocabulary else hints
}

/** The recognizers' hints: the custom terms alone unless the lab is on and the dictation isn't translated,
 * then the terms and the learned spellings, deduplicated and capped. */
internal fun recognitionHints(terms: List<String>, learned: List<DictationFix>, learning: Boolean, translated: Boolean) =
    if (learning && !translated) DictationFixes.hints(terms, learned) else terms

/** The spellings formatting forces: the custom terms, and only learned words that carry a capital. A plain
 * learned word ("there") is a hint, never a spelling, so it cannot rewrite another word that sounds the same. */
internal fun forcedSpellings(terms: List<String>, learned: List<DictationFix>, learning: Boolean, translated: Boolean) =
    if (!learning || translated) terms else terms + DictationFixes.spellings(learned)
internal class SpeechPreferences(private val context: Context) {
    private val prefs = context.getSharedPreferences("speech", Context.MODE_PRIVATE)
    private val fixes by lazy { DictationFixStore(context) }
    var style: Style
        get() = Style.entries.firstOrNull { it.name == prefs.getString("style", null) } ?: Style.Polished
        set(value) { prefs.edit().putString("style", value.name).apply() }
    // Null means Automatic; never save the resolved locale over that choice. A language picked by hand is also kept as
    // "picked": Labs › Automatic listens for the language counts it once Automatic is chosen again.
    var language: DictationLanguage?
        get() = DictationLanguage.matching(prefs.getString("spoken", null))
        set(value) {
            val edit = prefs.edit().putString("spoken", value?.code)
            if (value != null) edit.putString("picked", value.code)
            edit.apply()
        }
    // Keyboard › Keyboard layout when chosen there (null while the layout follows dictation).
    private val typing = context.getSharedPreferences("typing", Context.MODE_PRIVATE)
    private fun phoneLanguages() = systemLocales(context).map { it.language }
    /** The language Automatic dictates in without listening: the keyboard's layout's, else the phone's. */
    val automaticLanguage: DictationLanguage
        get() = DictationLanguage.automatic(Spoken.matching(typing.getString("layout", null)), phoneLanguages())
    val spoken: DictationLanguage
        get() = language ?: automaticLanguage
    /** What setup's Language row says: what Automatic dictates in and why, from the same languages. */
    val phoneLanguage: PhoneLanguage
        get() = phoneLanguage(language, Spoken.matching(typing.getString("layout", null)), systemLocales(context))
            .copy(listensAmong = candidates.takeIf { it.size > 1 } ?: emptyList())
    // Labs › Automatic listens for the language, off by default.
    var listenForLanguage: Boolean
        get() = prefs.getBoolean("listenForLanguage", false)
        set(value) { prefs.edit().putBoolean("listenForLanguage", value).apply() }
    /** What Automatic chooses among, whether or not it is the language chosen now: [automaticLanguage] alone with the
     * lab off. */
    val automaticCandidates: List<DictationLanguage>
        get() = if (!listenForLanguage) listOf(automaticLanguage)
            else DictationLanguage.candidates(automaticLanguage, phoneLanguages(), DictationLanguage.matching(prefs.getString("picked", null)))
    /** What a dictation chooses among now ([SpeechOptions.candidates]): empty unless Automatic is chosen with the lab on. */
    val candidates: List<DictationLanguage>
        get() = if (language != null || !listenForLanguage) emptyList() else automaticCandidates
    /** Whether Translate what I say applies: Spanish is the dictation language, or one Automatic chooses among. */
    val translatable get() = DictationLanguage.Spanish in candidates.ifEmpty { listOf(spoken) }
    var clean: Boolean
        get() = prefs.getBoolean("clean", false)
        set(value) { prefs.edit().putBoolean("clean", value).apply() }
    var translation: Boolean
        get() = prefs.getBoolean("translation", false)
        set(value) { prefs.edit().putBoolean("translation", value).apply() }
    // Settings › Dictation › Lowercase: everything dictation writes, after the style.
    var lowercase: Boolean
        get() = prefs.getBoolean("lowercase", false)
        set(value) { prefs.edit().putBoolean("lowercase", value).apply() }
    // Off, no line ends with a full stop; "?" and "!" stay.
    var endWithPeriod: Boolean
        get() = prefs.getBoolean("endWithPeriod", true)
        set(value) { prefs.edit().putBoolean("endWithPeriod", value).apply() }
    // Say "first, second" or "bullet" for a list (English and Spanish).
    var formatLists: Boolean
        get() = prefs.getBoolean("formatLists", true)
        set(value) { prefs.edit().putBoolean("formatLists", value).apply() }
    // Say "comma" or "question mark" for the mark.
    var spokenPunctuation: Boolean
        get() = prefs.getBoolean("spokenPunctuation", true)
        set(value) { prefs.edit().putBoolean("spokenPunctuation", value).apply() }
    // Custom vocabulary: names and terms spelled your way, kept even while the switch is off.
    var customVocabulary: Boolean
        get() = prefs.getBoolean("customVocabulary", false)
        set(value) { prefs.edit().putBoolean("customVocabulary", value).apply() }
    var vocabularyTerms: List<String>
        get() = prefs.getString("vocabularyTerms", null)?.split('\n').orEmpty().filter(String::isNotEmpty)
        set(value) { prefs.edit().putString("vocabularyTerms", VocabularySpelling.sanitize(value).joinToString("\n")).apply() }
    // Custom vocabulary › Word replacements, kept even while the switch is off.
    var wordReplacements: List<WordReplacement>
        get() = WordReplacements.decode(prefs.getString("wordReplacements", null).orEmpty())
        set(value) { prefs.edit().putString("wordReplacements", WordReplacements.encode(WordReplacements.valid(value))).apply() }
    // Labs › Faster English dictation, off by default: English dictations use the downloaded faster model once it is there.
    var fastEnglish: Boolean
        get() = prefs.getBoolean("fastEnglish", false)
        set(value) { prefs.edit().putBoolean("fastEnglish", value).apply() }
    // Labs › Learn from my fixes: learn from dictation edits and apply what was learned, off by default.
    var learnFixes: Boolean
        get() = prefs.getBoolean("learnFixes", false)
        set(value) { prefs.edit().putBoolean("learnFixes", value).apply() }
    // Labs › Signature: the line added to each keyboard dictation, off by default.
    var signature: Boolean
        get() = prefs.getBoolean("signature", false)
        set(value) { prefs.edit().putBoolean("signature", value).apply() }
    var signatureText: String
        get() = prefs.getString("signatureText", Signature.DEFAULT_TEXT)
            ?.takeIf { Signature.textProblem(it) == null }?.trim() ?: Signature.DEFAULT_TEXT
        set(value) { prefs.edit().putString("signatureText", value).apply() }
    fun options(nextEnglish: Boolean?): SpeechOptions {
        val translated = translation && translatable && (nextEnglish ?: true)
        // A translated dictation is English the model wrote: nothing learned applies to it, and it hints nothing.
        val learned = if (learnFixes && !translated) fixes.active() else emptyList()
        val terms = if (customVocabulary) vocabularyTerms else emptyList()
        // Formatting forces the spellings in vocabulary. Learned words join it only when they carry a capital,
        // as iOS's spellings do; their other words are recognition hints alone.
        val vocabulary = forcedSpellings(terms, learned, learnFixes, translated)
        return SpeechOptions(style, spoken, clean, translated, lowercase, endWithPeriod, formatLists, spokenPunctuation,
            vocabulary, fastEnglish, learned, learnFixes, recognitionHints(terms, learned, learnFixes, translated),
            if (customVocabulary) wordReplacements else emptyList(), candidates)
    }
}
