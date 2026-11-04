"""Build the artifact's index.html from the synthetic summary and the private
review summary (speaker counts and speeds only; no titles or text).

usage: build_index.py SYNTHETIC_SUMMARY_JSON ARTIFACT_DIR
"""

import html
import json
import sys
from pathlib import Path

SYSTEMS = [  # name, runtime, note
    ("nemotron3-offline-preset", "Nemotron-3 Diarization", "NeMo-Speech.cpp · ggml Metal · v3-offline preset"),
    ("nemotron3-stream", "Nemotron-3 Diarization", "NeMo-Speech.cpp · ggml Metal · default streaming"),
    ("fluidaudio-nemotron3-offline", "Nemotron-3 Diarization", "FluidAudio · CoreML · offline"),
    ("fluidaudio-nemotron3-fast32", "Nemotron-3 Diarization", "FluidAudio · CoreML · fast32"),
    ("nemotron3-full-attention", "Nemotron-3 Diarization", "NeMo-Speech.cpp · --offline full attention (≤6.6 min)"),
    ("sortformer-v2-stream", "Sortformer 4spk v2", "NeMo-Speech.cpp · ggml Metal · streaming"),
    ("sortformer-v2-offline-preset", "Sortformer 4spk v2", "NeMo-Speech.cpp · offline preset"),
    ("transcribecpp-sortformer-v21", "Sortformer 4spk v2.1", "transcribe.cpp (Hex's runtime, pinned rev) · Metal"),
    ("pyannote-community1-mps", "pyannote community-1", "pyannote.audio 4 · PyTorch MPS"),
    ("pyannote-community1-cpu", "pyannote community-1", "pyannote.audio 4 · PyTorch CPU †"),
    ("fluidaudio-community1-offline", "pyannote community-1", "FluidAudio · CoreML · VBx"),
    ("sherpa-seg3-wespeaker", "seg-3.0 + WeSpeaker", "sherpa-onnx · ONNX Runtime CPU"),
    ("sherpa-seg3-campplus", "seg-3.0 + CAM++", "sherpa-onnx · threshold 0.5 (untuned)"),
    ("baseline-vad-campplus-ahc", "Silero + CAM++ + AHC", "ONNX Runtime · 1.5 s windows · threshold 0.6"),
]
PRIMARY = {"nemotron3-offline-preset", "transcribecpp-sortformer-v21"}
REAL = [
    ("nemotron3-offline-preset", "Nemotron-3 (v3-offline)"),
    ("nemotron3-stream", "Nemotron-3 (streaming)"),
    ("pyannote-community1-mps", "pyannote community-1"),
    ("transcribecpp-sortformer-v21", "Sortformer v2.1 (transcribe.cpp)"),
]
CANDIDATES = [
    ("Nemotron-3 Diarization", "OpenMDW-1.1", "107 MB q8_0 GGUF · 190 MB per CoreML preset", "8",
     "No; add an embedding model", "NeMo-Speech.cpp C ABI (<code>nemo_speech/diar.h</code>, Apache-2.0, own ggml dylibs ~9 MB), "
     "or port the V3 head into transcribe.cpp's Sortformer family → <code>transcribe_cpp::DiarizeSession</code>"),
    ("Sortformer 4spk v2 / v2.1", "CC-BY-4.0 / NVIDIA Open Model License", "147 / 139 MB q8_0 GGUF", "4",
     "No", "<b>Already in Hex's pinned transcribe.cpp</b>: <code>Model::diarize_info</code>, <code>DiarizeSession::run(pcm)</code>; no new dependency"),
    ("Multitalker Parakeet", "NVIDIA Open Model License", "~0.6 B params (ASR + embedded Sortformer)", "4",
     "No", "In transcribe.cpp's catalog; speaker-tagged ASR, a bigger change than post-call diarization (not run)"),
    ("pyannote community-1", "CC-BY-4.0 weights, MIT code", "32 MB (PyTorch) · 21 MB CoreML", "Any (clustering)",
     "Yes, 256-d WeSpeaker", "No Rust path; PyTorch, or FluidAudio's CoreML port from a Swift bridge"),
    ("FluidAudio (SDK)", "Apache-2.0", "see models", "8 / any",
     "Yes (community-1 pipeline)", "Swift package: a SwiftPM dependency behind a bridge like <code>apple_speech.swift</code>; macOS only"),
    ("Silero + CAM++ + AHC", "MIT + Apache-2.0", "0.6 MB + 29 MB ONNX", "Any",
     "Yes, 192-d", "<code>ort</code> crate (Hex ships no ONNX Runtime today) or a ggml port"),
    ("sherpa-onnx seg-3.0 + embedder", "Apache-2.0 / MIT", "6 MB + 26–29 MB ONNX", "Any",
     "Yes", "sherpa-onnx C API, bundles ONNX Runtime"),
]

