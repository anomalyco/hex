# Settings and Home

Part of [Hex for iPhone](../README.md).

Home's single screen and the four Settings pages, the experiments list,
and every `--snap-state` scene the app and the keyboard render.

## Settings (`ContentView`, `HEXMobile/Settings/`)

Home fits one screen: the
Hex header, one status line ("Ready" with a check, or "Finish setup · 1 step ›"),
Dictate, and four rows that each open a short page, then **iOS Settings ↗**
(Hex's page in iOS Settings) and **Run setup again**, and last **Privacy
policy**, **Support**, **Send feedback** (only in a build with a report key; see
[Send feedback](#send-feedback)) and **Acknowledgements**. Privacy policy (App Review
5.1.1(i)) and Support open in Safari inside Hex (`HexLinks`, one base URL:
`https://dev.anoma.ly/hex` until the production site serves
`anoma.ly/hex/privacy`). Each row has a one-line description at most. While Home or its
setup sheet shows (and Hex is active), Home reads the setup state once a second
(`KeyboardSetupMonitor.refresh`), as well as when Hex comes to the foreground and when the
keyboard reports Full Access, so a change made with Hex still in front (the keyboard's Allow
Paste flag in one of Hex's own fields, a keyboard added from Settings beside Hex in iPad
Split View) shows within a second; on a Settings page it doesn't run. Home's speech model
card opens Settings › Dictation at Speech model in keyboard; the page opened from Home's
list starts at the top. The pages, one file each in `HEXMobile/Settings/`
(`DictationSettingsPage`, `KeyboardSettingsPage`, `PrivacySettingsPage`,
`ExperimentsSettingsPage`):

- **Dictation:** Style, Dictation language and Translate what I say; under **Formatting**,
  Lowercase, Format lists, and Spoken punctuation, Numbers as digits and End with a period,
  those three hidden while Style is Verbatim, which ignores them (their values are kept);
  **Live words** (iOS 26 and later): **Off**, **In bar** or **Into field**, one choice
  stored in Show what I say's and Type as I speak's two App Group keys (`LiveDictationMode`:
  Into field when Type as I speak's is on, In bar when only Show what I say's is, Off when
  neither; picking one writes both, so the two together, which 1.0.4 allowed, keep working
  until another is picked); then Clean up what I say and Custom vocabulary (its list below
  the switch); and last Speech model in keyboard, then **Model**: **English** (the
  default) or **Multilingual** (`SpeechModelChoice`, from Experiments until 2026-11-24, on
  iPhones with 6 GB or more), with what the picked one means and its status below,
  Keep mic warm and Microphone.
- **Keyboard:** Keyboard, **Full** (the default) or **Dictation bar** (stored
  as `FullKeyboardSettings.isEnabled`, App Group key `experiments.fullKeyboard`
  from when the full keyboard was an experiment: unset or true is Full, false
  the dictation bar, so testers who turned that experiment off keep the bar;
  an iPad follows the same choice, where until build 117 it always showed the bar), with
  Auto-start (Never, On switch by default, or Always) below it while the dictation bar is
  chosen, and Languages, Hex key and Number row (off by default) while Full is; under
  **Typing**, only while Full is chosen, Auto-capitalization, Autocorrect (with Spatial
  autocorrect below it while it's on), Swipe typing, Suggest swear words and Vocabulary (Hex's
  custom vocabulary list, how many words it holds); under **Feedback**, Key pop-ups (Full
  only), Haptics (the dictation bar uses it too; Strength below it while it's on: Light, the
  default until you choose, Medium or Strong; choosing one plays a key tick at it), Emoji key,
  Emoji suggestions (Full only; the strip's emoji, with or without the key) and Clear emoji
  history while either is on, which asks first; and a field to try the keyboard in.
- **Privacy:** "Everything stays on this iPhone", Learn how I type (what it
  learned below the switch), and under **Advanced**, Record typing logs (off
  by default) with its Typing logs viewer while it's on; last, under **Feedback**, what
  Send feedback sends and when (only in a build with a report key).
- **Experiments**, in four groups: **Dictation**: Two-language dictation, Voice isolation and Clean up with Apple Intelligence;
  **Languages**: Hinglish (Hindi in Latin letters), Greeklish (Greek in Latin letters),
  Korean keyboard (두벌식), Swipe typing in more languages and German spatial autocorrect
  (greyed, its value kept, unless Autocorrect and Spatial autocorrect are on); **Keyboard
  layout**: Comma and period keys, Dvorak punctuation keys, Quick accents, ABC under the
  emoji key, Keyboard themes (its Theme menu below the switch while it's on) and Join the
  keyboard when unfolded (only on a foldable iPhone, `FullKeyboardView.isFoldable`);
  **Typing**: Adaptive touch (its status and Reset touch learning below the switch),
  Two-thumb selection, Delete swiped words letter by letter, Hide word suggestions, Emoji
  shortcodes and Learn each language separately. 21 switches, 20 on an iPhone that doesn't fold.
  Last, **Send feedback**, for testers to say how an experiment went (only with a report key).

Simulator scenes: `--snap-state settings-home` (ready), `setup` (keyboard
added, Full Access not confirmed yet), `setup-sheet`, `setup-sheet-microphone`
(before iOS has asked for the microphone), and
`settings-<dictation|keyboard|privacy|experiments>[-on][-end]`, and
`model-card-<waiting|downloading|ready>` (Home's speech model card, with English
dictation and no keyboard limit, so nothing downloads). Each settings scene writes
its page's switches to the App Group: off by default, on with `-on`; switches that are
on by default (Spatial autocorrect, Swipe typing, Emoji key, Emoji suggestions) stay on.
`-end` starts the page scrolled to the bottom. `settings-privacy-confirm`
opens Record typing logs' consent dialog. `--experiment-keyboard-themes` turns
Keyboard themes on, and `--keyboard-theme automatic|black|white|navy|forest|ember|burgundy`
picks the theme. A theme colours the keys only (every keycap, special keys included, their
pressed and held states, the key pop-ups and the labels), the same in light and dark; the
background stays iOS's glass, matching the band iOS draws above a third-party keyboard and
the globe row below it, and the strip, emoji panel, menus and dictation bar keep the system
look.

## Send feedback (`HEXMobile/Feedback/`)

A form that sends one report to Bellhop, Anomaly's feedback inbox
(`POST https://api.blhp.app/v1/feedback`, https://blhp.app/docs/feedback), from Home's
**Send feedback** row (`metadata.screen` "Home") and the row at the end of Settings ›
Experiments ("Settings › Experiments"). Only the app has it: the keyboard target has no
feedback code or key and still makes no network request.

```ts
Send feedback (row)
├─ draws Hex's key window (never the screen) → the form's first picture, JPEG
└─ the form: Problem · Idea · Question · Other, subject, message, pictures (✕ removes;
   Add opens the system photo picker, no permission, up to 5 in all), an optional email
   (without one the feedback is anonymous), and what is sent in plain words
   └─ Send → Idempotency-Key made now → multipart (payload JSON + screenshot parts), progress bar
      ├─ 201 → Thanks; email "pending" → "you'll get an email if they reply, confirm once"
      ├─ no connection / 5xx → retried (4 tries, 1 s, 2 s, 4 s), same body and key → offline words
      ├─ 409 request_pending, 429 → after Retry-After (≤ 60 s), same key
      ├─ 413, 415, 429 → kind words; 401, 403 feedback_disabled → "unavailable", logged as a fault
      └─ other 4xx → not retried
```

- **Payload** (`FeedbackRequest.payload`): message, subject and category when set, app
  (name, version, build), device (platform, os, model identifier, locale), metadata (screen,
  dictation_language, speech_model: internal ids for the team, never shown). With an email:
  the email and install_id (a random UUID in Hex's `UserDefaults`, `feedback.installID`,
  made at the first such send). Without one: `anonymous: true`, no email, and no install_id
  (none is made). A typed email that isn't an address stops Send until it's fixed or cleared. Never `user` (no accounts)
  or `logs`, and nothing Hex keeps (typing logs, dictations, vocabulary, learned words,
  recordings).
- **Pictures** (`FeedbackImage`): re-encoded as JPEG at 80% with the longest side at most
  2048 px (HEIC included). Limits are checked before sending: message 20,000 characters,
  subject 200, 5 pictures, 10 MiB each, 20 MiB in all.
- **Report key**: `BELLHOP_REPORT_KEY` in `HEXMobile/Feedback.xcconfig`, committed: a
  Bellhop report key is publishable and safe to ship (Android keeps its own in
  `android/gradle.properties`). It reaches the app as Info.plist's `BellhopReportKey`; no Swift
  file holds it. A gitignored `HEXMobile/Feedback.local.xcconfig`, included after it, overrides
  it: written locally, or by `ci_scripts/ci_post_clone.sh` from an Xcode Cloud
  `BELLHOP_REPORT_KEY` secret if one is set. An empty key means no Send feedback row. To rotate
  it, make a new key and revoke the old one in Bellhop › Settings › Report keys, change the
  xcconfig (or set the override), and ship a build: builds with the old key then say feedback
  is unavailable. Each send that works logs `feedback sent as #<number>` (subsystem
  `anomaly.hex.mobile`, category `feedback`).
- **Simulator proof** (debug builds only): `--feedback-log-request` writes each request
  (headers without the key, the payload, each picture's bytes and pixel size) and each answer
  to Documents/feedback-requests; `--feedback-key <key>` and `--feedback-endpoint <url>`
  replace the key or address; `--feedback-drop-first-response` loses the first answer as a
  dropped connection would; `--feedback-broken-image` cuts the last picture in half.
- No unit test covers it: none of `HEXMobileTests`' files is about the app's networking.
