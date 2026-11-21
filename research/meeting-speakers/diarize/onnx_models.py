"""ONNX models used by the sherpa-onnx pipeline, the hand-built baseline and the
shared voiceprint extractor. $MODELS points at the folder setup.sh fills."""

import os
from pathlib import Path

import numpy as np
import sherpa_onnx

MODELS = Path(os.environ.get("MODELS", "models")).resolve()
SEGMENTATION = MODELS / "sherpa-onnx-pyannote-segmentation-3-0" / "model.onnx"
SILERO = MODELS / "silero_vad.onnx"
EMBEDDERS = {
    "campplus": MODELS / "3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx",
    "wespeaker-r34": MODELS / "wespeaker_en_voxceleb_resnet34_LM.onnx",
}


def extractor(name: str, threads: int = 4) -> sherpa_onnx.SpeakerEmbeddingExtractor:
    return sherpa_onnx.SpeakerEmbeddingExtractor(
        sherpa_onnx.SpeakerEmbeddingExtractorConfig(model=str(EMBEDDERS[name]), num_threads=threads))


def embed(ex: sherpa_onnx.SpeakerEmbeddingExtractor, samples: np.ndarray, sr: int = 16000) -> np.ndarray:
    stream = ex.create_stream()
    stream.accept_waveform(sample_rate=sr, waveform=samples.astype(np.float32))
    stream.input_finished()
    v = np.asarray(ex.compute(stream), dtype=np.float32)
    return v / (np.linalg.norm(v) + 1e-9)
