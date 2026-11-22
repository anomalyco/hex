# Settings and Home

Part of [Hex for iPhone](../README.md).

Home's single screen and the four Settings pages, the experiments list,
and every `--snap-state` scene the app and the keyboard render.

## Settings (`ContentView`, `SettingsPages.swift`)

Home fits one screen: the
Hex header, one status line ("Ready" with a check, or "Finish setup · 1 step ›"),
Dictate, and four rows that each open a short page, then **Keyboard settings**
(Hex's page in iOS Settings) and **Run setup again**, and last **Privacy
policy**, **Support** and **Acknowledgements**. Privacy policy (App Review
5.1.1(i)) and Support open in Safari inside Hex (`HexLinks`, one base URL:
`https://dev.anoma.ly/hex` until the production site serves
`anoma.ly/hex/privacy`). Each row has a one-line description at most. The pages:

- **Dictation:** Style, Lowercase, Format lists, Spoken punctuation, End with a
  period, Dictation language, Show what I say and Type as I speak (iOS 26 and later),
  Translate what I say, Clean up what I say,
  Custom vocabulary (its list below the switch), Keep mic warm,
  and Microphone.
- **Keyboard:** Keyboard, **Full** (the default) or **Dictation bar** (stored
  as `FullKeyboardSettings.isEnabled`, App Group key `experiments.fullKeyboard`
  from when the full keyboard was an experiment: unset or true is Full, false
  the dictation bar, so testers who turned that experiment off keep the bar;
  an iPad follows the same choice, where until build 117 it always showed the bar), Auto-start (Never, On
  switch by default, or Always) while the dictation bar is chosen, then Languages, Number row (off by default), Auto-capitalization, Autocorrect (with Spatial autocorrect below it while it's
  on), Suggest swear words, Vocabulary (Hex's custom vocabulary list, how many words it
  holds) and Key pop-ups only while Full is chosen, Haptics (the dictation bar uses it too),
  then Swipe typing and Emoji (Clear emoji history while it's on), which say they need the
  full keyboard while the dictation bar is chosen, and a field to try the keyboard in.
- **Privacy:** "Everything stays on this iPhone", Learn how I type (what it
  learned below the switch), and under **Advanced**, Record typing logs (off
  by default) with its Typing logs viewer while it's on.
- **Experiments:** Hinglish (Hindi in Latin letters), Greeklish (Greek in Latin
  letters), Two-language dictation,
  Clean up with Apple Intelligence, Adaptive touch (its status and Reset touch learning
  below the switch), German spatial autocorrect, Swipe typing in more languages, Korean
  keyboard (두벌식), Dvorak punctuation keys, Comma and period keys, Quick accents, Emoji
  shortcodes, ABC under the emoji key, Keyboard themes (its Theme menu below the switch
  while it's on), Two-thumb selection, Delete swiped words letter by letter, Hide word
  suggestions, Join the keyboard when unfolded, and Multilingual model (Parakeet).

Simulator scenes: `--snap-state settings-home` (ready), `setup` (keyboard
added, Full Access not confirmed yet), `setup-sheet`, `setup-sheet-microphone`
(before iOS has asked for the microphone), and
`settings-<dictation|keyboard|privacy|experiments>[-on][-end]`, and
`model-card-<waiting|downloading|ready>` (Home's speech model card, with English
dictation and no keyboard limit, so nothing downloads). Each settings scene writes
its page's switches to the App Group: off by default, on with `-on`; switches that are
on by default (Spatial autocorrect, Swipe typing, Emoji) stay on.
`-end` starts the page scrolled to the bottom. `settings-privacy-confirm`
opens Record typing logs' consent dialog. `--experiment-keyboard-themes` turns
Keyboard themes on, and `--keyboard-theme automatic|black|white|navy|forest|ember`
picks the theme the full keyboard draws;
the dictation bar keeps the system look, and iOS draws its own strip above or below
a third-party keyboard, which a theme can't color.
