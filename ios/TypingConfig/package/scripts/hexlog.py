"""Step 1 of every analysis: rebuild every typed word from the export's logs.

Reads <export>/logs/*.ndjson, follows each keyboard session's edits on a model of the text
before the cursor, and gives every word attempt its taps (touch samples, the key under the
finger, the key pressed), what Hex put in, and what became of it.
Writes <export>/out/attempts.json. Python 3.9+, standard library only, no network.

    python3 scripts/hexlog.py [EXPORT_DIR] [--logs FILE ...]
"""
import json, glob, os, collections

WORDCH = set("'’")


def is_word_char(c):
    return c.isalnum() or c in WORDCH


class Attempt:
    __slots__ = ("id", "file", "session", "build", "start", "ops", "first_pass", "taps", "commit",
                 "committed", "hex_out", "final", "linked_from", "swiped", "touched_after_commit",
                 "deleted_all", "ctx_before", "candidates_at_commit", "commit_touch", "field",
                 "autocap", "after_commit_ops", "first_pass_done", "picks", "chunk", "end_reason",
                 "final_taps", "final_srcs", "next_commit_touch", "t0", "commit_ops", "committed_taps")

    def __init__(self, aid, f, s, build, start, ctx, field):
        self.id = aid; self.file = f; self.session = s; self.build = build; self.start = start
        self.ops = []            # every op on this attempt in order
        self.first_pass = ""     # letters inserted before the first delete or commit
        self.taps = []           # touches for the first pass letters, in order
        self.first_pass_done = False
        self.commit = None       # dict: how it was first finished
        self.committed = None    # the token as typed when it was finished
        self.hex_out = None      # the token right after Hex finished it
        self.final = None
        self.linked_from = None
        self.swiped = False
        self.touched_after_commit = False
        self.deleted_all = False
        self.ctx_before = ctx
        self.candidates_at_commit = None
        self.commit_touch = None
        self.field = field
        self.autocap = None
        self.after_commit_ops = []
        self.picks = []
        self.chunk = None
        self.end_reason = None
        self.final_taps = None
        self.final_srcs = None
        self.next_commit_touch = None
        self.t0 = None
        self.commit_ops = None
        self.committed_taps = None


class Ch:
    __slots__ = ("c", "att", "tap", "src")

    def __init__(self, c, att, tap=None, src=None):
        self.c = c; self.att = att; self.tap = tap; self.src = src


def span_chars(buf, a):
    idx = [i for i, ch in enumerate(buf) if ch.att is a]
    if not idx:
        return []
    return buf[idx[0]:idx[-1] + 1]


def span_of(buf, a):
    idx = [i for i, ch in enumerate(buf) if ch.att is a]
    if not idx:
        return ""
    return "".join(ch.c for ch in buf[idx[0]:idx[-1] + 1])


def touch_summary(t):
    """A touch: its samples (phase, x, y, ts, key, under, layout)."""
    return t


def run(files):
    layouts = {}
    attempts = []
    sessions_meta = {}
    stats = collections.Counter()
    for f in files:
        sessions = collections.OrderedDict()
        for line in open(f, encoding="utf-8"):
            e = json.loads(line)
            if e["type"] == "layout" and "keys" in e:
                layouts[e["hash"]] = {k: e[k] for k in ("hash", "keys", "metrics", "strip", "slots", "width", "height", "letterWidth", "layer", "shift") if k in e}
            sessions.setdefault(e["session"], []).append(e)
        for sid, evs in sessions.items():
            st = evs[0]
            if st["type"] != "session_start":
                stats["session_without_start"] += 1
                continue
            meta = {"build": st["build"], "file": os.path.basename(f), "traits": st["traits"],
                    "settings": st["settings"], "surface": st["surface"], "t": st["t"]}
            sessions_meta[sid] = meta
            attempts += run_session(sid, meta, evs, layouts, stats)
    return attempts, layouts, sessions_meta, stats


