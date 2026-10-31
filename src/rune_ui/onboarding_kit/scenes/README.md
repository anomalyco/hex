# Onboarding scenes

Each `.scene` file is one "Without Hex" disaster and its "With Hex" payoff.
One command per line; `#` starts a comment. Times are seconds, measured from
the previous line, so you rarely have to time anything by hand. A `[…]` at the
end of a line holds options. `scripts/check onboarding` parses every scene and
reports errors with their line numbers.

**Setting** (at the top)
- `title Launch Room`: the thread's name. `rename` can change it later.
- `group Jay, you, and 3 investors`: makes it a group chat, with that subtitle. Leave it out for a one-on-one.
- `boss Jay amber`: the main other person, with an optional colour (`amber apple blue cobalt coral cyan ember lilac mint purple solaris yuzu`). Initials come from the name.
- `side Priya`: up to three side characters, each with an optional colour.
- `pill Jay is presenting`: the red status pill under the header (optional).
- `lead Investors are waiting. One quick reply by hand.`: the line under the title.

**Without Hex.** Lines before `start` are already in the thread when the scene opens. `end` is the last line.
- `Jay: text [0.4]`: someone's message. Without a number, the scene holds for its reading time. `[#desk]` names the line for a later `tap`.
- `cut 0.5 Jay: text`: lands 0.5 s from now, while you go on typing.
- `type text [panic 0.2-0.9]`: you type by hand, typos and all. Panic (0 calm, 1 frantic) sets the pace and how hard the keys hit; without it, panic stays where it was.
- `careful text [pace 0.09]`: slow, even typing, trying very hard.
- `delete 3` removes characters, `wait 0.4` pauses, and `wait freeze` holds the long silence.
- `send` sends the draft. Add `meant <text>` when it went out wrong: each word that differs counts as a typo. `send slip` means a finger hit Return by accident. Type what the fingers hit (`sorry fart fingers`), and put what you meant on the send.
- `autocorrect inconvience -> incontinence`: slow motion as Return goes down, the swap, and the AUTOCORRECT slam. The next `send` sends it.
- `read` puts "Read" under your last message. `dots 0.5` shows the boss typing (`dots` alone is the usual length, `dots 0.4 after 0.2` runs under what follows). `flare` flashes the pill. `elbow` knocks Caps Lock on, or off again.
- `tap ❤ 👍 1.1 ‼`: tapbacks on the newest line (`tap @desk …` for a named one). Each lands 0.2 s after the last unless you give it a time. Glyphs: `❤ 👍 👎 ‼ ? haha`.
- `end removed`, `end left`, `end blocked` or `end undelivered` cuts you off mid-word with that system line ("Jay removed you from “Launch Room”", "Jay left the conversation", "Jay blocked you", "Message not delivered"). `end removed: custom text` changes the wording.

**With Hex** (after `with`): your one spoken sentence, then an out-of-proportion triumph. Aim for about 8 s.
- `hex Yes, we're ready. Everything's tested, go ahead.`: the sentence, spoken in about 1.5 s.
- Then `tap` rains on it, and replies land fast (0.35 s apart unless given a time). Build the escalation in threes.
- `status Jay made you CEO`: a celebratory system pill. `rename Bow Before You` renames the thread with a pill. You can use more than one of each.
- `effect confetti` or `effect fireworks` (both are allowed).
- `score {hex}, no typos, one company.` and `typed Typing: {secs}, {typos}, and one job.`: the score. `{hex}` becomes "1.5 seconds", and `{secs}` and `{typos}` are filled in from the run ("27 seconds", "5 typos").

```
title  Dinner with Sam          # one-on-one: no `group`
boss   Sam coral
lead   It's your first date. One quick reply by hand.
Sam: so are you coming?
start
Sam: I'm at the table [0.4]
type   ys omw [panic 0.3-0.6]
send   slip meant yes omw    # a finger hit Return; 1 typo
read
tap    ? 0.6
end    blocked
with
hex    Yes, on my way. Two minutes out, order the wine.
tap    ❤ ❤ ‼
Sam:   wow. a full sentence
Sam:   my mom says marry you
status Sam pinned your message
effect confetti
score  {hex}, no typos, one wedding.
typed  Typing: {secs}, {typos}, and one date.
```
