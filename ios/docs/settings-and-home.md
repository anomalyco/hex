# Settings and Home

Part of [Hex for iPhone](../README.md).

Home's single screen and the four Settings pages, the experiments list,
and every `--snap-state` scene the app and the keyboard render.

## Settings (`ContentView`, `HEXMobile/Settings/`)

Home fits one screen: the
Hex header, one status line ("Ready" with a check, or "Finish setup · 1 step ›"),
Dictate, and four rows that each open a short page, then **iOS Settings ↗**
(Hex's page in iOS Settings) and **Run setup again**, and last **Privacy
policy**, **Support** and **Acknowledgements**. Privacy policy (App Review
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
  the switch); and last Speech model in keyboard (its status below it), Keep mic warm and
  Microphone.
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
  default until you choose, Medium or Strong) and Emoji (Clear emoji history while it's on,
  which asks first); and a field to try the keyboard in.
- **Privacy:** "Everything stays on this iPhone", Learn how I type (what it
  learned below the switch), and under **Advanced**, Record typing logs (off
  by default) with its Typing logs viewer while it's on.
- **Experiments**, in four groups: **Dictation**: Multilingual model (Parakeet),
  Two-language dictation, Voice isolation and Clean up with Apple Intelligence;
  **Languages**: Hinglish (Hindi in Latin letters), Greeklish (Greek in Latin letters),
  Korean keyboard (두벌식), Swipe typing in more languages and German spatial autocorrect
  (greyed, its value kept, unless Autocorrect and Spatial autocorrect are on); **Keyboard
  layout**: Comma and period keys, Dvorak punctuation keys, Quick accents, ABC under the
  emoji key, Keyboard themes (its Theme menu below the switch while it's on) and Join the
  keyboard when unfolded (only on a foldable iPhone, `FullKeyboardView.isFoldable`);
  **Typing**: Adaptive touch (its status and Reset touch learning below the switch),
  Two-thumb selection, Delete swiped words letter by letter, Hide word suggestions, Emoji
  shortcodes and Learn each language separately. 21 switches, 20 on an iPhone that doesn't fold.

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
