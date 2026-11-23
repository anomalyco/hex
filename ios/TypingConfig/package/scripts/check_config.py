"""Check a typing config before handing it over: the format (reference/typing-config.md), the
phone it is for, and every claim in it against the logs. Words: their counts are recomputed.
Aim and zones: simulate_aim.py and simulate_zones.py are re-run on the halves the config says
it was tuned and checked on, and every tally must match what the file claims, exactly (a zone's
taps and its expected words made right and wrong). A zone you didn't simulate can't pass, and
one must make more words right than wrong on the check half (simulate_zones.holds). Exits 1 on
any problem; --write fills in "id".

    python3 scripts/check_config.py typing-config.mhexconfig [EXPORT_DIR] [--write]
"""
import hashlib, json, os, sys, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import hexlog, words as W, geometry as G, simulate_aim as SA, simulate_zones as SZ
from limits import LIMITS

args = [a for a in sys.argv[1:] if a != "--write"]
path = args[0]
ex = hexlog.export_dir(args[1:])
raw = open(path, "rb").read()
problems, notes = [], []
L = LIMITS
if len(raw) > L["file"]["maxBytes"]:
    problems.append("the file is %d bytes; the limit is %d" % (len(raw), L["file"]["maxBytes"]))
cfg = json.loads(raw)
phone = json.load(open(os.path.join(ex, "state", "phone.json")))
layout = G.load_layout(ex)
want = (phone.get("formats") or {}).get("typingConfig", L["version"])
TOP = {"format", "version", "id", "basedOn", "madeFrom", "madeFor", "note", "words", "keys"}
for k in cfg:
    if k not in TOP:
        problems.append("unknown key %r (Hex skips it)" % k)
if cfg.get("format") != L["format"]:
    problems.append('"format" must be %r' % L["format"])
if cfg.get("version") != want:
    problems.append('"version" must be %s (state/phone.json formats.typingConfig)' % want)
if len(cfg.get("note") or "") > L["file"]["noteMaxCharacters"]:
    problems.append("the note is longer than %d characters" % L["file"]["noteMaxCharacters"])
mf = cfg.get("madeFrom") or {}
if mf.get("exportId") != phone.get("exportId"):
    problems.append("madeFrom.exportId should be %r" % phone.get("exportId"))
held = mf.get("heldOut") or {}
tuned, checked = held.get("tunedOn"), held.get("checkedOn")
if tuned not in ("A", "B") or checked not in ("A", "B") or tuned == checked:
    problems.append('madeFrom.heldOut needs "tunedOn" and "checkedOn": "A" and "B", one each')
    tuned, checked = "A", "B"
active = phone.get("activeTypingConfig")
if active and cfg.get("basedOn") != (active.get("id") if isinstance(active, dict) else active):
    problems.append("basedOn should be the active config's id")

# ---- words, recounted from the logs
words, LAY, _ = W.load(ex)
n = W.norm
left = collections.Counter(); left_sessions = collections.defaultdict(set)
for w in words:
    if w.get("final") and not w["swiped"]:
        left[n(w["final"])] += 1; left_sessions[n(w["final"])].add(w["session"])
taken_back_to = collections.Counter(n(w["ac_to"]) for w in words if set(w["cls"]) & {"ac_undone_strip", "ac_retyped_literal", "ac_wrong_word"} and w.get("ac_to"))
taken_back_to_exact = collections.Counter(w["ac_to"] for w in words if set(w["cls"]) & {"ac_undone_strip", "ac_retyped_literal", "ac_wrong_word"} and w.get("ac_to"))
hand = collections.Counter(); hand_sessions = collections.defaultdict(set)
for w in words:
    if "fixed_after_commit" in w["cls"] or ("inword_fix" in w["cls"] and w.get("raw_full")):
        typo = n(w["committed"] if "fixed_after_commit" in w["cls"] else w["raw_full"])
        if typo and typo != n(w["final"]):
            hand[(typo, n(w["final"]))] += 1; hand_sessions[(typo, n(w["final"]))].add(w["session"])
learned = set(x.lower() for x in ((phone.get("learnedWords") or {}).get("known") or []))
V = W.VOCAB
WL = L["words"]


def in_model(word):
    v = V.get(n(word)); return bool(v and v.get("model") is not None)


def is_word(word):
    v = V.get(n(word)); return bool(v and (v.get("model") is not None or v.get("dictionary")))


def claim(e, key):
    return (e.get("evidence") or {}).get(key, 0)


ws = cfg.get("words") or {}
for e in ws.get("keep") or []:
    w = e.get("word", ""); k = n(w)
    if left[k] < WL["keep"]["minTimes"] or len(left_sessions[k]) < WL["keep"]["minSessions"]:
        problems.append("words.keep %r: left in place %d time(s) in %d session(s); needs %d in %d" % (w, left[k], len(left_sessions[k]), WL["keep"]["minTimes"], WL["keep"]["minSessions"]))
    if claim(e, "times") > left[k] or claim(e, "sessions") > len(left_sessions[k]):
        problems.append("words.keep %r: claims %s, the logs have %d times in %d sessions" % (w, e.get("evidence"), left[k], len(left_sessions[k])))
    if in_model(w):
        problems.append("words.keep %r: Hex's model already has it" % w)
    if k in learned:
        notes.append("words.keep %r: Hex had already learned it" % w)
for e in ws.get("neverCorrectTo") or []:
    w = e.get("word", "")
    got = taken_back_to_exact[w] if w != w.lower() else taken_back_to[n(w)]
    if got < WL["neverCorrectTo"]["minTakenBack"]:
        problems.append("words.neverCorrectTo %r: corrections to it taken back %d time(s); needs %d" % (w, got, WL["neverCorrectTo"]["minTakenBack"]))
    if claim(e, "takenBack") > got:
        problems.append("words.neverCorrectTo %r: claims %d taken back, the logs have %d" % (w, claim(e, "takenBack"), got))
