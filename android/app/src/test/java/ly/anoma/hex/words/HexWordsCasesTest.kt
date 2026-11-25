package ly.anoma.hex.words

import ly.anoma.hex.speech.WordReplacement
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.math.BigDecimal

/** The Hex words file's shared cases (tests/fixtures/hex-words, a test resource folder in build.gradle.kts):
 * every read case, and every merge and write case for a phone, run against Android's reader, merger and writer,
 * plus the writer's output checked against hex-words.schema.json. */
class HexWordsCasesTest {
    private fun resource(name: String): ByteArray =
        requireNotNull(javaClass.classLoader!!.getResourceAsStream(name)) { "missing fixture $name" }.use { it.readBytes() }

    private fun fixture(name: String) = StrictJson.parse(String(resource(name), Charsets.UTF_8), maxDepth = 64) as Json.Obj

    private fun cases(name: String) = (fixture(name).members["cases"] as Json.Arr).items.map { it as Json.Obj }

    private val Json.Obj.name get() = (members["name"] as Json.Str).value

    /** Plain Kotlin values, so assertEquals compares objects as maps and numbers by value. */
    private fun plain(value: Json?): Any? = when (value) {
        null, Json.Null -> null
        is Json.Obj -> value.members.mapValues { plain(it.value) }
        is Json.Arr -> value.items.map(::plain)
        is Json.Str -> value.value
        is Json.Bool -> value.value
        is Json.Num -> BigDecimal(value.text).let { if (it.stripTrailingZeros().scale() <= 0) it.toLong() else it.toDouble() }
    }

    private fun skips(list: List<WordsSkip>) = list.map { mapOf("path" to it.path, "reason" to it.reason) }

    private fun strings(value: Json?) = (value as Json.Arr?)?.items.orEmpty().map { (it as Json.Str).value }

    private fun str(value: Json?) = (value as Json.Str).value

    @Test fun everyReadCase() {
        val all = cases("read.json")
        for (case in all) {
            val bytes = case.members["generate"]?.let { generate ->
                val recipe = (generate as Json.Obj).members
                val base = resource(str(recipe["base"]))
                val size = (plain(recipe["padWithSpacesTo"]) as Long).toInt()
                ByteArray(size) { if (it < base.size) base[it] else ' '.code.toByte() }
            } ?: resource(str(case.members["file"]))
            val expect = (case.members["expect"] as Json.Obj).members
            val actual = WordsReader.read(bytes)
            if (str(expect["result"]) == "refused") {
                assertEquals(case.name, WordsRead.Refused(str(expect["reason"])), actual)
                continue
            }
            assertTrue("${case.name}: $actual", actual is WordsRead.Read)
            val read = actual as WordsRead.Read
            val madeBy = buildMap { read.madeByApp?.let { put("app", it) }; read.madeByVersion?.let { put("version", it) } }
            val shown = mapOf("version" to read.version, "newerVersion" to read.newerVersion, "madeBy" to madeBy,
                "vocabulary" to read.vocabulary.map { mapOf("term" to it.term, "soundsLike" to it.soundsLike) },
                "replacements" to read.replacements.map { mapOf("heard" to it.heard, "write" to it.write, "learned" to it.learned) })
            assertEquals(case.name, plain(Json.Obj(expect.filterKeys { it in shown })), shown)
            if (expect["skippedCounts"] != null) {
                assertEquals(case.name, plain(expect["skippedCounts"]), read.skipped.groupingBy { it.reason }.eachCount().mapValues { it.value.toLong() })
            } else {
                assertEquals(case.name, (plain(expect["skipped"]) as List<*>).toSet(), skips(read.skipped).toSet())
            }
        }
        assertTrue(all.size >= 50)
    }

