---
name: hex-typing-config
description: Build a typing config (.mhexconfig) that tunes the Hex keyboard for iPhone to one person, from their Hex typing export. Use when someone gives you a Hex export folder (logs/, state/, scripts/) and asks you to tune their keyboard, make a typing config, or find what Hex gets wrong for them. Covers their words (words to keep, words never to correct to, typo fixes, dictation fixes) and their key positions (where they aim for each key, and where each key's edges should be), with every value tuned on half the logs and checked on the other half.
---

# Build a typing config for this person

This folder is one person's Hex typing export.

- `logs/` is everything they typed and dictated with Hex's keyboard, for up to 14 days: every
  touch, every word, the text around the cursor.
- Hex left out only fields apps mark as passwords, one-time codes and cards. A password typed into
  an ordinary field is in there, so treat the logs like someone's open diary.
- `state/` says which keyboard they use now and what Hex has already learned
  (`reference/state.md`).

You hand back **one file**, `typing-config.mhexconfig`, plus a short `report.md` for the person.

- The config holds two things (`reference/typing-config.md`):
  - **Words:** their own words Hex should keep, words autocorrect must never put in, typo fixes,
    and dictation fixes.
  - **Key positions:** where a tap counts. Hex's keys never move on screen. A config changes how
    taps are read in two ways:
    - **aim** is where this person's finger really lands for each letter;
    - **zones** move the edges of what a tap presses (for example the space bar's top edge, for
      someone who hits it high).
- Hex shows every part with its own switch, checks the config on their phone against their own
  typing, and keeps their earlier configs. A config that changes nothing costs them nothing; one
  that is wrong costs them typing until they notice. Be right before being thorough.

## Rules

1. **The text stays here.** Work on the files where they are. Don't upload, paste or send them
   anywhere, don't use the network, and don't copy `logs/` out of this folder. The scripts need
   only Python 3's standard library.
2. **Quote single words only.** In `report.md` and the config's `note`, never a phrase or
   sentence the person typed. A typo with its fix ("teh → the") and single letters are fine.
3. **No text in anything meant for Hex's makers.** The report's "For the Hex team" section holds
   counts and rates only.
4. **Every value comes from a script's output.** Write the numbers exactly as the script printed
   them. `scripts/check_config.py` re-runs the scripts and rejects any claim that doesn't match.
5. **Tune on one half, check on the other.** `scripts/split.py` splits the sessions into halves A
   and B.
   - Tune aim and zones on A (`--half A`), check them on B, and record that in
     `madeFrom.heldOut`. A value that doesn't hold on B doesn't go in.
   - Words are too sparse for halves: each needs its count in 2 different sessions instead.
6. **Never write a zone you haven't simulated.** Use `scripts/simulate_zones.py` with all of a
   block's zones together. They interact, and the checker compares its numbers.
7. **New, not known.** Hex already learns by itself (below), and `state/phone.json` says what it
   had learned. Don't propose what it already does.
8. **Less is better.** For most people Hex already handles their words and finger habits, and a
   small or empty config is the honest answer. Say so.

## What Hex already does by itself

- **Learn how I type** (on by default):
  - a word kept over an autocorrection twice is never corrected again;
  - a typo erased and retyped as the same word twice becomes a fix (only suggested when the typo
    is itself a word);
  - dictation words fixed the same way twice are fixed from then on;
  - per-key tap averages, by the key **pressed**.
- **Adaptive touch** (an experiment, usually off): personal key centres.
- **The next-letter odds:** a tap within 5 pt of a likelier next letter's key may press that
  letter instead.
- **A config adds what these can't:**
  - words typed often but never corrected (so never kept);
  - fixes made by moving the cursor back, or not yet made twice;
  - capitalised own words that replace ordinary typing;
  - aim measured by the letter **meant** (a better centre than Hex's pressed-key average, and it
    replaces it);
  - zones, which nothing else moves.

## Steps

1. **Look:** `python3 scripts/overview.py` (the phone, Hex's build and settings, how much typing
   there is, what Hex had learned), then `python3 scripts/split.py`.
2. **Words:** `python3 scripts/word_outcomes.py`, then with `--half A` and `--half B`. Look
   for:
   - words Hex doesn't know that they leave in place (`words.keep`);
   - corrections *to* a word they keep taking back (`words.neverCorrectTo`);
   - typos they fix by hand the same way (`words.fixes`; a typo that is a real word is
     `suggest`);
   - and with `scripts/dictation.py`, words dictation mishears (`words.dictation`).
3. **Key positions: measure.**
   - `python3 scripts/taps.py`: every tap with the key it was meant for, mapped onto today's
     keyboard.
   - `python3 scripts/key_clouds.py --tune A`: where the taps meant for each letter land, and the
     aim candidates.
   - `python3 scripts/zone_edges.py --half A`: the edges taps fall just across.
   - `python3 scripts/space_edges.py --half A` and `--half B`: the space bar against typical.
4. **Aim:** check the candidates with `python3 scripts/simulate_aim.py --aim o:0,0.28 … --half A`,
   then `--half B`.
   - Keep a letter only if B gains at least as many taps as it loses, and B's own mean is within
     0.1 keycap.
   - Keep only letters whose aim differs from Hex's by 0.05 keycap or more.
5. **Zones:** judge them by **words**, not taps. A letter tap pressed as its neighbour is often
   repaired by Hex; a space pressed as a letter runs two words together and rarely is.
   `simulate_zones.py` prints both: the taps gained and lost, and the words the change is
   expected to make right and wrong (`reference/typing-config.md`, "How a zone is scored").
   1. `python3 scripts/simulate_zones.py --repair --half A`: how often Hex still got a word
      right with each kind of slip, for this person.
   2. For the top edges from `zone_edges.py`, run `python3 scripts/simulate_zones.py --sweep
      key:edge --half A` and pick the move with the best net **words** (prefer the smaller of
      two close ones). Skip a move with under 15 taps in its strip.
   3. Simulate the chosen zones **together** on A, then on B. Keep a zone only if on B it has at
      least 10 taps in its strip, makes at least 2 more words right than wrong, and makes no more
      than a third as many wrong as right.
   4. Re-simulate the rest together and write the per-zone numbers it prints (`evidence json`).
6. **Write** `typing-config.mhexconfig`, following the worked examples in
   `reference/typing-config.md`:
   - the envelope: `exportId`, `formats.typingConfig` and `activeTypingConfig` from
     `state/phone.json`; `heldOut`;
   - `keys[0].context` copied from `state/phone.json` `layout.context`;
   - only the parts you justified, each with its evidence exactly as printed;
   - a one- or two-sentence `note`.
7. **Check:** `python3 scripts/check_config.py typing-config.mhexconfig --write` until it reports
   0 problems. If it disagrees with you, it is right: fix the file or drop the part.
8. **Report:** write `report.md`:

   ```
   # Your typing config
   ## In short            two or three sentences: what Hex gets wrong for you, what the config changes, what it can't fix
   ## Words               each entry and why (counts; single words only)
   ## Key positions       each aim and zone: what you saw, the tuned and checked numbers, what it changes for you
   ## Left out            what you measured and didn't put in, and why
   ## Couldn't tell       what these logs can't answer
   ## For the Hex team    counts and rates only, no words
   ```

9. **Tell the person:** put `typing-config.mhexconfig` on the iPhone (AirDrop, Files, or open it
   from Messages or Mail) and open it in Hex. Hex shows each part, checks it on their own typing,
   and keeps earlier configs so they can go back.
