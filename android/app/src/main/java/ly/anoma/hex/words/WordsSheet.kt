package ly.anoma.hex.words

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.colorResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import ly.anoma.hex.Action
import ly.anoma.hex.Card
import ly.anoma.hex.Label
import ly.anoma.hex.R
import ly.anoma.hex.Rule
import ly.anoma.hex.Toggle

private const val shownRows = 20
private const val shownExamples = 3

/** The review sheet: what a words file would add, what is already there, each clash, what this phone can't use
 * and what the file itself skipped, with a switch per part. Every string from the file is shown verbatim (plain
 * text, no links), shortened and in its own direction-isolated run ([WordsText.shown]). */
@Composable
internal fun WordsSheet(model: WordsModel, done: () -> Unit) {
    Box(Modifier.fillMaxSize().background(colorResource(R.color.page)).safeDrawingPadding(), contentAlignment = Alignment.TopCenter) {
        Column(Modifier.widthIn(max = 520.dp).fillMaxWidth().verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp).padding(top = 12.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            val state = model.state
            Row(Modifier.heightIn(min = 52.dp), verticalAlignment = Alignment.CenterVertically) {
                val added = state is WordsModel.State.Added && !state.merged.summary.nothingNew
                Label(stringResource(if (added) R.string.words_added_title else R.string.words_sheet_title),
                    size = 26, weight = FontWeight.SemiBold, lines = 2, mod = Modifier.semantics { heading() })
            }
            when (state) {
                null, WordsModel.State.Reading -> Label(stringResource(R.string.words_reading), muted = true)
                is WordsModel.State.Refused -> {
                    Label(stringResource(refusal(state.reason)), lines = Int.MAX_VALUE)
                    Buttons(stringResource(R.string.words_done), done)
                }
                is WordsModel.State.Review -> Review(model, state) { model.forget(); done() }
                is WordsModel.State.Added -> {
                    Label(confirmation(state.merged.summary), lines = Int.MAX_VALUE)
                    Buttons(stringResource(R.string.words_done), done, stringResource(R.string.words_undo), model::undo)
                }
                WordsModel.State.Undone -> {
                    Label(stringResource(R.string.words_undone), lines = Int.MAX_VALUE)
                    Buttons(stringResource(R.string.words_done), done)
                }
            }
        }
    }
}

