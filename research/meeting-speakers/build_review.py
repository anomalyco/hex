"""Build a private review page per real meeting: Hex's transcript lines coloured
by the speaker a diarizer assigned (segment-level: the speaker with the most
time inside the line), a speaker timeline, and the other systems' agreement.

Runs on the owner's Mac only. Pages and audio go to $ARTIFACT/private/, never
into the repository.

usage: build_review.py PRIVATE_DIR MEETINGS_DIR ARTIFACT_DIR [primary-system]
  PRIVATE_DIR holds map.txt ("m1 <meeting-id>") and results/<system>/m1.rttm
"""

import html
import json
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

import numpy as np

COLORS = ["#7aa2f7", "#f7768e", "#9ece6a", "#e0af68", "#bb9af7", "#7dcfff", "#ff9e64", "#c0caf5", "#73daca", "#db4b4b"]
NICE = {
    "nemotron3-stream": "Nemotron-3 (streaming)",
    "nemotron3-offline-preset": "Nemotron-3 (v3-offline preset)",
    "pyannote-community1-mps": "pyannote community-1",
    "sherpa-seg3-campplus": "seg-3.0 + CAM++ (sherpa-onnx)",
    "transcribecpp-sortformer-v21": "Sortformer v2.1 (transcribe.cpp, 4 max)",
}


def read_rttm(path: Path) -> list[tuple[float, float, str]]:
    segs = []
    for line in path.read_text().splitlines():
        p = line.split()
        if len(p) >= 8 and p[0] == "SPEAKER":
            segs.append((float(p[3]), float(p[3]) + float(p[4]), p[7]))
    return sorted(segs)


def dominant(segs, a: float, b: float) -> tuple[str | None, float, dict]:
    share = defaultdict(float)
    for s, e, lab in segs:
        o = min(e, b) - max(s, a)
        if o > 0:
            share[lab] += o
    if not share:
        mid = (a + b) / 2
        near = min(segs, key=lambda x: min(abs(x[0] - mid), abs(x[1] - mid)), default=None)
        return (near[2] if near else None), 0.0, {}
    lab = max(share, key=share.get)
    return lab, share[lab] / max(sum(share.values()), 1e-9), dict(share)


def rename_by_time(segs) -> dict[str, str]:
    """Speaker 1 talks most, Speaker 2 next, ..."""
    total = defaultdict(float)
    for s, e, lab in segs:
        total[lab] += e - s
    return {lab: f"Speaker {i + 1}" for i, lab in enumerate(sorted(total, key=total.get, reverse=True))}


def map_onto(primary, other) -> dict[str, str]:
    """Each label of `other` -> the primary label it overlaps most."""
    out = {}
    for lab in {x[2] for x in other}:
        share = defaultdict(float)
        for s, e, l in other:
            if l != lab:
                continue
            for ps, pe, pl in primary:
                o = min(e, pe) - max(s, ps)
                if o > 0:
                    share[pl] += o
        out[lab] = max(share, key=share.get) if share else None
    return out


def fmt_t(t: float) -> str:
    return f"{int(t // 3600)}:{int(t % 3600 // 60):02d}:{int(t % 60):02d}" if t >= 3600 else f"{int(t // 60)}:{int(t % 60):02d}"


