# Hex words file (`.hexwords`), version 1

A Hex words file carries a person's **vocabulary** (names Hex should spell right, with the
spellings a transcript shows instead) and their **word replacements** (heard words → what to
write) from one Hex to another: the Mac, Windows or Linux app, Hex for iPhone and Hex for
Android. Any Hex can write one. Any Hex can open one. The person moves it themselves (AirDrop,
Files, Messages, email, a USB stick, Nearby Share). Nothing is sent anywhere by Hex.

This page is the whole contract. The three apps implement it separately and prove they agree
with the shared cases in [`tests/fixtures/hex-words/`](../../tests/fixtures/hex-words/). Version 1
is frozen once this page merges: a change that a version 1 reader would misread needs a new
`format` name, and anything else is a new `version` (see [Versions](#versions)).

## In one minute

- One UTF-8 JSON object, at most **8 MiB**, extension **`.hexwords`**.
- `"format": "hex.words"`, `"version": 1`, an optional `madeBy`, and two optional lists:
  `vocabulary` and `replacements`.
- A reader **refuses** a file that isn't one (too large, not UTF-8, not JSON, too deeply nested,
  a key repeated, another format, a bad version) and keeps nothing.
- Otherwise it **reads what it can**. Each entry that breaks a rule is **skipped** with a
  reason from a fixed list, and the rest is read.
- Nothing is applied until the person has seen the **review sheet** and tapped Add. Adding
  **merges** into their lists and never removes anything. A clash keeps theirs unless they
  pick the file's. **Undo** puts the lists back.

## Why JSON

- **No new dependency anywhere:** `serde_json` on the desktop and Foundation's
  `JSONSerialization` on the iPhone, each after the check pass below. On Android, a small strict
  parser in Kotlin, grown from that check pass: `org.json` is lenient, and neither it nor
  `android.util.JsonReader` runs in the app's JVM unit tests.
- **It holds both lists.** The desktop's CSV (Teams' custom dictionary columns) has room for
  terms and their spellings only. Adding replacements to it would break the Teams layout people
  already use, so the CSV stays as it is, beside this file.
- **A person can read it.** Opened in a text editor, it says what it holds. That matters for a
  file people send to each other.
- **It has a schema** ([`hex-words.schema.json`](../../tests/fixtures/hex-words/hex-words.schema.json))
  that a writer's output is checked against. A property list would be Apple-only.
- It is the same choice as the iPhone's typing configs (`.mhexconfig`). This is a different
  file, though: a typing config describes one phone's keyboard, every part of it needs evidence,
  and it applies only with Full Access.

## The file

```json
{
  "format": "hex.words",
  "version": 1,
  "madeBy": {"app": "Hex for Mac", "version": "3.1.0"},
  "vocabulary": [
    {"term": "OpenCode", "soundsLike": ["open code", "open coat"]},
    {"term": "Rune"}
  ],
  "replacements": [
    {"heard": "jet hub", "write": "GitHub"},
    {"heard": "um", "write": ""},
    {"heard": "cloud code", "write": "Claude Code", "learned": true}
  ]
}
```

| Field | Required | What it is |
| --- | --- | --- |
| `format` | yes | Exactly `"hex.words"`, case included. |
| `version` | yes | A whole number, at least 1. This page is version 1. |
| `madeBy` | no | `{app, version}`, both strings: which Hex wrote the file. It is shown as the file's claim, never trusted. |
| `vocabulary` | no | A list of `{term, soundsLike}`. `term` is a string; `soundsLike` is an optional list of strings: spellings a transcript may show instead of the term. |
| `replacements` | no | A list of `{heard, write, learned}`. `heard` is the words to find, matched whole-phrase and ignoring case. `write` is what replaces them, kept exactly; empty means delete them. `learned` is optional: `true` when a phone learned the fix rather than the person typing it. |

A missing list is the same as an empty one. A file with neither list reads as "nothing to
import", not as an error.

### Limits

