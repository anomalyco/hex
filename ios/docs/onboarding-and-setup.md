# Onboarding and setup

Part of [Hex for iPhone](../README.md).

Hex's first run, the setup sheet, Full Access, the ways the keyboard
opens Hex, and the cold-start handoff. The keyboard itself is in
[keyboard.md](keyboard.md); dictation in [dictation.md](dictation.md).

## First run (`HEXMobile/Onboarding.swift`)

The desktop's pages and
order. **Setting up Hex** starts the speech model downloading (the engine
`transcriberFactory` picks; a launch that found Parakeet ended mid-load
stays failed until Try again) and moves on after a beat. **With Hex**, a
short demo, moves on by itself. **Let Hex hear you** has two rows, the
microphone (its pill and the page's Continue say Continue before iOS's
prompt, never Allow, per App Review 5.1.1(iv)) and the Hex keyboard with
Full Access (Open Settings, Hex's page); a done row shows only its check,
and the page moves on once both are done. Continue never needs the
microphone. **Almost ready** shows only while the model downloads, with
Continue without a model after a failure or 30 s without progress.
**Your turn** keeps the keyboard up and counts once a dictation arrives or
three words are typed with Full Access; an empty dictation says Hex heard
nothing (the keyboard notes it in the App Group). The keyboard also notes
when it comes on screen and leaves (`KeyboardPresence`): when Hex's
keyboard isn't up 2 s after a keyboard switch, the page says to tap the
globe until it is, and only words that arrive while it's up count as
typed. iOS's dictation and other keyboards take it off screen, so their
text never finishes the practice; the page says to tap the Hex key
instead. **Hex is ready** shows
only once every step is done, else Almost ready with what's left. The
model step counts the keyboard's own model too
(`KeyboardSpeechPreparation`): on iOS 26 the keyboard dictates with
Apple Speech even when Hex uses Parakeet, so Hex installs that language's
model as the keyboard would; where Apple has none on the iPhone, it marks
`keyboardNeedsHex` (with `keyboardLanguageUnavailable`) so the Hex key says "<Language> isn't available on this iPhone yet" from the first tap, with only a ✕. Home's
Ready counts it the same way. The
model's progress pill sits in the top bar until it's ready; Skip shows on
the demo and the practice only. It shows once: `onboarding.completed` in
the app's UserDefaults records Start using Hex or Continue without a
model, `onboarding.page` resumes a quit part-way on the same page, and a
launch that finds the keyboard ready and the microphone allowed (an
existing user) records completion without showing it. **Run setup again**
replays it from page 1. Its `--snap-state` scenes are `onboarding-setup`,
`-demo`, `-permissions`, `-permissions-done`, `-waiting`, `-failed`,
`-try`, `-try-done`, `-try-empty`, `-try-off`, `-ready`, and
`onboarding-tour`, which walks every page by itself for a recording.

## Without Full Access (App Review 4.4.1)

Hex is a keyboard first. The
full keyboard types without Full Access or network: letters, shift and caps
lock, numbers and symbols, accents, delete, return, the globe key, and
suggestions and autocorrect (`UITextChecker`, the supplementary lexicon,
and, where a typing model is bundled for the language, Spatial autocorrect's
model: English, Spanish and French, and German with its experiment on). Words
it learns may last only while the keyboard runs, as an iPhone may not let
it write the App Group; haptics, key clicks (Apple's keyboard guide), Paste and
dictation need Full Access. Without Full
Access the keyboard always mounts the full keyboard, whichever keyboard is
chosen (the dictation bar types only spaces and line breaks). Dictation needs
Full Access (Hex's App Group and the microphone): tapping the Hex key, or
sliding for its menu, shows for 8 s in the suggestion strip "Allow Full
Access to dictate" and "Settings › Apps › Hex › Keyboards", with an **Open
Hex** chip (`hex-dictation://keyboard/setup`) that opens the app's setup
sheet. Nothing else opens Hex without Full Access, and the strip shows no
notice until the Hex key is tapped.

## Setup

Home's status line counts the setup steps left ("Finish
setup · 1 step ›", else "Ready") and opens the same sheet: live status for
the keyboard, Full Access, the microphone ("For dictation" with
**Continue**, which iOS's prompt follows, until it's asked; never a button
that says Allow, per App Review 5.1.1(iv)), and the speech model, a field to
switch to Hex in while Full Access is unconfirmed, and **Open Settings**
(Hex's page in Settings) while the keyboard isn't ready. iOS reports Full
Access only inside the keyboard, so the keyboard reports it to the App Group
as it loads, each time it appears and every 5 s while shown
(`keyboard.fullAccessOn`/`Off`, with when its process started, and
`keyboard.fullAccessSeenAt` for an on), and posts a Darwin notification the
app refreshes on. Full Access shows as on once a keyboard has run with it,
and as off only when a keyboard said so since Hex last came to the
foreground (it changes only in Settings), from a keyboard process no older
than the one that last saw it on. Otherwise the sheet says "Not confirmed
yet" with "Switch to Hex below to confirm." under it, and focuses the field.

## Opening Hex

SwiftUI `Link`, `openURL`, and `extensionContext.open`
do nothing in a keyboard extension, so the keyboard opens Hex through
the `UIApplication` at the end of its responder chain
(`ContainingAppOpener`).

## Cold start (before iOS 26)

Tapping **Dictate** opens the containing app
(`hex-dictation://keyboard/record?...`) with a 10-second
handoff window and starts recording. Hex never opens another app: the
handoff screen shows a quiet "Swipe bar to return" cue over the home
indicator, and you swipe back to the app you were typing in. The text goes in
once the keyboard is on screen there again: iOS often starts a new keyboard on
the way back, and one that inserted while it loaded, before its field was
connected, lost the text (iPad mini, iPadOS 18, TestFlight build 115).
