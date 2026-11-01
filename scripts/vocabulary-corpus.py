#!/usr/bin/env python3
"""Generate file-only ElevenLabs fixtures. Never opens an audio device.

Credentials are supplied by the caller as ELEVENLABS_API_KEY. Existing WAVs
are reused; the manifest records the exact request and each WAV's SHA-256.
Private recordings belong in a separate, untracked manifest, never here.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import urllib.request
import wave


TERMS = ["Hex", "OpenCode", "Rune", "Scrawl", "Effect", "Anomaly",
         "Dax", "Aiden", "Kit", "Parakeet", "Moonshine", "GPUI", "Sparkle",
         "Argon", "Astra", "HiveMind"]

SENTENCES = [
    ("Hex", ["I use Hex to dictate my notes.", "Please check whether Hex heard that correctly.", "The latest Hex build is ready to test."]),
    ("OpenCode", ["OpenCode is running on my desktop.", "Send the patch to OpenCode for review.", "I started a new session in OpenCode."]),
    ("Rune", ["Build the settings panel with Rune.", "The Rune window should stay open.", "We moved the interface to Rune last week."]),
    ("Scrawl", ["Use Scrawl to capture that feedback.", "I left a note in Scrawl about the button.", "Scrawl should send the recording to this session."]),
    ("Effect", ["Use Effect to manage the background task.", "The Effect service owns the connection.", "I wrote the new client with Effect."]),
    ("Anomaly", ["Anomaly builds tools for developers.", "Send the update to the team at Anomaly.", "We should use the Anomaly account for this."]),
    ("Dax", ["Dax reviewed the new interface.", "Please ask Dax about that change.", "I think Dax already fixed this issue."]),
    ("Aiden", ["Aiden is working on the installer.", "Can Aiden review the latest build?", "I sent Aiden the notes from our meeting."]),
    ("Kit", ["Kit wants the animation to feel faster.", "Please send Kit the comparison.", "I think Kit already approved that plan."]),
    ("Parakeet", ["Parakeet transcribes the audio locally.", "Load Parakeet before starting the benchmark.", "The default model is Parakeet."]),
    ("Moonshine", ["Moonshine listens for voice commands.", "We should compare Moonshine with the other model.", "Unload Moonshine when commands are disabled."]),
    ("GPUI", ["The old interface used GPUI.", "We removed GPUI from the application.", "Does GPUI support this kind of window?"]),
    ("Sparkle", ["Sparkle handles updates on the Mac.", "The Sparkle feed has the new release.", "Check whether Sparkle found an update."]),
    ("Argon", ["Ask Argon to review the implementation.", "Argon is working on the next patch.", "Run the research task with Argon."]),
    ("Astra", ["Astra found the problem in the decoder.", "Give Astra the benchmark results.", "I want Astra to check this change."]),
    ("HiveMind", ["Route the build through HiveMind.", "HiveMind should choose an available worker.", "The HiveMind job finished successfully."]),
]

DISTRACTORS = [
    "Tom Hanks starred in that movie.",
    "I watched Avatar at the cinema.",
    "The stone has a rune carved into it.",
    "She bought two hanks of wool for the scarf.",
    "Choose an avatar for your profile.",
    "The ancient rune was difficult to read.",
    "Please check the text before you send it.",
    "The checks arrived in the mail today.",
    "They stretched their necks to see the stage.",
    "The hexadecimal number begins with a letter.",
    "Put the toolkit back on the shelf.",
    "The side effects should go away soon.",
    "There was an anomaly in the weather report.",
    "The water began to sparkle in the sunlight.",
    "Her handwriting was a hurried scrawl.",
    "The parakeet sat on its perch.",
    "We packed a first aid kit for the hike.",
    "The moonshine lit the path through the trees.",
]


def definitions():
    clips = []
    for term, sentences in SENTENCES:
        for index, text in enumerate(sentences):
            ident = f"term-{term.lower()}-{index + 1}"
            clips.append(dict(id=ident, audio=f"audio/{ident}.wav", expected=text,
                              expected_terms=[term], group="target"))
    for index, text in enumerate(DISTRACTORS):
        ident = f"distractor-{index + 1:02}"
        clips.append(dict(id=ident, audio=f"audio/{ident}.wav", expected=text,
                          expected_terms=[], group="distractor"))
    return clips


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--generate", action="store_true", help="Bill TTS credits and save WAVs")
    parser.add_argument("--limit", type=int)
    args = parser.parse_args()
    clips = definitions()
    print(f"{len(clips)} clips, {sum(len(c['expected']) for c in clips)} characters")
    if not args.generate:
        return
    root = args.output
    (root / "audio").mkdir(parents=True, exist_ok=True)
    voice = "8olojUk4IXpvKgaOCHXj"
    provenance = dict(provider="ElevenLabs", model="eleven_v4", voice=voice,
                      stability=0.5, similarity=0.75, sample_rate=16000,
                      reference="TTS input, not human-audited speech", synthetic=True)
    manifest_path = root / "manifest.json"
    previous = json.loads(manifest_path.read_text()) if manifest_path.exists() else {}
    old_clips = {c["id"]: c for c in previous.get("clips", [])}
    completed = []
    generated = 0
    for clip in clips:
        path = root / clip["audio"]
        reused = path.exists()
        old = old_clips.get(clip["id"])
        if path.exists() and (not old or old["expected"] != clip["expected"] or
                              previous.get("provenance") != provenance):
            raise ValueError(f"request changed or receipt missing for {path}; remove that generated WAV explicitly before regenerating")
        if not path.exists() and args.limit is not None and generated >= args.limit:
            continue
        if not path.exists():
            body = dict(model_id="eleven_v4", inputs=[dict(voice_id=voice, text=clip["expected"])],
                        settings=dict(stability=0.5, similarity=0.75), use_pvc_as_ivc=False)
            request = urllib.request.Request(
                "https://api.elevenlabs.io/v1/text-to-dialogue?output_format=pcm_16000",
                data=json.dumps(body).encode(),
                headers={"xi-api-key": os.environ["ELEVENLABS_API_KEY"], "Content-Type": "application/json"})
            with urllib.request.urlopen(request, timeout=120) as response:
                pcm = response.read()
            if not pcm or len(pcm) % 2:
                raise ValueError(f"invalid PCM for {clip['id']}")
            temporary = path.with_suffix(".partial")
            with wave.open(str(temporary), "wb") as wav:
                wav.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
                wav.writeframes(pcm)
            temporary.replace(path)
            generated += 1
        clip["sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
        if reused and old and old.get("sha256") and old["sha256"] != clip["sha256"]:
            raise ValueError(f"WAV checksum changed: {path}")
        with wave.open(str(path)) as wav:
            clip["duration_s"] = wav.getnframes() / wav.getframerate()
        print(clip["id"], clip["duration_s"], flush=True)
        completed.append(clip)
        # Checkpoint successful requests so an interrupted run does not re-bill.
        retained = {c["id"]: c for c in previous.get("clips", [])}
        retained.update({c["id"]: c for c in completed})
        manifest = dict(vocabulary=TERMS, provenance=provenance,
                        clips=[retained[c["id"]] for c in clips if c["id"] in retained])
        temporary = manifest_path.with_suffix(".partial")
        temporary.write_text(json.dumps(manifest, indent=2) + "\n")
        temporary.replace(manifest_path)


if __name__ == "__main__":
    main()