| What | Limit | Counted as |
| --- | --- | --- |
| The file | 8 MiB (8,388,608 bytes) | bytes, before anything else |
| Nesting | 8 levels (the top object is level 1) | `{` and `[` outside strings |
| `vocabulary` | 500 terms | entries read so far |
| `term` | 64 characters | Unicode code points, after cleaning |
| `soundsLike` | 16 per term, each at most 64 characters and 8 words | as above |
| `replacements` | 1,000 | entries read so far |
| `heard` | 128 characters, 8 words | as above |
| `write` | 1,000 characters, may be empty | as above |
| `madeBy.app` / `madeBy.version` | 64 / 32 characters | as above |

A file that holds every list at its limits, written in 4-byte characters, is about 6.5 MiB. So a
writer that keeps to the limits always fits in 8 MiB.

## Cleaning text

Every string is cleaned before it is checked, compared or kept. The steps below are the same
on every platform.

1. **Compose it:** Unicode NFC. A decomposed "e + ◌̈" becomes "ë".
2. **Spaces, for `term`, `soundsLike`, `heard` and `madeBy`:** every run of whitespace
   becomes one space (U+0020), and spaces at either end are removed. *Whitespace* means exactly
   these code points, the Unicode White_Space set: U+0009–U+000D, U+0020, U+0085, U+00A0, U+1680,
   U+2000–U+200A, U+2028, U+2029, U+202F, U+205F, U+3000. Don't use a platform's own "is
   whitespace" test. Java's, for one, leaves out U+00A0 and adds U+001C–U+001F.
3. **`write` keeps its spaces:** it is never trimmed or collapsed. CR LF and a lone CR become
   LF, and nothing else changes.
4. **Hidden characters:** after steps 1–3, an entry or spelling that still holds any of these is
   skipped as `hiddenCharacters`. Nothing is removed silently, because a word that hides a
   character doesn't type what it shows.
   - control characters U+0000–U+001F and U+007F–U+009F. The one exception is `write`, which may
     keep LF (U+000A) and TAB (U+0009);
   - soft hyphen U+00AD, Arabic letter mark U+061C, Mongolian vowel separator U+180E,
     zero-width space U+200B, left-to-right and right-to-left marks U+200E and U+200F;
   - direction embeddings and overrides U+202A–U+202E, and isolates U+2066–U+2069;
   - word joiner and invisible operators U+2060–U+2064, byte order mark U+FEFF, interlinear
     annotation marks U+FFF9–U+FFFB.

   The zero-width non-joiner U+200C and joiner U+200D are allowed. Persian and the Indic scripts
   spell with them, and emoji such as 👩‍💻 are joined by them.
5. **Counting characters:** Unicode code points (Rust `chars().count()`, Swift
   `unicodeScalars.count`, Kotlin `codePointCount`), never UTF-16 units or graphemes.
6. **Counting words:** the number of pieces between single spaces after step 2.
7. **A letter or digit** is a code point in Unicode general category L (letters) or N (numbers).
   A term, spelling or heard phrase with none is skipped as `noLetters`, and so is one that
   is empty after cleaning.
8. **The key** two strings are compared by: the cleaned text, lowercased with the full,
   locale-independent Unicode mapping (`İ` → `i̇`), and then with every final sigma `ς`
   (U+03C2) replaced by `σ` (U+03C3). Platforms disagree on final sigma, and this step makes them
   agree. "OpenCode", "opencode" and "OPENCODE" share a key. "Open  Code" and "open code" share a
   key. "open code" and "OpenCode" don't.

## Reading, step by step

A reader takes the file's bytes and produces a **result**:

- **refused**, with one reason; or
- **read**, with:
  - the version, and whether it is newer than the reader;
  - `madeBy`;
  - the entries it accepted (cleaned);
  - and every part it skipped, as `{path, reason}`.

A refused file is never shown entry by entry and nothing of it is kept.

1. **Larger than 8 MiB:** refuse, `tooLarge`, before reading it. With a stream of unknown
   length, read at most 8 MiB + 1 byte.
2. A UTF-8 byte order mark (EF BB BF) at the very start is dropped.
3. **Not valid UTF-8** (any bad byte, an overlong form, a surrogate encoded in UTF-8, or
   UTF-16): refuse, `notUTF8`.
