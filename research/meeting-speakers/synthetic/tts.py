"""Synthesize every line of calls.txt with Qwen3-TTS (MLX).

1. VoiceDesign makes one reference clip per fictional voice.
2. Base cloning speaks every line from that reference, so a voice stays the
   same across lines and across calls.

Output (under $OUT, default ./out): voices/<Name>.wav, turns/<call>/<NNN>.wav.
Existing files are kept, so the script resumes.
"""

import gc
import os
import sys
import time
from pathlib import Path

import mlx.core as mx
import numpy as np
import soundfile as sf
from mlx_audio.tts.utils import load_model

sys.path.insert(0, str(Path(__file__).resolve().parent))
from script import REFERENCE_TEXT, VOICES, load_calls  # noqa: E402

OUT = Path(os.environ.get("OUT", "out")).resolve()
DESIGN = os.environ.get("TTS_DESIGN", "mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16")
BASE = os.environ.get("TTS_BASE", "mlx-community/Qwen3-TTS-12Hz-1.7B-Base-bf16")


def collect(results) -> np.ndarray:
    return np.concatenate([np.array(r.audio, dtype=np.float32).reshape(-1) for r in results])


def main() -> None:
    calls = load_calls()
    only = set(sys.argv[1:])
    voices_dir = OUT / "voices"
    voices_dir.mkdir(parents=True, exist_ok=True)

    missing = [n for n in VOICES if not (voices_dir / f"{n}.wav").exists()]
    if missing:
        model = load_model(DESIGN)
        for i, name in enumerate(VOICES):
            if name not in missing:
                continue
            mx.random.seed(1000 + i)
            t = time.time()
            audio = collect(model.generate_voice_design(
                text=REFERENCE_TEXT, language="English", instruct=VOICES[name]))
            sf.write(voices_dir / f"{name}.wav", audio, model.sample_rate)
            print(f"voice {name}: {len(audio) / model.sample_rate:.1f}s in {time.time() - t:.1f}s", flush=True)
        del model
        gc.collect()
        mx.clear_cache()

    model = load_model(BASE)
    total_audio = total_wall = 0.0
    for call, turns in calls.items():
        if only and call not in only:
            continue
        turn_dir = OUT / "turns" / call
        turn_dir.mkdir(parents=True, exist_ok=True)
        for turn in turns:
            path = turn_dir / f"{turn.index:03d}.wav"
            if path.exists():
                continue
            mx.random.seed(sum(map(ord, call)) * 1000 + turn.index)
            t = time.time()
            audio = collect(model.generate(
                text=turn.text,
                ref_audio=str(voices_dir / f"{turn.speaker}.wav"),
                ref_text=REFERENCE_TEXT,
            ))
            sf.write(path, audio, model.sample_rate)
            wall = time.time() - t
            total_audio += len(audio) / model.sample_rate
            total_wall += wall
            print(f"{call} {turn.index:03d} {turn.speaker}: {len(audio) / model.sample_rate:.1f}s in {wall:.1f}s", flush=True)
    if total_wall:
        print(f"TTS total {total_audio:.0f}s audio in {total_wall:.0f}s ({total_audio / total_wall:.2f}x realtime)")


if __name__ == "__main__":
    main()