def page(mid: str, meeting: Path, results: Path, out_dir: Path, primary: str, voiceprints: dict, all_names: dict) -> dict:
    manifest = json.loads((meeting / "manifest.json").read_text())
    duration = manifest["duration_ms"] / 1000
    lines = [json.loads(l) for l in (meeting / "transcript.ndjson").read_text().splitlines() if l.strip()]
    systems = {d.name: read_rttm(d / f"{mid}.rttm") for d in sorted(results.iterdir()) if (d / f"{mid}.rttm").exists()}
    metas = {d.name: json.loads((d / f"{mid}.json").read_text()) for d in results.iterdir() if (d / f"{mid}.json").exists()}
    prim = systems[primary]
    names = rename_by_time(prim)
    color = {lab: COLORS[i % len(COLORS)] for i, lab in enumerate(sorted(names, key=lambda l: int(names[l].split()[1])))}

    audio = out_dir / f"{mid}.m4a"
    if not audio.exists():
        subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(meeting / "system.wav"), "-ac", "1", "-ar", "16000",
                        "-c:a", "aac", "-b:a", "24k", str(audio)], check=True)

    others = {s: map_onto(prim, segs) for s, segs in systems.items() if s != primary}
    rows, agree = [], defaultdict(lambda: [0, 0])
    split_lines = 0
    for line in lines:
        a, b = line["start_ms"] / 1000, max(line["end_ms"], line["start_ms"] + 300) / 1000
        text = html.escape(line["text"])
        if line["source"] != "system":
            rows.append(f'<div class="ln you" data-t="{a:.2f}"><span class="t">{fmt_t(a)}</span><span class="chip you">You</span><span class="tx">{text}</span></div>')
            continue
        lab, frac, _ = dominant(prim, a, b)
        marks = ""
        if frac and frac < 0.7:
            split_lines += 1
            marks += '<span class="mark" title="another speaker holds over 30% of this line">split?</span>'
        dis = []
        for s, mapping in others.items():
            olab, _, _ = dominant(systems[s], a, b)
            agree[s][1] += 1
            if mapping.get(olab) == lab:
                agree[s][0] += 1
            else:
                dis.append(NICE.get(s, s))
        if dis:
            marks += f'<span class="mark dim" title="disagrees: {html.escape(", ".join(dis))}">≠{len(dis)}</span>'
        name = names.get(lab, "?")
        c = color.get(lab, "#888")
        rows.append(f'<div class="ln" data-t="{a:.2f}" style="--c:{c}"><span class="t">{fmt_t(a)}</span>'
                    f'<span class="chip" style="background:{c}">{name}</span><span class="tx">{text}</span>{marks}</div>')

    def track(segs, mapping=None):
        out = []
        for s, e, lab in segs:
            key = mapping.get(lab) if mapping is not None else lab
            c = color.get(key, "#555")
            out.append(f'<i style="left:{100 * s / duration:.3f}%;width:{max(100 * (e - s) / duration, 0.05):.3f}%;background:{c}"></i>')
        return "".join(out)

    talk = defaultdict(float)
    for s, e, lab in prim:
        talk[lab] += e - s
    speakers_html = "".join(
        f'<div class="spk"><span class="chip" style="background:{color[l]}">{names[l]}</span> {fmt_t(talk[l])} talking'
        f'{similar_html(mid, l, primary, voiceprints, all_names)}</div>'
        for l in sorted(names, key=lambda l: int(names[l].split()[1])))
    tracks = [f'<div class="trk"><b>{NICE.get(primary, primary)}</b><div class="bar">{track(prim)}</div></div>']
    for s, mapping in others.items():
        tracks.append(f'<div class="trk"><b>{NICE.get(s, s)}</b><div class="bar">{track(systems[s], mapping)}</div></div>')
    table = "".join(
        f"<tr><td>{NICE.get(s, s)}</td><td>{len({x[2] for x in segs})}</td>"
        f"<td>{metas.get(s, {}).get('audio_seconds', 0) / max(metas.get(s, {}).get('wall_seconds', 1e-9), 1e-9):.0f}×</td>"
        f"<td>{'—' if s == primary else f'{100 * agree[s][0] / max(agree[s][1], 1):.0f}%'}</td></tr>"
        for s, segs in systems.items())

    sys_lines = sum(1 for l in lines if l["source"] == "system")
    doc = f"""<!doctype html><html><head><meta charset="utf-8"><title>PRIVATE · {html.escape(manifest['title'])}</title>
<meta name="viewport" content="width=device-width,initial-scale=1"><meta name="robots" content="noindex">
<style>{CSS}</style></head><body>
<div class="private">Private · real meeting · stays on the owner's machines</div>
<h1>{html.escape(manifest['title'])}</h1>
<p class="sub">{html.escape(manifest.get('app', ''))} · {fmt_t(duration)} · {sys_lines} "Them" transcript lines · coloured by {NICE.get(primary, primary)} · <a href="../index.html">back to results</a></p>
<audio id="a" controls preload="none" src="{audio.name}"></audio>
<h2>Speakers found</h2><div class="spks">{speakers_html}</div>
<table><tr><th>System</th><th>Speakers</th><th>Speed</th><th>Agrees with primary (lines)</th></tr>{table}</table>
<h2>Timeline</h2><div class="tracks">{''.join(tracks)}</div>
<p class="sub">Other systems are coloured by the primary speaker they overlap most. {split_lines} lines are marked <span class="mark">split?</span> where a second speaker holds over 30% of the line; word-level times would split those.</p>
<h2>Transcript</h2><p class="sub">Click a line to play it. "You" lines come from the microphone track and are not diarized.</p>
<div class="tr">{''.join(rows)}</div>
<script>const a=document.getElementById('a');document.querySelectorAll('.ln').forEach(l=>l.onclick=()=>{{a.currentTime=+l.dataset.t;a.play()}});</script>
</body></html>"""
    (out_dir / f"{mid}.html").write_text(doc)
    return dict(meeting=mid, title=manifest["title"], app=manifest.get("app"), minutes=duration / 60,
                speakers={s: len({x[2] for x in segs}) for s, segs in systems.items()},
                speed={s: metas.get(s, {}).get("audio_seconds", 0) / max(metas.get(s, {}).get("wall_seconds", 1e-9), 1e-9) for s in systems},
                agreement={s: agree[s][0] / max(agree[s][1], 1) for s in agree}, split_lines=split_lines, system_lines=sys_lines)