4. **Empty, or only spaces, tabs and line breaks:** refuse, `empty`.
5. **The check pass.** One walk over the text, before any parser builds values. It refuses
   with the first of these problems it meets:
   - `tooDeep`: more than 8 levels of `{` or `[` outside strings. The walk never recurses,
     so 10,000 levels can't exhaust a stack;
   - `duplicateKey`: an object that repeats a key, comparing keys after decoding escapes
     (`"version"` and `"versio\u006e"` are the same key);
   - `notJSON`: anything RFC 8259 doesn't allow.

   Strict JSON allows no comments, no trailing commas, no single quotes or unquoted keys, no
   `NaN` or `Infinity`, no leading zeros, and exactly one value. It also allows no raw control
   character U+0000–U+001F inside a string, and no `\u` escape for a lone surrogate. Most
   parsers check some of these and not others. That is why every platform checks all of them.
6. **The top level isn't an object:** refuse, `notAnObject`.
7. **`format` isn't the string `"hex.words"`:** refuse, `notHexWords`. A typing config is
   refused here.
8. **`version` is missing, not a number, not whole, or below 1:** refuse, `badVersion`.
   `1.0` counts as 1. `"1"`, `true`, `0`, `1.5` and `-1` don't.
9. **Top-level keys other than the five above:** skipped as `unknownField` (path: the key).
10. **`madeBy`:** if it isn't an object, `badField`. Each of `app` and `version` that isn't
    a string, is empty, too long or has hidden characters after cleaning is `badField` and left
    out. Other keys inside it are `unknownField`.
11. **`vocabulary`**, if present and a list (otherwise the whole part is `badPart`). For each
    entry, in order, the first rule that applies decides:
    1. the part already holds 500 terms → `overLimit`, with no other check;
    2. not an object, or `term` missing or not a string → `badEntry`;
    3. hidden characters in the term → `hiddenCharacters`;
    4. no letter or digit → `noLetters`;
    5. over 64 characters → `tooLong`;
    6. the same key as a term already accepted → `duplicate` (the whole entry, spellings
       included);
    7. accepted. Then its other keys are checked: a `soundsLike` that isn't a list is
       `badField` and the term keeps no spellings, and any other key is `unknownField`.
       Each spelling in turn: the term already has 16 → `overLimit`; not a string →
       `badEntry`; hidden characters, no letter or digit, over 64 characters, over 8 words →
       `hiddenCharacters`, `noLetters`, `tooLong`, `tooManyWords`; the term's own key →
       `sameAsTerm`; a key this term already has → `duplicate`; otherwise kept.
12. **`replacements`**, if present and a list (otherwise `badPart`). For each entry:
    1. the part already holds 1,000 → `overLimit`;
    2. not an object, or `heard` or `write` missing or not a string → `badEntry`;
    3. hidden characters in either → `hiddenCharacters`;
    4. no letter or digit in `heard` → `noLetters`;
    5. `heard` over 128 or `write` over 1,000 characters → `tooLong`;
    6. `heard` over 8 words → `tooManyWords`;
    7. the same key as a heard phrase already accepted → `duplicate`;
    8. accepted, with `learned` false unless it is `true`. A `learned` that isn't
       `true` or `false` is `badField`, and the entry is read as not learned. Other keys
       are `unknownField`.

Paths name the place in the file: `vocabulary[3]`, `vocabulary[3].soundsLike[0]`,
`replacements[0].learned`, `madeBy.app`, `colors`. Unknown fields inside a skipped entry are
not listed.

### Reasons

| Refusals (nothing is kept) | |
| --- | --- |
| `tooLarge` | over 8 MiB |
| `notUTF8` | not UTF-8 |
| `empty` | nothing but whitespace |
| `tooDeep` | nested more than 8 levels |
| `duplicateKey` | an object repeats a key |
| `notJSON` | not strict JSON |
| `notAnObject` | the top level isn't an object |
| `notHexWords` | `format` isn't `hex.words` |
| `badVersion` | `version` isn't a whole number ≥ 1 |

