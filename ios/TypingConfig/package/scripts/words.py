"""Step 2: every word, with what the fingers hit, what Hex did and what the person left.

Builds on hexlog.py's attempts (runs it if needed). For each word: the keys under the fingers
("raw"), the keys Hex pressed (after its next-letter odds), what Hex put in on space, what the
word was when the session ended ("final", taken as what the person meant: a typo they never
fixed counts as right, so every error count is a floor), and classes (below). Writes
<export>/out/words.json. Standard library only.

    python3 scripts/words.py [EXPORT_DIR] [--half A|B]

Classes (w["cls"]):
  as_typed               finished with space or punctuation, Hex changed nothing
  ac_kept                Hex autocorrected it on space and the person kept the correction
  ac_undone_strip        Hex autocorrected it and the person took it back (strip's revert)
  ac_retyped_literal     Hex autocorrected it and the person typed what they had typed again
  ac_wrong_word          Hex autocorrected it to a word, the person ended with a third word
  ac_of_valid_word       what was typed was already a word, and Hex changed it
  fixed_after_commit     Hex left it, the person fixed it by hand after the space
  inword_fix             deleted and retyped letters before finishing the word
  pick_bar / pick_literal  finished by tapping a suggestion (a word / what they typed)
  swipe                  typed with a swipe
  uncommitted            never got a space (e.g. sent with the app's button): Hex never saw it end
  abandoned              deleted and never retyped in place (intent unknown)
  retyped_as_two         run together (a space read as a letter), deleted and typed again as two words
  revised_by_next_word, merged_into_next, changed_after_commit, case_fixed_by_hand
"""
import json, os, collections, re, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import hexlog

VOCAB = {}


def is_word(w):
    """Whether Hex knows the word: its language model or the dictionary (state/vocabulary.json)."""
    v = VOCAB.get(norm(w))
    return bool(v and (v.get("model") is not None or v.get("dictionary")))


def norm(w):
    return (w or "").lower().replace("\u2019", "'").replace("\u2018", "'").strip()


def letters(w):
    return re.sub(r"[^a-z]", "", norm(w))




def lev(a, b):
    """Damerau-Levenshtein (optimal string alignment)."""
    la, lb = len(a), len(b)
    d = [[0] * (lb + 1) for _ in range(la + 1)]
    for i in range(la + 1): d[i][0] = i
    for j in range(lb + 1): d[0][j] = j
    for i in range(1, la + 1):
        for j in range(1, lb + 1):
            c = 0 if a[i - 1] == b[j - 1] else 1
            d[i][j] = min(d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + c)
            if i > 1 and j > 1 and a[i - 1] == b[j - 2] and a[i - 2] == b[j - 1]:
                d[i][j] = min(d[i][j], d[i - 2][j - 2] + 1)
    return d[la][lb]