for e in ws.get("fixes") or []:
    t, w, m = n(e.get("typo")), n(e.get("word")), e.get("mode")
    if m not in ("apply", "suggest"):
        problems.append("words.fixes %r: mode must be apply or suggest" % t)
    if hand[(t, w)] < WL["fixes"]["minTimes"] or len(hand_sessions[(t, w)]) < WL["fixes"]["minSessions"]:
        problems.append("words.fixes %r -> %r: fixed by hand %d time(s) in %d session(s); needs %d in %d" % (t, w, hand[(t, w)], len(hand_sessions[(t, w)]), WL["fixes"]["minTimes"], WL["fixes"]["minSessions"]))
    if hand[(w, t)]:
        problems.append("words.fixes %r -> %r: also fixed the other way round" % (t, w))
    if is_word(t) and m == "apply":
        problems.append("words.fixes %r is itself a word: mode must be suggest" % t)
for e in ws.get("dictation") or []:
    if claim(e, "times") < WL["dictation"]["minTimes"]:
        problems.append("words.dictation %r: needs %d fixes" % (e.get("heard"), WL["dictation"]["minTimes"]))

# ---- key positions, re-simulated
ctx = layout["context"]
A, Z = L["keys"]["aim"], L["keys"]["zones"]
for b, block in enumerate(cfg.get("keys") or []):
    if block.get("context") != ctx:
        diff = [k for k in ctx if (block.get("context") or {}).get(k) != ctx[k]]
        problems.append("keys[%d] is for another keyboard (%s differ from state/phone.json layout.context); Hex won't apply it here" % (b, ", ".join(diff)))
        continue
    aim = {k: v.get("offset") for k, v in (block.get("aim") or {}).items()}
    for k, v in aim.items():
        if not (isinstance(v, list) and len(v) == 2 and all(isinstance(x, (int, float)) and abs(x) <= A["bound"] for x in v)):
            problems.append("keys[%d].aim.%s: offset must be two numbers within +-%g" % (b, k, A["bound"]))
    if aim:
        for half, name in ((tuned, "tuned"), (checked, "checked")):
            got = SA.simulate(ex, aim, half)
            for k, v in got.items():
                said = ((block["aim"][k].get("evidence") or {}).get(name) or {})
                if {x: said.get(x) for x in ("taps", "gained", "lost")} != v:
                    problems.append("keys[%d].aim.%s evidence.%s says %s; simulate_aim.py --half %s gives %s" % (b, k, name, said, half, v))
                need = A["minTunedTaps"] if name == "tuned" else A["minCheckedTaps"]
                if v["taps"] < need:
                    problems.append("keys[%d].aim.%s: %d meant taps on half %s; needs %d" % (b, k, v["taps"], half, need))
                if name == "checked" and v["gained"] < v["lost"]:
                    problems.append("keys[%d].aim.%s loses more than it gains on the check half" % (b, k))
    zones = [{"key": z.get("key"), "edge": z.get("edge"), "move": z.get("move")} for z in block.get("zones") or []]
    if zones:
        for z in zones:
            if not isinstance(z["move"], (int, float)) or abs(z["move"]) > Z["maxMove"]:
                problems.append("keys[%d] zone %s %s: move must be within +-%g" % (b, z["key"], z["edge"], Z["maxMove"]))
        for half, name in ((tuned, "tuned"), (checked, "checked")):
            per, _, _ = SZ.simulate(ex, zones, half)
            for i, (z, p) in enumerate(zip(zones, per)):
                label = "keys[%d].zones[%d] (%s %s %+g)" % (b, i, z["key"], z["edge"], z["move"])
                if "skipped" in p:
                    problems.append("%s: Hex would skip it: %s" % (label, p["skipped"])); continue
                said = ((block["zones"][i].get("evidence") or {}).get(name) or {})
                want = SZ.evidence(p)
                if {x: said.get(x) for x in want} != want:
                    problems.append("%s evidence.%s says %s; simulate_zones.py (all of this block's zones) --half %s gives %s. Simulate it, then write what it prints" % (label, name, said, half, want))
                if name == "tuned" and p["taps"] < Z["minTunedTaps"]:
                    problems.append("%s: %d taps in its strip on half %s; needs %d" % (label, p["taps"], half, Z["minTunedTaps"]))
                if name == "tuned" and SZ.net_words(p) <= 0:
                    problems.append("%s: on the tuning half it makes no more words right than wrong (%.1f, %.1f)" % (label, p["wordsRight"], p["wordsWrong"]))
                if name == "checked" and not SZ.holds(p):
                    problems.append("%s: on the check half %d taps in its strip, words right %.1f and wrong %.1f; needs %d taps, %g net words, and wrong at most %d%% of right" % (
                        label, p["taps"], p["wordsRight"], p["wordsWrong"], Z["minCheckedTaps"], Z["minNetCheckedWords"], 100 * Z["maxWrongShare"]))

canon = json.dumps({k: v for k, v in cfg.items() if k != "id"}, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
cid = "cfg-" + hashlib.sha256(canon.encode()).hexdigest()[:16]
if "--write" in sys.argv:
    cfg["id"] = cid
    open(path, "w").write(json.dumps(cfg, indent=1, ensure_ascii=False))
elif cfg.get("id") != cid:
    problems.append('"id" should be %s (run with --write)' % cid)
for p in problems:
    print("PROBLEM:", p)
for x in notes:
    print("note:", x)
print("%d problem(s); id %s; %d bytes" % (len(problems), cid, len(raw)))
sys.exit(1 if problems else 0)