| Skips (the rest is read) | |
| --- | --- |
| `badPart` | `vocabulary` or `replacements` isn't a list |
| `badEntry` | an entry or spelling of the wrong shape |
| `badField` | an optional field of the wrong shape (only that field is dropped) |
| `unknownField` | a key this version doesn't know (ignored) |
| `hiddenCharacters` | see [Cleaning text](#cleaning-text) |
| `noLetters` | no letter or digit, or empty |
| `tooLong` | over its character limit |
| `tooManyWords` | over 8 words |
| `duplicate` | a key already read |
| `sameAsTerm` | a spelling equal to its own term |
| `overLimit` | past 500 terms, 16 spellings or 1,000 replacements |

These names are stable identifiers for tests and logs. The words a person sees are each app's
own, for example "3 skipped: longer than 64 characters".

## Versions

- A version 1 reader reads any `version` ≥ 1. For a newer one it reads the fields it knows,
  lists the rest as `unknownField`, and the review sheet says the file was made by a newer
  Hex.
- So a later version may only **add** fields. A field it adds must be safe to ignore: dropping
  it must leave entries that still mean what they say. A change that would make a version 1
  reader misread a file needs a new `format` name, which version 1 readers refuse.
- Writers write `"version": 1` until this page says otherwise.

## Writing

Every writer:

- writes UTF-8 without a byte order mark, as one JSON object with `format`, `version: 1`,
  `madeBy`, `vocabulary` and `replacements`. It writes both lists even when empty, and keeps
  each list in the app's own order;
- sets `madeBy.app` to "Hex for Mac", "Hex for Windows", "Hex for Linux", "Hex for iPhone" or
  "Hex for Android", and `madeBy.version` to the app's version;
- writes text **cleaned** as above, and writes non-ASCII as UTF-8 rather than `\u` escapes;
  any whitespace layout is fine;
- leaves out every entry a reader would skip, applying the same rules and limits in the same
  order, and tells the person how many it left out and why. It never writes a file its own
  reader would skip anything in. Its output passes `hex-words.schema.json`;
- writes `soundsLike` only when a term has spellings, and `learned` only when it is `true`;
- names the file `hex-words.hexwords` and writes it atomically (write, then rename).

What each app exports:

- **Desktop:**
  - the shared Vocabulary with its spellings;
  - the **Global** mode's replacements (`dictation_processing.default_mode.replacements`:
    `matched_phrase` → `heard`, `output` → `write`), deletions included.

  Other modes' replacements never travel. They are tied to Mac, Windows and Linux apps and
  websites.
- **iPhone and Android:**
  - Custom vocabulary's terms (no spellings);
  - the new **Word replacements** list;
  - optionally, after them, **learned dictation fixes**. This is behind a switch in the export
    sheet that is off by default. Only fixes in use are written: applied and not switched off.
    They are written with `learned: true`. A learned fix whose heard words the person already
    replaces is left out as `duplicate`.

## Arrival: the review sheet and merging

Opening a file reads it. A refusal shows one sentence with the reason and keeps nothing.
Otherwise the review sheet shows:

- what will be added;
- what is already there;
- each **clash**, with *Keep mine* (the default) or *Use this one*;
- what can't be used on this device, grouped by reason, one plain sentence each;
- what was skipped in the file.

Each part, Vocabulary and Replacements, has its own switch. Nothing changes until **Add**.
Cancel leaves everything as it was.

Merging runs in file order: vocabulary first, then replacements. On a phone, the spellings that
become replacements come last.