CSS = """
:root{color-scheme:dark}body{background:#0d0f14;color:#d7dae0;font:15px/1.55 -apple-system,BlinkMacSystemFont,Inter,sans-serif;margin:0}
main{max-width:1120px;margin:0 auto;padding:40px 24px 80px}h1{font-size:26px;margin:0 0 4px}h2{font-size:18px;margin:40px 0 10px;color:#fff}
.sub{color:#8a90a0;font-size:13px}.verdict{background:#141821;border:1px solid #2a3140;border-radius:10px;padding:16px 20px;margin:20px 0}
.verdict b{color:#fff}.verdict ol{margin:8px 0 0 18px;padding:0}table{border-collapse:collapse;width:100%;font-size:13px;margin:8px 0}
th,td{padding:6px 10px;border-bottom:1px solid #1e232d;text-align:left;vertical-align:top}th{color:#8a90a0;font-weight:500;white-space:nowrap}
td.n{text-align:right;font-variant-numeric:tabular-nums;white-space:nowrap}tr.hi td{background:#121a24}tr.hi td:first-child{box-shadow:inset 3px 0 #7aa2f7}
.good{color:#9ece6a}.bad{color:#f7768e}code{background:#1a1e27;padding:1px 5px;border-radius:4px;font-size:12px}
.calls{display:grid;grid-template-columns:repeat(auto-fill,minmax(330px,1fr));gap:12px}.call{background:#141821;border:1px solid #232937;border-radius:8px;padding:12px}
.call audio{width:100%;margin-top:8px}.private{background:#3b1d22;color:#ffb4bd;border:1px solid #6b2a35;padding:2px 8px;border-radius:6px;font-size:12px}
a{color:#7aa2f7}.note{color:#9aa0ad;font-size:13px}ul{padding-left:20px}
"""


def pct(x, digits=1):
    return "–" if x is None else f"{x * 100:.{digits}f}"