def run_session(sid, meta, evs, layouts, stats):
    build = meta["build"]; f = meta["file"]
    buf = []                     # list[Ch]: the text before the cursor as we know it
    out = []
    down = {}                    # touch id -> dict(samples=[...], key, consumed)
    ended = []                   # touches ended, not yet matched to an edit
    last_cand = None
    cand_at_began = {}           # touch id -> candidates showing when it went down
    pending_orphans = []         # attempts fully deleted, waiting for a retype
    known = True                 # whether buf is in sync with the host
    cur_layout = None
    shift_state = "off"
    chunk = 0
    last_commit_att = None
    aid_base = len(out)

    def text():
        return "".join(ch.c for ch in buf)

    def new_attempt():
        nonlocal pending_orphans
        a = Attempt(f"{sid}:{len(out)}", f, sid, build, len(buf), text()[-400:], meta["traits"])
        a.t0 = cur_t
        a.chunk = chunk
        if pending_orphans:
            # A word typed where a deleted one was: its retype.
            o = pending_orphans.pop(0)
            a.linked_from = o.id
        out.append(a)
        return a

    def take_touch(char):
        """The touch that typed `char`: an ended one first, else one still down (rollover)."""
        want = char.lower()
        pool = [t for t in ended if not t["consumed"]] + [t for t in down.values() if not t["consumed"]]
        pool.sort(key=lambda t: t["samples"][0]["ts"])
        for t in pool:
            k = (t["samples"][-1].get("key") or "")
            if k.lower() == want or (want == " " and k == "space"):
                t["consumed"] = True
                return t
        if pool:
            t = pool[0]; t["consumed"] = True; t["mismatch"] = True
            return t
        return None

    def pop_chars(n, how):
        nonlocal pending_orphans
        removed = []
        for _ in range(n):
            if not buf:
                break
            ch = buf.pop()
            removed.append(ch)
            if ch.att is not None:
                a = ch.att
                a.ops.append(("del", ch.c, how))
                if a.commit is not None:
                    a.touched_after_commit = True
                    a.after_commit_ops.append(("del", ch.c, how))
                elif not a.first_pass_done:
                    a.first_pass_done = True
                # fully gone?
                if not any(c.att is a for c in buf):
                    a.deleted_all = True
                    if a not in pending_orphans:
                        pending_orphans.append(a)
                        pending_orphans.sort(key=lambda x: x.start)
        return removed

    def push_chars(s, src, touch=None, kind="ins"):
        for c in s:
            att = None
            if is_word_char(c):
                prev = buf[-1] if buf else None
                if prev is not None and prev.att is not None and is_word_char(prev.c):
                    att = prev.att
                    if att.deleted_all:
                        att.deleted_all = False
                        if att in pending_orphans:
                            pending_orphans.remove(att)
                else:
                    att = new_attempt()
                if att.commit is not None:
                    att.touched_after_commit = True
                    att.after_commit_ops.append(("ins", c, src))
                att.ops.append(("ins", c, src))
                if not att.first_pass_done and att.commit is None:
                    att.first_pass += c
                    att.taps.append(touch)
                if src == "swipe":
                    att.swiped = True
            else:
                # a separator finishes the word before it
                prev = buf[-1] if buf else None
                if prev is not None and prev.att is not None and prev.att.commit is None:
                    finish(prev.att, c, src, touch)
                elif prev is not None and prev.att is not None:
                    prev.att.next_commit_touch = touch
                    prev.att.after_commit_ops.append(("sep", c, src))
            buf.append(Ch(c, att, touch if len(s) == 1 else None, src))

    def finish(a, sep, src, touch, hex_action=None):
        a.commit = {"sep": sep, "src": src, "action": hex_action}
        a.commit_ops = len(a.ops)
        a.committed = token_of(a)
        a.committed_taps = [c.tap for c in span_chars(buf, a)]
        a.hex_out = a.committed
        a.first_pass_done = True
        a.commit_touch = touch
        a.candidates_at_commit = cand_at_began.get(touch["id"]) if touch else last_cand
        a.autocap = shift_state

    def token_of(a):
        return span_of(buf, a)

    cur_t = None
    for e in evs:
        t = e["type"]
        cur_t = e.get("mt")
        if t == "layout":
            cur_layout = e["hash"]; shift_state = e.get("shift", shift_state)
        elif t == "candidates":
            last_cand = e["items"]
        elif t == "touch":
            ph = e["phase"]
            s = {k: e.get(k) for k in ("phase", "x", "y", "ts", "key", "under", "layout", "r")}
            s["has_under"] = "under" in e
            if ph == "began":
                down[e["id"]] = {"id": e["id"], "samples": [s], "consumed": False, "cand": last_cand}
                cand_at_began[e["id"]] = last_cand
            elif e["id"] in down:
                down[e["id"]]["samples"].append(s)
                if ph in ("ended", "cancelled"):
                    tt = down.pop(e["id"])
                    tt["cancelled"] = ph == "cancelled"
                    ended.append(tt)
                    ended = ended[-12:]
        elif t == "edit":
            op = e["op"]; src = e.get("src")
            if op == "insert":
                ins = e.get("inserted", "")
                touch = None
                if src == "key" and len(ins) == 1:
                    touch = take_touch(ins)
                if src == "swipe":
                    stats["swipe_inserts"] += 1
                push_chars(ins, src, touch)
                if src == "swipe":
                    # the swiped word's attempt
                    a = buf[-1].att if buf and buf[-1].att else None
                    if a is not None:
                        a.swiped = True
                        swipe_touch = [tt for tt in ended if not tt["consumed"]]
                        if swipe_touch:
                            swipe_touch[-1]["consumed"] = True
                            a.taps = [swipe_touch[-1]]
                if ins == " " and src == "key" and last_commit_att is not None:
                    pass
            elif op in ("delete", "delete_word"):
                if "deleted" in e:
                    n = len(e["deleted"])
                else:
                    n = e.get("deletedCount", 1)
                if src == "key":
                    take_touch("delete") if False else None
                pop_chars(n, src or op)
            elif op == "autocorrect":
                frm, to = e["from"], e["to"]
                touch = None
                # the space touch
                pool = [tt for tt in ended if not tt["consumed"]] + [tt for tt in down.values() if not tt["consumed"]]
                for tt in pool:
                    if tt["samples"][-1].get("key") == "space" or tt["samples"][0].get("key") == "space":
                        tt["consumed"] = True; touch = tt; break
                a = buf[-1].att if buf and buf[-1].att else None
                cur = text()
                dele = e["deleted"]; insd = e["inserted"]
                if dele != frm and dele.endswith(frm) and insd.endswith(to + " ") and cur.endswith(dele):
                    # the word before revised too ("its coming" -> "it's coming")
                    pdel = dele[: len(dele) - len(frm)]; pins = insd[: len(insd) - len(to) - 1]
                    owner = None
                    for ch in buf[len(buf) - len(dele): len(buf) - len(frm)]:
                        if ch.att is not None:
                            owner = ch.att
                    tail = buf[len(buf) - len(frm):]
                    del buf[len(buf) - len(dele):]
                    for c in pins:
                        buf.append(Ch(c, owner if (owner is not None and is_word_char(c)) else None))
                    buf.extend(tail)
                    if owner is not None:
                        owner.ops.append(("revised", pdel.strip(), pins.strip()))
                        owner.hex_out = span_of(buf, owner)
                    e = dict(e); e["deleted"] = frm; e["inserted"] = to + " "
                    cur = text()
                if a is None or not cur.endswith(frm):
                    stats["autocorrect_unsynced"] += 1
                    pop_chars(len(e["deleted"]), "autocorrect")
                    push_chars(e["inserted"], "autocorrect")
                    continue
                if a.commit is None:
                    finish(a, " ", "autocorrect", touch, hex_action={"kind": "autocorrect", "from": frm, "to": to})
                    a.committed = frm
                else:
                    a.ops.append(("re-autocorrect", frm, to))
                # replace chars, keeping the attempt (a merge may cover two attempts)
                removed = []
                for _ in range(len(e["deleted"])):
                    ch = buf.pop(); removed.append(ch)
                    if ch.att is not None and ch.att is not a:
                        ch.att.ops.append(("merged_into", a.id))
                        ch.att.deleted_all = True
                        ch.att.hex_out = ""
                        ch.att.commit = ch.att.commit or {"sep": "", "src": "merged", "action": {"kind": "merged_into", "into": a.id}}
                for c in e["inserted"]:
                    buf.append(Ch(c, a if is_word_char(c) or (c == " " and False) else None))
                # a split ("on this"): the second word belongs to the same attempt as far as we can tell
                a.hex_out = to
                a.ops.append(("autocorrect", frm, to))
                last_commit_att = a
            elif op == "autocorrect_undo":
                a = None
                for ch in reversed(buf[-(len(e["deleted"]) + 2):]):
                    if ch.att is not None:
                        a = ch.att
                        break
                pop_chars(len(e["deleted"]), "undo")
                # the attempt's chars come back
                for c in e["inserted"]:
                    buf.append(Ch(c, a if a is not None and is_word_char(c) else None))
                if a is not None:
                    a.ops.append(("undo", e["deleted"], e["inserted"]))
                    a.touched_after_commit = True
                    a.after_commit_ops.append(("undo", e["deleted"], e["inserted"]))
                    a.deleted_all = False
                    if a in pending_orphans:
                        pending_orphans.remove(a)
            elif op == "suggestion":
                a = buf[-1].att if buf and buf[-1].att else None
                dele = e.get("deleted", "")
                touch = None
                if a is not None and a.commit is None:
                    finish(a, " ", "suggestion", None, hex_action={"kind": "suggestion", "slot": e["slot"], "text": e["text"], "candidates": e.get("candidates")})
                    a.candidates_at_commit = e.get("candidates")
                    a.committed = dele
                elif a is not None:
                    a.ops.append(("suggestion_after", e["slot"], e["text"]))
                    a.touched_after_commit = True
                    a.after_commit_ops.append(("suggestion", e["slot"], e["text"]))
                for _ in range(len(dele)):
                    if buf:
                        buf.pop()
                ins = e.get("inserted", "")
                for c in ins:
                    buf.append(Ch(c, a if a is not None and is_word_char(c) else None))
                if a is not None:
                    a.picks.append({"slot": e["slot"], "text": e["text"], "deleted": dele, "inserted": ins,
                                    "candidates": e.get("candidates")})
                    if a.hex_out is not None and a.commit and a.commit["src"] == "suggestion" and len(a.picks) == 1:
                        a.hex_out = ins.strip()
                    a.ops.append(("pick", e["slot"], e["text"]))
            elif op == "double_space_period":
                if buf:
                    buf.pop()
                for c in ". ":
                    buf.append(Ch(c, None))
            elif op == "return":
                if "inserted" in e:
                    push_chars("\n", "return")
            elif op == "paste":
                push_chars(e.get("inserted", ""), "paste")
            elif op == "undo_dictation":
                if "deleted" in e:
                    pop_chars(len(e["deleted"]), "undo_dictation")
                else:
                    pop_chars(e.get("deletedCount", 0), "undo_dictation")
            elif op == "cursor":
                known = False
                chunk += 1
                stats["cursor_moves"] += 1
            elif op == "space_swallowed":
                pass
            else:
                stats["op_" + op] += 1
        elif t == "dictation":
            ins = e.get("inserted") or ""
            # dictated text: not attempts
            for c in ins:
                buf.append(Ch(c, None))
            chunk += 1
        elif t == "context":
            before = e.get("before")
            if before is None:
                continue
            cur = text()
            if cur.endswith(before):
                continue
            norm = lambda x: x.replace("\u2019", "'").replace("\u2018", "'")
            if norm(cur).endswith(norm(before)) and len(before) > 0:
                # smart quotes: take the host's characters
                for i, c in enumerate(before):
                    buf[len(buf) - len(before) + i].c = c
                continue
            if before.endswith(cur) and len(cur) < len(before):
                # the host knows more text than we do (it was there already)
                pre = before[: len(before) - len(cur)]
                buf[:0] = [Ch(c, None) for c in pre]
                for a in out:
                    a.start += len(pre)
                continue
            # out of sync: rebuild from the host's text, attempts lose their place
            stats["resync_" + e["why"]] += 1
            if os.environ.get("DEBUG_RESYNC") and e["why"] in ("space", "suggestion", "return"):
                print("RESYNC", e["why"], build, sid, e["seq"], repr(cur[-25:]), "|", repr(before[-25:]))
            seen = set()
            for ch in buf:
                if ch.att is not None and id(ch.att) not in seen:
                    seen.add(id(ch.att))
                    ch.att.final = span_of(buf, ch.att)
                    sc = span_chars(buf, ch.att)
                    ch.att.final_taps = [c.tap for c in sc]; ch.att.final_srcs = [c.src for c in sc]
                    ch.att.end_reason = "resync_" + e["why"]
            buf = [Ch(c, None) for c in before]
            pending_orphans = []
            chunk += 1
        elif t == "session_end":
            pass
    # finals
    seen = set()
    for ch in buf:
        if ch.att is not None and id(ch.att) not in seen:
            seen.add(id(ch.att))
            if ch.att.final is None:
                ch.att.final = span_of(buf, ch.att)
                sc = span_chars(buf, ch.att)
                ch.att.final_taps = [c.tap for c in sc]; ch.att.final_srcs = [c.src for c in sc]
                ch.att.end_reason = "session_end"
    for a in out:
        if a.final is None:
            a.final = ""
            a.end_reason = a.end_reason or "deleted"
    return out