| Case | Desktop | iPhone and Android |
| --- | --- | --- |
| New term | added at the end, with its spellings | added at the end, without spellings |
| Same term (desktop: the key; phone: lowercase letters and digits only, so "Next.js" = "NextJS") | gains the spellings it lacks, up to 16; your casing stays; counted as updated, or already there if nothing was new | already there |
| Term's spellings | kept on the term | each becomes a replacement `spelling → term`, after the file's own replacements, in the **Replacements** part |
| Term with fewer than 2 letters or digits ("C#") | added | not kept (`tooShortForThisDevice`); its spellings still become replacements |
| Term list full | `overDeviceLimit` (500) | `overDeviceLimit` (100) |
| New replacement (heard key not in the list) | added at the end of the Global mode's list | added at the end of Word replacements |
| Same heard key, same `write` (after NFC) | already there | already there |
| Same heard key, another `write`: a **clash** | yours stays unless the person picks theirs, which replaces the output in place | the same |
| A spelling-replacement with the same heard key as one of the file's replacements | n/a | `duplicate`: the file's replacement wins |
| Empty or blank `write` (a deletion) | added | not supported yet (`deletionNotSupported`) |
| Replacement list holds 1,000 | `overDeviceLimit` | `overDeviceLimit` |
| `learned` | dropped: it becomes the person's own replacement | dropped the same way; never written into what the phone learned |
| A part switched off | that part is left alone | the same; a phone's spelling-replacements belong to Replacements |

- Entries the merge can't use are listed as `{path, reason}`. The reasons are stable
  identifiers: `overDeviceLimit`, `tooShortForThisDevice`, `deletionNotSupported` and `duplicate`.
  Clashes are listed with the person's words and the file's.
- The desktop's 1,000 applies to importing only. A Global list that already holds more keeps
  them all.
- A phone's merge reads and changes only Custom vocabulary and Word replacements. It never
  changes the learned fixes. When dictating, a Word replacement wins over a learned fix with the
  same heard words.

**Undo.** Before Add, the app keeps a copy of both lists. A phone also keeps the state of any
switch the sheet turned on (Custom vocabulary). Undo puts that copy back exactly. It is offered
in the confirmation after Add and as "Undo last import" in the list's settings until the person
edits either list or imports again, and it survives a restart. The copy holds the lists, never
the file.

**The confirmation** says what happened in one line, for example "Added 12 terms and 4
replacements · 2 already there · kept yours for 1 · Undo", or "No new words in that file".

## File name and type

| | Desktop (macOS, Windows, Linux) | iPhone | Android |
| --- | --- | --- | --- |
| Extension | `.hexwords` | `.hexwords` | `.hexwords` |
| MIME type | `application/vnd.anomaly.hex-words+json` (declared by Hex; not registered with IANA) | the same | the same |
| Opening | **Open…** in the Vocabulary sheet: a file dialog filtered to `*.hexwords`, with All files | exported type `anomaly.hex.words`, conforming to `public.json`, tags `hexwords` and the MIME type; a document type Hex owns (`LSHandlerRank` Owner), copied in (`LSSupportsOpeningDocumentsInPlace` NO); AirDrop, Files and Open in Hex all arrive there | an `ACTION_VIEW` filter and an `ACTION_SEND` filter (below), plus **Open a words file…** in Settings (`ACTION_OPEN_DOCUMENT`, `*/*`) |
| Saving | **Export…**: a save dialog named `hex-words.hexwords` | the share sheet (AirDrop, Save to Files…) | the share sheet (`ACTION_SEND` through a `FileProvider`) and Save to… (`ACTION_CREATE_DOCUMENT`) |
| Size a reader must accept | 8 MiB | 8 MiB | 8 MiB |

Android's filters, on the activity that shows the review sheet:

```xml
<intent-filter>
    <action android:name="android.intent.action.VIEW" />
    <category android:name="android.intent.category.DEFAULT" />
    <data android:scheme="content" />
    <data android:mimeType="application/vnd.anomaly.hex-words+json" />
    <data android:mimeType="application/json" />
    <data android:mimeType="application/octet-stream" />
</intent-filter>
<intent-filter>
    <action android:name="android.intent.action.SEND" />
    <category android:name="android.intent.category.DEFAULT" />
    <data android:mimeType="application/vnd.anomaly.hex-words+json" />
    <data android:mimeType="application/json" />
    <data android:mimeType="application/octet-stream" />
</intent-filter>
```