    @Test fun everyPhoneMergeCaseAndItsUndo() {
        val phone = cases("merge.json").filter { str(it.members["receiver"]) == "phone" }
        for (case in phone) {
            val m = case.members
            val before = lists(m["before"] as Json.Obj)
            val incoming = (m["incoming"] as Json.Obj).members
            val read = WordsRead.Read(1, false, null, null,
                (incoming["vocabulary"] as Json.Arr).items.map { val e = (it as Json.Obj).members; WordsTerm(str(e["term"]), strings(e["soundsLike"])) },
                (incoming["replacements"] as Json.Arr).items.map {
                    val e = (it as Json.Obj).members
                    WordsReplacement(str(e["heard"]), str(e["write"]), (e["learned"] as Json.Bool?)?.value ?: false)
                }, emptyList())
            val limits = (m["limits"] as Json.Obj?)?.members.orEmpty()
            val parts = (m["parts"] as Json.Obj?)?.members.orEmpty()
            val choices = (m["choices"] as Json.Obj?)?.members.orEmpty().filterValues { str(it) == "useTheirs" }.keys
            val merged = WordsMerge.phone(before, read, (parts["vocabulary"] as Json.Bool?)?.value ?: true,
                (parts["replacements"] as Json.Bool?)?.value ?: true, choices,
                (plain(limits["terms"]) as Long?)?.toInt() ?: WordsMerge.PHONE_TERMS,
                (plain(limits["replacements"]) as Long?)?.toInt() ?: WordsMerge.PHONE_REPLACEMENTS)
            val s = merged.summary
            val actual = mapOf(
                "after" to mapOf("vocabulary" to merged.after.terms,
                    "replacements" to merged.after.replacements.map { mapOf("heard" to it.heard, "write" to it.write) }),
                "summary" to mapOf("addedTerms" to s.addedTerms, "updatedTerms" to 0, "alreadyHaveTerms" to s.alreadyHaveTerms,
                    "addedReplacements" to s.addedReplacements, "changedReplacements" to s.changedReplacements,
                    "keptYours" to s.keptYours, "alreadyHaveReplacements" to s.alreadyHaveReplacements).mapValues { it.value.toLong() },
                "notApplied" to skips(merged.notApplied),
                "clashes" to merged.clashes.map { mapOf("path" to it.path, "heard" to it.heard, "yours" to it.yours, "theirs" to it.theirs) })
            assertEquals(case.name, plain(m["expect"]), actual)
            // Undo gives back before exactly, the switch included.
            for (on in listOf(true, false)) assertEquals(case.name, WordsUndo(before, on), WordsUndo.decode(WordsUndo(before, on).encode()))
        }
        assertTrue(phone.size >= 10)
    }

    private fun lists(value: Json.Obj) = WordsLists(strings(value.members["vocabulary"]),
        (value.members["replacements"] as Json.Arr).items.map { val e = (it as Json.Obj).members; WordReplacement(str(e["heard"]), str(e["write"])) })

    @Test fun everyPhoneWriteCase() {
        val schema = fixture("hex-words.schema.json")
        val phone = cases("write.json").filter { str(it.members["writer"]) == "phone" }
        for (case in phone) {
            val input = (case.members["input"] as Json.Obj).members
            val learned = (input["learnedFixes"] as Json.Arr).items.map {
                val e = (it as Json.Obj).members
                LearnedFix(str(e["heard"]), str(e["meant"]), (e["active"] as Json.Bool).value)
            }
            val written = WordsWriter.phone("0.2.2", strings(input["vocabulary"]),
                lists(Json.Obj(mapOf("vocabulary" to Json.Arr(emptyList()), "replacements" to input["replacements"]!!))).replacements,
                learned, (input["includeLearned"] as Json.Bool).value)
            checkWritten(case.name, written, schema)
            val expect = (case.members["expect"] as Json.Obj).members
            val file = StrictJson.parse(String(written.bytes, Charsets.UTF_8)) as Json.Obj
            assertEquals(case.name, plain(expect["file"]), plain(Json.Obj(file.members - "madeBy")))
            assertEquals(case.name, (plain(expect["notExported"]) as List<*>).toSet(), skips(written.notExported).toSet())
        }
        assertTrue(phone.size >= 2)
    }