def main() -> None:
    summary = {s["system"]: s for s in json.loads(Path(sys.argv[1]).read_text())}
    artifact = Path(sys.argv[2])
    real = json.loads((artifact / "private" / "summary.json").read_text())
    calls = sorted((artifact / "synthetic").glob("call*.m4a"))
    meta = {}
    for c in calls:
        segs = [l.split() for l in (artifact / "synthetic" / f"{c.stem}.rttm").read_text().splitlines() if l.strip()]
        meta[c.stem] = (sorted({s[7] for s in segs}), max(float(s[3]) + float(s[4]) for s in segs))

    rows = []
    for name, model, runtime in SYSTEMS:
        s = summary.get(name)
        if not s:
            continue
        rt = s["rtfx"] or 0
        speed = f"{rt:.0f}×" if rt >= 10 else f"{rt:.1f}×"
        rows.append(
            f'<tr class="{"hi" if name in PRIMARY else ""}"><td><b>{model}</b><br><span class="sub">{runtime}</span></td>'
            f'<td class="n">{pct(s["mean_der"])}</td><td class="n">{pct(s["mean_confusion"])}</td>'
            f'<td class="n">{s["count_exact"]}/{len(s["calls"])}</td><td class="n">{speed}</td>'
            f'<td class="n">{(s["peak_mb"] or 0):.0f}</td><td class="n">{pct(s["cross_call_accuracy"], 0)}%</td>'
            f'<td class="n">{pct(s["reference_accuracy"], 0)}%</td>'
            f'<td class="n">{pct(s["word_level"])} / {pct(s["segment_level"])}</td></tr>')

    real_rows = []
    for m in real:
        cells = "".join(f'<td class="n">{m["speakers"].get(k, "–")} · {m["speed"].get(k, 0):.0f}×</td>' for k, _ in REAL)
        real_rows.append(f'<tr><td><a href="private/{m["meeting"]}.html">{m["meeting"]}</a> <span class="private">private</span></td>'
                         f'<td class="n">{m["minutes"]:.0f} min</td>{cells}</tr>')

    cand = "".join(f"<tr><td><b>{a}</b></td><td>{b}</td><td>{c}</td><td>{d}</td><td>{e}</td><td>{f}</td></tr>" for a, b, c, d, e, f in CANDIDATES)
    players = "".join(
        f'<div class="call"><b>{c.stem}</b> <span class="sub">{meta[c.stem][1] / 60:.1f} min · {len(meta[c.stem][0])} voices: '
        f'{html.escape(", ".join(meta[c.stem][0]))}</span><audio controls preload="none" src="synthetic/{c.name}"></audio>'
        f'<span class="sub"><a href="synthetic/{c.stem}.rttm">truth RTTM</a></span></div>' for c in calls)

    page = f"""<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Meeting speakers · diarization spike</title><style>{CSS}</style></head><body><main>
<h1>Who said what in “Them”: on-device diarization for Hex meetings</h1>
<div class="sub">Spike t1303 · 2026-11-04 · measurement only, no Hex change · synthetic on M2 Max, real on M4 Pro (both shared with other jobs)</div>

<div class="verdict"><b>Use NVIDIA Nemotron-3 Diarization, running in transcribe.cpp, plus a separate voiceprint model.</b>
<ol>
<li><b>Accuracy.</b> It handles up to 8 speakers. It scored 9.1–9.5% DER on the synthetic calls (pyannote 14.3%) and got the speaker count exactly right on 4 of 5 calls. It finds the same 6–7 speakers as pyannote on the two big real meetings, and gives each speaker 97% of their words.</li>
<li><b>How it runs in Hex.</b> transcribe.cpp is already linked, and Hex's pinned rev already exposes <code>DiarizeSession</code>. It runs Sortformer v2.1 today at 180× realtime with 11.8% DER, but that model stops at 4 speakers. The remaining work is porting the V3 8-speaker head into that model family. NeMo-Speech.cpp's ggml version, which already runs V3 at 400–640× realtime, is the reference for the port.</li>
<li><b>Size and voiceprints.</b> The model is 107 MB (q8_0). Voiceprints need a separate embedding model run over each speaker's clean stretches. CAM++ named people across calls correctly 81–94% of the time. It couldn't tell an unknown voice apart at a fixed threshold, though, so Hex should suggest names rather than assign them.</li>
</ol></div>

<h2>Synthetic calls: scores</h2>
<p class="note">5 calls, 2–5 voices made with Qwen3-TTS (VoiceDesign references, Base clones), 3–5% overlap. Each call went through 16 kHz, Opus or AAC at 16–32 kb/s, noise at 20–28 dB SNR and level gaps of up to 9 dB.
DER uses a 0.25 s collar and scores overlap. About 9 points of every DER is a floor: the reference marks short pauses inside a turn as speech.
x-RT is audio seconds per wall second, not counting model load. Voiceprints use one CAM++ extractor over each found speaker's solo stretches of 0.5 s or more, cosine ≥ 0.5.
“Cross-call” enrols each voice from the other calls; “clean ref” enrols from one clean 10 s clip.</p>
<table><tr><th>System</th><th>DER %</th><th>Confusion %</th><th>Count exact</th><th>x-RT</th><th>Peak MB</th><th>Cross-call</th><th>Clean ref</th><th>Words right: word / segment</th></tr>
{"".join(rows)}</table>
<p class="note">† PyTorch CPU ran next to an Xcode release build on the same Mac, so its speed is unreliable. Every speed here was measured on a shared worker; compare systems run on the same machine. Call 4 has 5 voices, and every system with an 8-speaker limit still merged two of them (one has 18 s of speech).
Word vs segment: the gap is under 1 point everywhere, because Hex's “Them” lines are short. Giving each line its majority speaker is enough.</p>

<h2>Real meetings (system.wav only)</h2>
<p class="note">Each cell is speakers found · x-RT on M4 Pro. Hex's transcript doesn't say how many people were on a call, so open a page, play it, and check the colours.
Each page colours every “Them” line by Nemotron-3's speaker, shows the other systems' timelines next to it, and marks lines where a second speaker holds over 30% of the line.</p>
<table><tr><th>Meeting</th><th>Length</th>{"".join(f"<th>{n}</th>" for _, n in REAL)}</tr>{"".join(real_rows)}</table>
<ul class="note">
<li><b>m1 and m2:</b> Nemotron-3 hears one remote voice. On m2, Sortformer v2.1 and pyannote both split off a second voice of about 1.5–2.5 min (CAM++ puts it 0.74–0.83 from the main voice), and Nemotron-3 folds it in. Worth a listen.</li>
<li><b>m3 and m4:</b> Nemotron-3 and pyannote agree on 6–7 speakers. Sortformer v2.1 has to squeeze them into 4, which costs 21–29% line agreement.</li>
<li><b>Memory:</b> on the 85 min meeting, both ggml runtimes peaked near 2 GB (pyannote MPS 8.7 GB). Hex should feed long meetings through the streaming API rather than as one whole buffer.</li>
</ul>

<h2>Candidates</h2>
<table><tr><th>Model</th><th>Licence</th><th>Download</th><th>Max speakers</th><th>Voiceprint</th><th>From Rust</th></tr>{cand}</table>

<h2>Voiceprints: what works and what doesn't</h2>
<ul class="note">
<li>Naming by nearest match works: Nemotron-3 + CAM++ named 15 of 16 cross-call appearances. The one miss was the voice that appears only once (Marcus), and it was confidently matched to someone else.</li>
<li>CAM++ cosines are squeezed together on codec audio: the same voice scores 0.85–0.94 and the closest different voice 0.81–0.89. That leaves no fixed threshold that rejects strangers. TTS clones from one model probably make this worse than real people would. Before Hex auto-assigns names, calibrate on real enrolments (score normalisation, or a margin over the second-best match).</li>
<li>Hex ships no ONNX Runtime. To keep voiceprints on ggml, a speaker-verification ECAPA-TDNN fits transcribe.cpp's existing ECAPA family (used today for language ID). It hasn't been measured yet.</li>
</ul>

<h2>Synthetic calls</h2>
<div class="calls">{players}</div>
<p class="note">Code: <code>research/meeting-speakers/</code> in the Hex repo (draft PR). Real audio, transcripts and results stay on the owner's machines, in <code>private/</code> here.</p>
</main></body></html>"""
    (artifact / "index.html").write_text(page)
    print(artifact / "index.html")


if __name__ == "__main__":
    main()