@Composable
private fun Review(model: WordsModel, review: WordsModel.State.Review, cancel: () -> Unit) {
    val read = review.read
    val before = review.before
    // Counts and rows show the whole file; what Add does follows the switches.
    val whole = remember(review, model.useTheirs) { WordsMerge.phone(before, read, useTheirs = model.useTheirs) }
    val merged = remember(review, model.vocabulary, model.replacements, model.useTheirs) { model.preview(review) }
    val madeBy = listOfNotNull(read.madeByApp, read.madeByVersion)
    if (madeBy.isNotEmpty()) Label(stringResource(R.string.words_made_by, WordsText.shown(madeBy.joinToString(" "))),
        muted = true, size = 14, lines = 2)
    if (read.newerVersion) Label(stringResource(R.string.words_newer), muted = true, size = 14, lines = Int.MAX_VALUE)
    if (!review.customVocabulary) Card {
        Toggle(stringResource(R.string.words_turn_on), stringResource(R.string.words_turn_on_detail), model.turnOn,
            noteLines = Int.MAX_VALUE) { model.turnOn = it }
    }
    val s = whole.summary
    if (read.vocabulary.isNotEmpty()) Card {
        Toggle(stringResource(R.string.words_part_vocabulary), counts(s.addedTerms, s.alreadyHaveTerms, 0), model.vocabulary) {
            model.vocabulary = it
        }
        if (model.vocabulary) Rows(whole.after.terms.drop(before.terms.size).map(WordsText::shown))
    }
    if (read.replacements.isNotEmpty() || read.vocabulary.any { it.soundsLike.isNotEmpty() }) Card {
        Toggle(stringResource(R.string.words_replacements), counts(s.addedReplacements, s.alreadyHaveReplacements, whole.clashes.size),
            model.replacements) { model.replacements = it }
        if (model.replacements) {
            Rows(whole.after.replacements.drop(before.replacements.size).map { WordsText.shown(it.heard) + " → " + WordsText.shown(it.write) })
            for (clash in whole.clashes) {
                Rule()
                Clash(clash, clash.path in model.useTheirs) { theirs ->
                    model.useTheirs = if (theirs) model.useTheirs + clash.path else model.useTheirs - clash.path
                }
            }
        }
    }
    if (merged.notApplied.isNotEmpty()) Section(stringResource(R.string.words_not_usable)) {
        merged.notApplied.groupBy { it.reason }.entries.forEachIndexed { index, (reason, skips) ->
            if (index != 0) Rule()
            val texts = skips.map { WordsText.shown(text(read, it.path)) }
            val examples = texts.take(shownExamples).joinToString(", ") + if (texts.size > shownExamples) ", " + WordsText.ELLIPSIS else ""
            Line(pluralStringResource(notUsable(reason), skips.size, skips.size, examples))
        }
    }
    if (read.skipped.isNotEmpty()) Section(stringResource(R.string.words_skipped_title)) {
        read.skipped.groupingBy { it.reason }.eachCount().entries.forEachIndexed { index, (reason, count) ->
            if (index != 0) Rule()
            Line(stringResource(R.string.words_skipped, count, stringResource(skipped(reason))))
        }
    }
    Buttons(stringResource(R.string.words_add), model::add, stringResource(R.string.words_cancel), cancel)
}

@Composable private fun counts(added: Int, there: Int, clashes: Int) = buildList {
    add(stringResource(R.string.words_count_new, added))
    if (there > 0) add(stringResource(R.string.words_count_there, there))
    if (clashes > 0) add(stringResource(R.string.words_count_clash, clashes))
}.joinToString(" · ")

@Composable private fun Rows(items: List<String>) {
    for (item in items.take(shownRows)) { Rule(); Line(item, size = 16) }
    if (items.size > shownRows) { Rule(); Line(stringResource(R.string.words_more, items.size - shownRows), muted = true) }
}

@Composable private fun Line(text: String, size: Int = 14, muted: Boolean = false) =
    Label(text, size = size, muted = muted, lines = 3, mod = Modifier.padding(horizontal = 16.dp, vertical = 12.dp))

@Composable private fun Section(title: String, content: @Composable ColumnScope.() -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Label(title, muted = true, size = 14, mod = Modifier.padding(start = 16.dp))
        Card(content = content)
    }
}

@Composable private fun Clash(clash: WordsClash, theirs: Boolean, choose: (Boolean) -> Unit) {
    Column(Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp, top = 12.dp)) {
        Label(WordsText.shown(clash.heard), weight = FontWeight.Medium, lines = 2)
        Label(stringResource(R.string.words_yours, WordsText.shown(clash.yours)), muted = true, size = 13, lines = 3)
        Label(stringResource(R.string.words_theirs, WordsText.shown(clash.theirs)), muted = true, size = 13, lines = 3)
        Row(Modifier.selectableGroup()) {
            Choice(stringResource(R.string.words_keep_mine), !theirs) { choose(false) }
            Choice(stringResource(R.string.words_use_this), theirs) { choose(true) }
        }
    }
}

@Composable private fun Choice(text: String, selected: Boolean, pick: () -> Unit) {
    Box(Modifier.heightIn(min = 48.dp).selectable(selected, role = Role.RadioButton, onClick = pick).padding(end = 16.dp),
        contentAlignment = Alignment.CenterStart) {
        Label((if (selected) "✓ " else "") + text, size = 15, muted = !selected,
            weight = if (selected) FontWeight.SemiBold else FontWeight.Normal)
    }
}

