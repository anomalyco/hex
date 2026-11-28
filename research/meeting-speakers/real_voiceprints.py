"""One CAM++ voiceprint per found speaker, for each meeting and system.

usage: real_voiceprints.py AUDIO_DIR RESULTS_DIR  ->  RESULTS_DIR/voiceprints.json
       {system: {meeting: {speaker: [floats], ...}}}
"""

import json
import sys
from pathlib import Path

import soundfile as sf

from score import read_rttm, voiceprints

sys.path.insert(0, str(Path(__file__).resolve().parent / "diarize"))
from onnx_models import extractor  # noqa: E402


def main() -> None:
    audio_dir, results = Path(sys.argv[1]), Path(sys.argv[2])
    ex = extractor("campplus")
    out: dict = {}
    for wav in sorted(audio_dir.glob("*.wav")):
        audio = sf.read(wav, dtype="float32")[0]
        for sdir in sorted(d for d in results.iterdir() if d.is_dir()):
            rttm = sdir / f"{wav.stem}.rttm"
            if rttm.exists():
                prints = voiceprints(read_rttm(rttm), audio, ex)
                out.setdefault(sdir.name, {})[wav.stem] = {k: [round(float(x), 5) for x in v] for k, v in prints.items()}
    (results / "voiceprints.json").write_text(json.dumps(out))


if __name__ == "__main__":
    main()