def similar(a, b):
    a, b = norm(a), norm(b)
    if not a or not b:
        return False
    return lev(a, b) <= max(1, min(len(a), len(b)) // 2)



def align(a, b):
    """Edit operations turning typed `a` into intended `b` (optimal string alignment).
    Returns a list of (op, i, j): m match, s substitute, x extra typed char (a[i] not in b),
    o omitted (b[j] not typed), t transposition (a[i:i+2] == b[j+1], b[j])."""
    la, lb = len(a), len(b)
    d = [[0] * (lb + 1) for _ in range(la + 1)]
    for i in range(la + 1): d[i][0] = i
    for j in range(lb + 1): d[0][j] = j
    for i in range(1, la + 1):
        for j in range(1, lb + 1):
            c = 0 if a[i - 1] == b[j - 1] else 1
            d[i][j] = min(d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + c)
            if i > 1 and j > 1 and a[i - 1] == b[j - 2] and a[i - 2] == b[j - 1]:
                d[i][j] = min(d[i][j], d[i - 2][j - 2] + 1)
    ops = []
    i, j = la, lb
    while i > 0 or j > 0:
        if i > 0 and j > 0 and d[i][j] == d[i - 1][j - 1] + (0 if a[i - 1] == b[j - 1] else 1):
            ops.append(("m" if a[i - 1] == b[j - 1] else "s", i - 1, j - 1)); i -= 1; j -= 1
        elif i > 1 and j > 1 and a[i - 1] == b[j - 2] and a[i - 2] == b[j - 1] and d[i][j] == d[i - 2][j - 2] + 1:
            ops.append(("t", i - 2, j - 2)); i -= 2; j -= 2
        elif i > 0 and d[i][j] == d[i - 1][j] + 1:
            ops.append(("x", i - 1, j)); i -= 1
        else:
            ops.append(("o", i, j - 1)); j -= 1
    return ops[::-1]


def error_types(typed, intended):
    t = re.sub(r"[^a-z' ]", "", norm(typed).replace("’", "'"))
    w = re.sub(r"[^a-z' ]", "", norm(intended))
    out = []
    for op, i, j in align(t, w):
        if op == "m":
            continue
        if op == "s":
            if t[i] == " ":
                out.append(("space_for_letter", w[j]))
            elif w[j] == " ":
                out.append(("letter_for_space", t[i]))
            elif w[j] == "'":
                out.append(("apostrophe_sub", t[i]))
            else:
                out.append(("sub", t[i] + ">" + w[j]))
        elif op == "x":
            if t[i] == " ":
                out.append(("extra_space", ""))
            elif i > 0 and t[i - 1] == t[i] or (i + 1 < len(t) and t[i + 1] == t[i]):
                out.append(("doubled", t[i]))
            elif t[i] == "'":
                out.append(("extra_apostrophe", ""))
            else:
                out.append(("extra", t[i]))
        elif op == "o":
            if w[j] == " ":
                out.append(("missed_space", ""))
            elif w[j] == "'":
                out.append(("missed_apostrophe", ""))
            elif (j > 0 and w[j - 1] == w[j]) or (j + 1 < len(w) and w[j + 1] == w[j]):
                out.append(("missed_double", w[j]))
            else:
                out.append(("omitted", w[j]))
        elif op == "t":
            out.append(("transposed", t[i:i + 2]))
    return out




def build(export, half=None):
    global VOCAB
    vp = os.path.join(export, "state", "vocabulary.json")
    VOCAB = json.load(open(vp)).get("words", {}) if os.path.exists(vp) else {}
    ap = os.path.join(export, "out", "attempts.json")
    D = json.load(open(ap)) if os.path.exists(ap) else hexlog.build(export)
    LAY = D["layouts"]
    A = {a["id"]: a for a in D["attempts"]}
    kept = [a for a in D["attempts"] if a["ops"] and a["ops"][0][2] in ("key", "swipe")]
    kept_ids = {a["id"] for a in kept}

    # chains: a word deleted whole and typed again where it was
    child = {}
    for a in kept:
        p = a["linked_from"]
        if p and p in kept_ids:
            par = A[p]
            if similar(par["first_pass"] or par["committed"] or "", a["final"]) or similar(par["hex_out"] or "", a["final"]):
                child[p] = a["id"]
            else:
                a["linked_from"] = None
                a["_rewrite_of"] = p
    heads = [a for a in kept if not (a["linked_from"] and a["linked_from"] in kept_ids and child.get(a["linked_from"]) == a["id"])]


    def chain(a):
        out = [a]
        while out[-1]["id"] in child:
            out.append(A[child[out[-1]["id"]]])
        return out


    def key_frames(layout_hash):
        L = LAY.get(layout_hash)
        if not L:
            return {}
        return {k["id"]: k for k in L["keys"]}


    def raw_of(taps):
        """The key under each tap's landing point (before the next-letter odds moved it). A touch
        on the strip's lower half the engine gave to the row-1 key below counts as that key."""
        out = []
        for t in taps:
            if t is None:
                out.append("?")
                continue
            u = t["under0"]
            if u is None:
                u = t["key0"]
            out.append(u if (u and len(u) == 1) else (" " if u == "space" else "#"))
        return "".join(out)


    def complete(tx, taps):
        return tx and taps and len(taps) == len(tx) and all(t is not None for t in taps)


    def error_kept(head, ch):
        """The word typed with its slips kept: the first pass (before any delete), then the letters
        of the word as Ryan finished it (before Hex acted, else as left) past the part the first
        pass covered, with their taps. Returns (letters, taps, distance) or None."""
        t0 = head["first_pass"]
        taps0 = head["taps"]
        if not t0 or head["swiped"] or not complete(t0, taps0):
            return None
        src = None
        for a in [head] + ch[1:]:
            if a["commit"] and complete(a["committed"], a["committed_taps"]):
                src = (a["committed"], a["committed_taps"])
                break
        if src is None:
            last = ch[-1]
            if complete(last["final"], last["final_taps"]):
                src = (last["final"], last["final_taps"])
        if src is None:
            return None
        F, ftaps = src
        if " " in F:
            return None
        nf = norm(F); n0 = norm(t0)
        best = None
        for j in range(0, len(nf) + 1):
            d = lev(n0, nf[:j])
            if best is None or d < best[0] or (d == best[0] and j > best[1]):
                best = (d, j)
        d, j = best
        return (t0 + F[j:], taps0 + ftaps[j:], d)


    words = []
    for h in heads:
        ch = chain(h)
        last = ch[-1]
        final = last["final"]
        commit = h["commit"] or {}
        act = commit.get("action") or {}
        w = {
            "id": h["id"], "build": h["build"], "session": h["session"], "file": h["file"],
            "typed": h["first_pass"], "raw": raw_of(h["taps"]) if not h["swiped"] else None,
            "committed": h["committed"], "hex_out": h["hex_out"], "final": final, "chain": len(ch),
            "swiped": h["swiped"], "commit_src": commit.get("src"), "commit_sep": commit.get("sep"),
            "action": act.get("kind"), "ac_from": act.get("from"), "ac_to": act.get("to"),
            "pick_slot": act.get("slot"), "pick_text": act.get("text"),
            "cands": h["candidates_at_commit"], "ops": h["ops"], "after": h["after_commit_ops"],
            "undo": any(o[0] == "undo" for c in ch for o in c["ops"]),
            "revised": next(([o[1], o[2]] for c in ch for o in c["ops"] if o[0] == "revised"), None),
            "merged_into": next((o[1] for o in h["ops"] if o[0] == "merged_into"), None),
            "later_picks": [p for c in ch for p in c["picks"]][1 if act.get("kind") == "suggestion" else 0:],
            "end_reason": last["end_reason"], "commit_ops": h["commit_ops"], "deleted_all": last["deleted_all"] and not final,
            "ctx": h["ctx"], "traits": h["traits"], "rewrite_of": h.get("_rewrite_of"),
            "taps": h["taps"], "commit_touch": h["commit_touch"], "final_commit_touch": last["commit_touch"] if len(ch) > 1 else (h["next_commit_touch"] or h["commit_touch"]),
            "inword_fix": any(o[0] == "del" for o in h["ops"][: next((i for i, o in enumerate(h["ops"]) if o[0] in ("autocorrect", "pick")), len(h["ops"]))]) and h["commit"] is not None and False,
        }
        # a delete before the first commit (or before the end when never committed)
        first_commit_at = None
        n_ins = 0
        pre = []
        for o in h["ops"]:
            if o[0] in ("autocorrect", "pick"):
                break
            pre.append(o)
        # in-word fix: a delete while the word was being typed and not yet finished
        w["inword_fix"] = (not h["swiped"]) and len(h["first_pass"]) < sum(1 for o in h["ops"] if o[0] == "ins") and any(o[0] == "del" for o in h["ops"]) and not h["touched_after_commit"] or (
            (not h["swiped"]) and h["commit"] is not None and any(o[0] == "del" for o in h["ops"]) and h["committed"] != h["first_pass"])
        ek = error_kept(h, ch)
        w["error_kept"] = ek[0] if ek else None
        w["error_kept_taps"] = ek[1] if ek else None
        w["error_kept_dist"] = ek[2] if ek else None
        words.append(w)

    # ---------------------------------------------------------------- classification
    for w in words:
        F = norm(w["final"]); T = norm(w["typed"]); C = norm(w["committed"]); H = norm(w["hex_out"])
        cls = []
        if not F:
            cls.append("abandoned")  # deleted, never typed again at its place
        if w["action"] == "autocorrect":
            frm, to = norm(w["ac_from"]), norm(w["ac_to"])
            if w["undo"]:
                cls.append("ac_undone_strip")
            elif F and F == frm:
                cls.append("ac_retyped_literal")
            elif F and F == to:
                cls.append("ac_kept")
            elif F and to and F.replace("'", "") == to.replace("'", ""):
                cls.append("ac_kept_apostrophe_changed")
            elif F and (similar(F, frm) or similar(F, to)):
                cls.append("ac_wrong_word")
            elif F:
                cls.append("ac_rewritten")
            if is_word(frm) and frm != to:
                cls.append("ac_of_valid_word")
        elif w["action"] == "suggestion":
            cands = w["cands"] or []
            auto = any(c.get("auto") for c in cands)
            if w["pick_slot"] == 0:
                cls.append("pick_literal_auto" if auto else "pick_literal")
            else:
                cls.append("pick_bar")
        elif w["swiped"]:
            cls.append("swipe")
        elif w["commit_src"] is None:
            cls.append("uncommitted")
        else:
            cls.append("as_typed")
        # what he changed afterwards when Hex did nothing
        if w["action"] is None and not w["swiped"] and F and C and F != C and not w["revised"] and not w["merged_into"]:
            if F.replace("'", "") == C.replace("'", ""):
                cls.append("fixed_after_apostrophe")
            elif similar(F, C):
                cls.append("fixed_after_commit")
            else:
                cls.append("changed_after_commit")
        ap = lambda x: (x or "").replace("\u2019", "'").replace("\u2018", "'")
        if w["final"] and w["hex_out"] and ap(w["final"]) != ap(w["hex_out"]) and norm(w["final"]) == norm(w["hex_out"]):
            cls.append("case_fixed_by_hand")
        if w["revised"]:
            cls.append("revised_by_next_word")
        if w["merged_into"]:
            cls.append("merged_into_next")
        if w["inword_fix"]:
            cls.append("inword_fix")
        w["cls"] = cls


    for w in words:
        F = w["final"]; H = w["hex_out"]; T = w["typed"]
        w["known"] = bool(F) and not w["swiped"]
        # the raw letters the fingers hit, with any slips the hand-fix removed kept
        if w["error_kept"] is not None and w["error_kept_taps"]:
            raw = raw_of(w["error_kept_taps"])
            pressed = "".join((t["key1"] if t and t["key1"] and len(t["key1"]) == 1 else "?") for t in w["error_kept_taps"])
        else:
            raw = w["raw"]; pressed = T
        w["raw_full"] = raw
        w["pressed_full"] = pressed
        if w["known"] and raw:
            w["raw_right"] = norm(raw).replace("'", "") == norm(F).replace("'", "").replace("’", "") or norm(raw) == norm(F)
            w["pressed_right"] = norm(pressed) == norm(F) or norm(pressed).replace("'", "") == norm(F).replace("'", "")
            w["raw_errors"] = error_types(raw, F)
            w["pressed_errors"] = error_types(pressed, F)
        # what Hex left at commit when it got the word (no hand fix before the commit)
        pre = w["ops"]
        w["hex_saw"] = w["commit_src"] is not None and not w["swiped"]
        if w["hex_saw"] and w["known"]:
            # Hex saw the word as Ryan finished it the first time
            w["hex_right"] = norm(H) == norm(F)
            w["hex_right_letters"] = re.sub(r"[^a-z ]", "", norm(H)) == re.sub(r"[^a-z ]", "", norm(F))


    # A run-in deleted and typed again as two words ("gazingvat", then "gazing" and "at"): the
    # first was meant as both, with a space read as a letter. Without this, every run-in the
    # person fixed by retyping would be missing from what Hex got wrong.
    pos = {w["id"]: i for i, w in enumerate(words)}
    for w in words:
        if "abandoned" not in w["cls"] or w["swiped"] or not w.get("typed") or " " in w["typed"]:
            continue
        kid = next((x for x in words if x.get("rewrite_of") == w["id"]), None)
        if kid is None or not kid.get("final") or " " in kid["final"]:
            continue
        nxt = words[pos[kid["id"]] + 1] if pos[kid["id"]] + 1 < len(words) else None
        if nxt is None or nxt["session"] != w["session"] or not nxt.get("final") or " " in nxt["final"]:
            continue
        meant = norm(kid["final"]) + " " + norm(nxt["final"])
        typed = norm(w["typed"])
        ops = align(typed, meant)
        space = next((op for op in ops if op[2] == len(norm(kid["final"])) and op[0] != "x"), None)
        if len(typed) != len(meant) or space is None or space[0] != "s" or lev(typed, meant) > 2:
            continue
        w["final"] = kid["final"] + " " + nxt["final"]
        w["known"] = True
        w["cls"] = [c for c in w["cls"] if c != "abandoned"] + ["retyped_as_two"]
        if w["hex_saw"]:
            w["hex_right"] = norm(w["hex_out"]) == norm(w["final"])
            w["hex_right_letters"] = re.sub(r"[^a-z ]", "", norm(w["hex_out"])) == re.sub(r"[^a-z ]", "", norm(w["final"]))

    sessions = D["sessions"]
    for w in words:
        w["half"] = half_of(w["session"])
        w["in_vocab"] = is_word(w["final"]) if w["final"] else None
        w["settings"] = sessions.get(w["session"], {}).get("settings")
    json.dump({"words": words, "layouts": LAY, "sessions": sessions, "files": D.get("files")}, open(os.path.join(export, "out", "words.json"), "w"))
    return words, LAY, sessions


def half_of(session):
    """The held-out split: every session is A or B by a hash of its id (stable, about half each)."""
    h = 0
    for ch in session:
        h = (h * 131 + ord(ch)) % 1000003
    return "A" if h % 2 == 0 else "B"


def load(export, half=None):
    """Words (built if needed), the layouts by hash and the sessions; `half` keeps one half."""
    global VOCAB
    vp = os.path.join(export, "state", "vocabulary.json")
    VOCAB = json.load(open(vp)).get("words", {}) if os.path.exists(vp) else {}
    p = os.path.join(export, "out", "words.json")
    if os.path.exists(p):
        d = json.load(open(p)); words, LAY, sessions = d["words"], d["layouts"], d["sessions"]
    else:
        words, LAY, sessions = build(export)
    if half:
        words = [w for w in words if w["half"] == half]
    return words, LAY, sessions


def half_arg(argv):
    """--half A, --half=A or "--half A" as one argument; anything else after --half stops the script."""
    for i, a in enumerate(argv):
        parts = a.replace("=", " ").split()
        if parts and parts[0] == "--half":
            h = parts[1] if len(parts) > 1 else (argv[i + 1] if i + 1 < len(argv) else "")
            if h not in ("A", "B"):
                raise SystemExit("--half takes A or B, not %r" % h)
            print("[half %s only]" % h)
            return h
    return None


def scorable(w):
    """A word whose meaning is known and that Hex decided on: typed with taps, finished by space
    or punctuation, letters only, not changed by a bar pick or rewritten afterwards."""
    bad = {"pick_bar", "changed_after_commit", "uncommitted", "revised_by_next_word", "merged_into_next", "swipe", "abandoned"}
    f = norm(w["final"]).replace("'", "")
    return bool(f) and f.isalpha() and bool(w.get("raw_full")) and not (set(w["cls"]) & bad)


if __name__ == "__main__":
    ex = hexlog.export_dir(sys.argv[1:])
    words, LAY, sessions = build(ex)
    half = half_arg(sys.argv)
    if half:
        words = [w for w in words if w["half"] == half]
    by = collections.Counter()
    for w in words:
        for c in w["cls"]:
            by[c] += 1
    print(len(words), "words" + (" (half %s)" % half if half else ""), "->", os.path.join(ex, "out", "words.json"))
    print("vocabulary file:", "yes" if VOCAB else "MISSING (in_vocab and ac_of_valid_word are unknown)")
    for c, n in by.most_common():
        print("  %-24s %5d" % (c, n))