def tap_record(t, layouts):
    if t is None:
        return None
    s0 = t["samples"][0]; s1 = t["samples"][-1]
    return {"id": t["id"], "samples": [[s["phase"][0], s["x"], s["y"], s["ts"], s.get("key"), s.get("under") if s.get("has_under") else "=", s.get("r")] for s in t["samples"]],
            "layout": s0.get("layout"), "key0": s0.get("key"), "key1": s1.get("key"),
            "under0": s0.get("under") if s0.get("has_under") else s0.get("key"), "cancelled": t.get("cancelled", False),
            "mismatch": t.get("mismatch", False)}



def files_in(export, logs=None):
    if logs:
        return [os.path.abspath(f) for f in logs]
    return sorted(glob.glob(os.path.join(export, "logs", "*.ndjson")))


def export_dir(argv):
    """The export folder: the first argument that is a directory, else the folder above scripts/."""
    for a in argv:
        if os.path.isdir(a):
            return os.path.abspath(a)
    return os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def build(export, logs=None):
    files = files_in(export, logs)
    if not files:
        raise SystemExit("no logs: put the .ndjson files in %s/logs/" % export)
    attempts, layouts, sessions_meta, stats = run(files)
    os.makedirs(os.path.join(export, "out"), exist_ok=True)
    rows = []
    for a in attempts:
        rows.append({
            "id": a.id, "file": a.file, "session": a.session, "build": a.build, "chunk": a.chunk,
            "ctx": a.ctx_before, "first_pass": a.first_pass, "committed": a.committed, "hex_out": a.hex_out,
            "final": a.final, "commit": a.commit, "swiped": a.swiped, "linked_from": a.linked_from,
            "touched_after_commit": a.touched_after_commit, "deleted_all": a.deleted_all,
            "ops": a.ops, "after_commit_ops": a.after_commit_ops, "picks": a.picks,
            "candidates_at_commit": a.candidates_at_commit,
            "taps": [tap_record(t, layouts) for t in a.taps],
            "commit_touch": tap_record(a.commit_touch, layouts) if a.commit_touch else None,
            "shift_at_commit": a.autocap, "end_reason": a.end_reason, "t0": a.t0, "commit_ops": a.commit_ops,
            "final_taps": [tap_record(t, layouts) for t in (a.final_taps or [])],
            "final_srcs": a.final_srcs or [],
            "committed_taps": [tap_record(t, layouts) for t in (a.committed_taps or [])],
            "next_commit_touch": tap_record(a.next_commit_touch, layouts) if a.next_commit_touch else None,
            "traits": {k: a.field.get(k) for k in ("autocapitalization", "autocorrection", "keyboardType", "returnKeyType")},
        })
    out = {"files": [os.path.basename(f) for f in files], "attempts": rows, "layouts": layouts,
           "sessions": sessions_meta, "stats": stats}
    json.dump(out, open(os.path.join(export, "out", "attempts.json"), "w"))
    return out


if __name__ == "__main__":
    import sys
    args = sys.argv[1:]
    logs = args[args.index("--logs") + 1:] if "--logs" in args else None
    ex = export_dir([a for a in args if a != "--logs" and not (logs and a in logs)])
    d = build(ex, logs)
    print(len(d["attempts"]), "word attempts from", ", ".join(d["files"]), "->", os.path.join(ex, "out", "attempts.json"))
    print("parser notes:", dict(d["stats"]))
