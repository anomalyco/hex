"""Words: what Hex did with them and what the person did next. The per-person findings live
here: words Hex doesn't know that the person keeps typing, corrections they undo, typos they
fix the same way by hand, and slips that are themselves real words. Prints single words only.

    python3 scripts/word_outcomes.py [EXPORT_DIR] [--half A|B]
"""
import json, os, sys, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import hexlog, words as W

ex = hexlog.export_dir(sys.argv[1:])
half = W.half_arg(sys.argv)
words, LAY, _ = W.load(ex, half)
n = W.norm
sc = [w for w in words if W.scorable(w)]
raw_wrong = [w for w in sc if not w.get("raw_right")]
hex_wrong = [w for w in sc if w.get("hex_saw") and not w.get("hex_right")]
changed = [w for w in sc if w.get("hex_saw") and w.get("raw_right") and not w.get("hex_right")]
print("scorable words: %d%s" % (len(sc), " (half %s)" % half if half else ""))
print("  at least one tap on the wrong key: %d (%.1f%%)" % (len(raw_wrong), 100 * len(raw_wrong) / max(1, len(sc))))
print("  wrong after Hex (as the phone did it): %d (%.1f%%)" % (len(hex_wrong), 100 * len(hex_wrong) / max(1, len(sc))))
print("  typed right, changed by Hex: %d" % len(changed))
odds_fix = sum(1 for w in sc if w.get("raw_right") is False and w.get("pressed_right") is True)
odds_hurt = sum(1 for w in sc if w.get("raw_right") is True and w.get("pressed_right") is False)
print("  next-letter odds (Hex moving a tap to a likelier neighbour): fixed %d words, broke %d" % (odds_fix, odds_hurt))
cls = collections.Counter(c for w in words for c in w["cls"])
ac = cls["ac_kept"] + cls["ac_undone_strip"] + cls["ac_retyped_literal"] + cls["ac_wrong_word"]
print("autocorrections: %d; kept %d, undone %d, retyped as typed %d, ended as a third word %d" % (
    ac, cls["ac_kept"], cls["ac_undone_strip"], cls["ac_retyped_literal"], cls["ac_wrong_word"]))


sessions = collections.defaultdict(set)


def show(title, counter, extra=None):
    print("\n" + title)
    if not counter:
        print("  (none)")
    for k, c in counter.most_common(40):
        print("  %-28s %d time(s) in %d session(s)%s" % (k, c, len(sessions[(title, k)]), extra(k) if extra else ""))


UNKNOWN = "words Hex doesn't know (state/vocabulary.json) that the person left in place (of which Hex corrected first):"
unknown = collections.Counter(); unk_changed = collections.Counter()
for w in words:
    f = n(w["final"])
    if f and f.replace("'", "").isalpha() and w.get("in_vocab") is False and not w["swiped"]:
        unknown[f] += 1
        sessions[(UNKNOWN, f)].add(w["session"])
        if w.get("action") == "autocorrect":
            unk_changed[f] += 1
show(UNKNOWN, unknown,
     lambda k: " (Hex corrected %d)" % unk_changed[k] if unk_changed[k] else "")
UNDONE = "corrections the person took back (typed -> Hex's word):"
undone = collections.Counter()
for w in words:
    if set(w["cls"]) & {"ac_undone_strip", "ac_retyped_literal"}:
        k = n(w["ac_from"]) + " -> " + w["ac_to"]
        undone[k] += 1; sessions[(UNDONE, k)].add(w["session"])
show(UNDONE, undone)
TO = "corrections taken back or retyped, by the word Hex put in (words.neverCorrectTo; exact spelling):"
to = collections.Counter()
for w in words:
    if set(w["cls"]) & {"ac_undone_strip", "ac_retyped_literal", "ac_wrong_word"} and w.get("ac_to"):
        to[w["ac_to"]] += 1; sessions[(TO, w["ac_to"])].add(w["session"])
show(TO, to)
wrongw = collections.Counter(n(w["ac_from"]) + " -> " + n(w["ac_to"]) + " (meant " + n(w["final"]) + ")" for w in words if "ac_wrong_word" in w["cls"])
show("corrected to the wrong word:", wrongw)
FIXES = "typos fixed by hand (after the space, or mid-word), typo -> word:"
REAL = "...of which the typo is itself a real word (autocorrect leaves those alone; a config fix must be suggest):"
fixes = collections.Counter(); realword = collections.Counter()
for w in words:
    if "fixed_after_commit" in w["cls"] or ("inword_fix" in w["cls"] and w.get("raw_full")):
        typo = n(w["committed"] if "fixed_after_commit" in w["cls"] else w["raw_full"])
        f = n(w["final"])
        if typo and f and typo != f:
            fixes[typo + " -> " + f] += 1; sessions[(FIXES, typo + " -> " + f)].add(w["session"])
            v = W.VOCAB.get(typo)
            if v and (v.get("model") is not None or v.get("dictionary")):
                realword[typo + " -> " + f] += 1; sessions[(REAL, typo + " -> " + f)].add(w["session"])
show(FIXES, fixes)
show(REAL, realword)
picks = collections.Counter(n(w["pick_text"]) for w in words if "pick_bar" in w["cls"] and w.get("pick_text"))
print("\nwords picked from the suggestion bar:", dict(picks.most_common(20)) or "(none)")