    /** UTF-8 without a byte order mark, madeBy.app set, passes the schema and reads back with nothing skipped. */
    private fun checkWritten(name: String, written: WordsWritten, schema: Json.Obj) {
        assertFalse(name, written.bytes.size >= 3 && written.bytes[0] == 0xEF.toByte())
        val back = WordsReader.read(written.bytes) as WordsRead.Read
        assertEquals(name, emptyList<WordsSkip>(), back.skipped)
        assertEquals(name, WordsWriter.APP, back.madeByApp)
        val errors = ArrayList<String>()
        Schema(schema).check(schema, StrictJson.parse(String(written.bytes, Charsets.UTF_8)), "", errors)
        assertEquals(name, emptyList<String>(), errors)
    }

    /** The cross-check's Android sample (hex-words-crosscheck/README.md), written to build/hexwords/android.hexwords
     * to compare with expected.json's apps.android.file. */
    @Test fun crossCheckSampleExport() {
        val written = WordsWriter.phone("0.2.2", listOf("Hex", "Zoë", "שלום"),
            listOf(WordReplacement("toda", "תודה"), WordReplacement("open code", "OpenCode")), emptyList(), false)
        checkWritten("android sample", written, fixture("hex-words.schema.json"))
        assertEquals(emptyList<WordsSkip>(), written.notExported)
        File("build/hexwords").apply { mkdirs() }.resolve(WordsWriter.FILE_NAME.replace("hex-words", "android")).writeBytes(written.bytes)
    }

    /** The JSON Schema 2020-12 keywords hex-words.schema.json uses, and nothing more. */
    private class Schema(private val root: Json.Obj) {
        fun check(schema: Json.Obj, value: Json, at: String, errors: MutableList<String>) {
            val s = schema.members
            (s["\$ref"] as Json.Str?)?.let { ref ->
                val name = ref.value.removePrefix("#/\$defs/")
                check(((root.members["\$defs"] as Json.Obj).members[name]) as Json.Obj, value, at, errors)
            }
            (s["allOf"] as Json.Arr?)?.items?.forEach { check(it as Json.Obj, value, at, errors) }
            s["const"]?.let { if (it != value) errors += "$at: not $it" }
            when ((s["type"] as Json.Str?)?.value) {
                "object" -> if (value !is Json.Obj) { errors += "$at: not an object"; return }
                "array" -> if (value !is Json.Arr) { errors += "$at: not a list"; return }
                "string" -> if (value !is Json.Str) { errors += "$at: not a string"; return }
            }
            if (value is Json.Obj) {
                val properties = (s["properties"] as Json.Obj?)?.members.orEmpty()
                (s["required"] as Json.Arr?)?.items?.forEach { if ((it as Json.Str).value !in value.members) errors += "$at: no ${it.value}" }
                for ((key, member) in value.members) {
                    val sub = properties[key]
                    if (sub != null) check(sub as Json.Obj, member, "$at.$key", errors)
                    else if (s["additionalProperties"] == Json.Bool(false)) errors += "$at: $key not allowed"
                }
            }
            if (value is Json.Arr) {
                (s["minItems"] as Json.Num?)?.let { if (value.items.size < it.text.toInt()) errors += "$at: too few" }
                (s["maxItems"] as Json.Num?)?.let { if (value.items.size > it.text.toInt()) errors += "$at: too many" }
                if (s["uniqueItems"] == Json.Bool(true) && value.items.toSet().size != value.items.size) errors += "$at: repeated"
                (s["items"] as Json.Obj?)?.let { items -> value.items.forEachIndexed { i, item -> check(items, item, "$at[$i]", errors) } }
            }
            if (value is Json.Str) {
                val length = value.value.codePointCount(0, value.value.length)
                (s["minLength"] as Json.Num?)?.let { if (length < it.text.toInt()) errors += "$at: too short" }
                (s["maxLength"] as Json.Num?)?.let { if (length > it.text.toInt()) errors += "$at: too long" }
                (s["pattern"] as Json.Str?)?.let { if (!Regex(it.value).containsMatchIn(value.value)) errors += "$at: fails ${it.value}" }
            }
        }
    }
}