- Android knows no extension for `.hexwords`, so mail and file apps label the file
  `application/octet-stream` or `application/json`. Hex is therefore offered for those too,
  and the reader refuses what isn't a words file.
- There is no `BROWSABLE` category, so a web page can't open Hex with a file.
- Every platform decides by the content, never by the name: a `hex-words.hexwords.json` or
  `.txt` that a mail app renamed still opens.

## What a reader must never do

A words file is something a stranger can send. Treat every byte of it as untrusted, whoever
sent it.

1. **Never run anything from it.** No string is a command, a script, a transform, a format
   string or a pattern. Replacements match their words literally. The desktop builds its
   matcher with `regex::escape`, and phones compare words.
2. **Never follow a link.** A string that looks like a URL, an email address or a phone number
   is plain text. The review sheet shows strings verbatim: no Markdown (SwiftUI
   `Text(verbatim:)`), no link detection, no `Linkify`. Nothing in the file is fetched, and
   `madeBy` is never looked up.
3. **Never apply without the review sheet and the person's Add.** Not on launch, not from a URL
   scheme or QR code, not from a watched folder, not because the file came from another of
   their own devices.
4. **Never keep the file.**
   - Copy it to a private temporary place to read it, and delete the copy when the sheet closes:
     after Add, Cancel or a refusal.
   - On the iPhone, remove the Inbox copy.
   - On Android, don't take a persistable URI permission.
   - Undo keeps the lists from before, not the file.
5. **Never change anything but the two lists.** That means no settings, modes, models, Labs
   or switches, apart from a switch the person turns on in the sheet itself.
6. **Never log its contents.** Log counts and reasons only.
7. **Never trust its sizes.** Enforce the byte limit before reading, the depth limit before
   parsing, and the count and length limits while reading. Show long strings shortened (the
   first 64 characters and "…"), and the skipped list as counts with a few examples.
8. **Never let its text steer the screen.** Show each string in its own direction-isolated run,
   so a right-to-left term can't reorder the labels around it. Hidden direction characters are
   already refused (`hiddenCharacters`).
9. **Never use its strings as names:** not as file paths, preference keys or identifiers.

## Shared cases

[`tests/fixtures/hex-words/`](../../tests/fixtures/hex-words/) holds the cases each app's tests
load. Each case's `name` says in a sentence what it checks.

- **`read.json`**: about 50 files in `files/`, from a small one to hostile ones, each with the
  whole expected result. Two cases are generated rather than stored: the small file padded with
  spaces to exactly 8 MiB (read) and to 8 MiB + 1 byte (`tooLarge`).
- **`merge.json`**: a device's lists before, a file already read, the person's choices, and the
  lists, counts, clashes and refusals after, for the desktop and for a phone. Some cases set
  lower limits so they stay small. Undo after any case must give back `before` exactly.
- **`write.json`**: an app's own lists, and the file it must write (compared after parsing,
  without `madeBy`) with what it must leave out.
- **`hex-words.schema.json`**: what a writer's output must pass.

How each app loads them:

- **Desktop:** `include_str!("../../tests/fixtures/hex-words/read.json")` and the files by path,
  as `src/updater/protocol.rs` does with `tests/fixtures/release-protocol`.
- **iPhone:** the folder as a resource of the `HEXMobileTests` target in `ios/project.yml`
  (`- path: ../tests/fixtures/hex-words`, `type: folder`).
- **Android:** `sourceSets["test"].resources.srcDir("../../tests/fixtures/hex-words")` in
  `android/app/build.gradle.kts`.

A change to this page that changes any outcome changes the cases in the same pull request. All
three apps must then pass them again.

## Not in version 1

- Desktop modes other than Global, and their transformations, rewrite prompts and models.
- Typing shortcuts. Apple's Text Replacements already sync between Mac and iPhone and the
  iPhone keyboard reads them. Android's personal dictionary belongs to Android.
- The keyboards' learned words, typo fixes and "never correct" words.
- Deletions on phones, and removing anything on arrival ("replace my lists").
- Sync of any kind: iCloud, a shared folder, the local network, QR codes.