def similar_html(mid, lab, primary, voiceprints, all_names) -> str:
    vp = voiceprints.get(primary, {})
    if lab not in vp.get(mid, {}):
        return ""
    v = np.array(vp[mid][lab])
    hits = []
    for other, prints in vp.items():
        if other == mid:
            continue
        for olab, ov in prints.items():
            s = float(v @ np.array(ov))
            if s >= 0.6:
                hits.append((s, other, olab))
    hits.sort(reverse=True)
    if not hits:
        return ""
    txt = ", ".join(f'<a href="{o}.html">{o} {all_names[o].get(l, l)}</a> ({s:.2f})' for s, o, l in hits[:3])
    return f'<span class="sim"> · same voice as {txt}?</span>'


CSS = """
:root{color-scheme:dark}body{background:#0f1117;color:#d6d9e0;font:15px/1.5 -apple-system,BlinkMacSystemFont,Inter,sans-serif;max-width:1100px;margin:0 auto;padding:24px}
a{color:#7aa2f7}h1{font-size:22px;margin:8px 0 2px}h2{font-size:16px;margin:28px 0 8px;color:#fff}.sub{color:#8a90a0;font-size:13px}
.private{background:#3b1d22;color:#ffb4bd;border:1px solid #6b2a35;padding:6px 10px;border-radius:6px;font-size:13px;display:inline-block}
audio{width:100%;margin:12px 0}table{border-collapse:collapse;margin:12px 0;font-size:13px}td,th{padding:4px 12px;border-bottom:1px solid #23262f;text-align:left}th{color:#8a90a0;font-weight:500}
.spks{display:flex;flex-direction:column;gap:4px;font-size:13px}.chip{display:inline-block;color:#0f1117;font-weight:600;font-size:12px;border-radius:4px;padding:0 6px;min-width:70px;text-align:center}
.chip.you{background:#3a3f4b;color:#aab}.sim{color:#8a90a0}
.tracks{display:flex;flex-direction:column;gap:6px}.trk{display:grid;grid-template-columns:230px 1fr;align-items:center;font-size:12px}.trk b{font-weight:500;color:#aab}
.bar{position:relative;height:16px;background:#181b23;border-radius:3px;overflow:hidden}.bar i{position:absolute;top:0;bottom:0}
.tr{display:flex;flex-direction:column}.ln{display:grid;grid-template-columns:56px 90px 1fr auto;gap:8px;padding:3px 6px;border-left:3px solid var(--c,#333);cursor:pointer;align-items:baseline}
.ln:hover{background:#181b23}.ln.you{opacity:.55;border-left-color:#333}.t{color:#6b7080;font-size:12px;font-variant-numeric:tabular-nums}
.mark{font-size:11px;color:#e0af68;border:1px solid #5a4a2a;border-radius:3px;padding:0 4px;margin-left:4px}.mark.dim{color:#8a90a0;border-color:#333}
"""


def main() -> None:
    private, meetings, artifact = map(Path, sys.argv[1:4])
    primary = sys.argv[4] if len(sys.argv) > 4 else "nemotron3-stream"
    out_dir = artifact / "private"
    out_dir.mkdir(parents=True, exist_ok=True)
    results = private / "results"
    vp_file = results / "voiceprints.json"
    voiceprints = json.loads(vp_file.read_text()) if vp_file.exists() else {}
    mapping = dict(l.split() for l in (private / "map.txt").read_text().splitlines() if l.strip())
    all_names = {mid: rename_by_time(read_rttm(results / primary / f"{mid}.rttm")) for mid in mapping}
    summary = [page(mid, meetings / mid_id, results, out_dir, primary, voiceprints, all_names) for mid, mid_id in mapping.items()]
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=1))
    for s in summary:
        print(s["meeting"], f"{s['minutes']:.0f} min", s["speakers"], {k: f"{v:.0%}" for k, v in s["agreement"].items()})


if __name__ == "__main__":
    main()