/** A secondary text action and the one primary button, at the end of the row. */
@Composable private fun Buttons(primary: String, onPrimary: () -> Unit, secondary: String? = null, onSecondary: () -> Unit = {}) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End),
        verticalAlignment = Alignment.CenterVertically) {
        if (secondary != null) Action(secondary, onSecondary)
        Box(Modifier.heightIn(min = 48.dp).clip(RoundedCornerShape(14.dp)).background(colorResource(R.color.ink))
            .clickable(role = Role.Button, onClick = onPrimary).padding(horizontal = 24.dp), contentAlignment = Alignment.Center) {
            Label(primary, inverted = true, weight = FontWeight.SemiBold)
        }
    }
}

@Composable private fun confirmation(s: WordsSummary): String {
    val terms = if (s.addedTerms > 0) pluralStringResource(R.plurals.words_terms, s.addedTerms, s.addedTerms) else null
    val replacements = if (s.addedReplacements > 0)
        pluralStringResource(R.plurals.words_replacement_count, s.addedReplacements, s.addedReplacements) else null
    val parts = ArrayList<String>()
    when {
        terms != null && replacements != null -> parts += stringResource(R.string.words_added_both, terms, replacements)
        terms != null || replacements != null -> parts += stringResource(R.string.words_added, terms ?: replacements!!)
        s.changedReplacements == 0 -> parts += stringResource(R.string.words_nothing_new)
    }
    val there = s.alreadyHaveTerms + s.alreadyHaveReplacements
    if (there > 0) parts += stringResource(R.string.words_count_there, there)
    if (s.keptYours > 0) parts += stringResource(R.string.words_kept_yours, s.keptYours)
    if (s.changedReplacements > 0) parts += stringResource(R.string.words_used_theirs, s.changedReplacements)
    return parts.joinToString(" · ")
}

/** The file's own text at a merge path: a term, a spelling or a replacement's heard words. */
private fun text(read: WordsRead.Read, path: String): String {
    val numbers = Regex("\\d+").findAll(path).map { it.value.toInt() }.toList()
    return when {
        path.startsWith("replacements[") -> read.replacements[numbers[0]].heard
        numbers.size == 2 -> read.vocabulary[numbers[0]].soundsLike[numbers[1]]
        else -> read.vocabulary[numbers[0]].term
    }
}

private fun refusal(reason: String) = when (reason) {
    "tooLarge" -> R.string.words_refused_too_large
    "notUTF8" -> R.string.words_refused_not_utf8
    "empty" -> R.string.words_refused_empty
    "tooDeep" -> R.string.words_refused_too_deep
    "duplicateKey" -> R.string.words_refused_duplicate_key
    "notJSON" -> R.string.words_refused_not_json
    "notAnObject", "notHexWords" -> R.string.words_refused_not_hex_words
    "badVersion" -> R.string.words_refused_bad_version
    else -> R.string.words_refused_cant_open
}

private fun notUsable(reason: String) = when (reason) {
    "tooShortForThisDevice" -> R.plurals.words_not_usable_short
    "overDeviceLimit" -> R.plurals.words_not_usable_full
    "deletionNotSupported" -> R.plurals.words_not_usable_deletion
    else -> R.plurals.words_not_usable_duplicate
}

internal fun skipped(reason: String) = when (reason) {
    "badPart" -> R.string.words_skip_bad_part
    "badEntry" -> R.string.words_skip_bad_entry
    "badField" -> R.string.words_skip_bad_field
    "unknownField" -> R.string.words_skip_unknown_field
    "hiddenCharacters" -> R.string.words_skip_hidden
    "noLetters" -> R.string.words_skip_no_letters
    "tooLong" -> R.string.words_skip_too_long
    "tooManyWords" -> R.string.words_skip_too_many_words
    "duplicate" -> R.string.words_skip_duplicate
    "sameAsTerm" -> R.string.words_skip_same_as_term
    else -> R.string.words_skip_over_limit
}
