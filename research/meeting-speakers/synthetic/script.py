"""Parse calls.txt into calls of turns, and hold the fictional voice designs."""

from dataclasses import dataclass
from pathlib import Path

HERE = Path(__file__).resolve().parent

# Qwen3-TTS VoiceDesign prompts. Each voice is made once, then cloned for every
# line so a speaker sounds the same in every call they join.
VOICES = {
    "Maya": "A clear, warm female voice in her early thirties with a neutral American accent, medium pitch, speaking briskly and brightly.",
    "Tom": "A relaxed middle-aged British man with a low, slightly gravelly voice, speaking calmly and a little slowly.",
    "Priya": "A confident woman in her late thirties with a light Indian English accent, medium-low pitch, articulate and steady.",
    "Jake": "A young American man in his mid twenties with a bright, slightly nasal tenor voice, casual and quick.",
    "Elena": "A mature woman in her fifties with a soft Spanish accent in English, low warm pitch, measured and gentle pace.",
    "Sam": "A man in his thirties with an Australian accent, mid-range voice, friendly and laid back.",
    "Marcus": "A deep, resonant bass voice of a man in his forties, energetic and authoritative, American accent.",
    "Grace": "A young woman in her twenties with a high, light, gentle voice and a slight Irish lilt.",
}

REFERENCE_TEXT = (
    "Hi, it's me. I'm just checking my microphone before the call starts, "
    "so I'll read this sentence at a normal pace and you can tell me how it sounds."
)


@dataclass
class Turn:
    index: int
    kind: str  # "turn" | "barge" | "back"
    speaker: str
    text: str


def load_calls(path: Path = HERE / "calls.txt") -> dict[str, list[Turn]]:
    calls: dict[str, list[Turn]] = {}
    current = None
    for raw in path.read_text().splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("== "):
            current = line[3:].strip()
            calls[current] = []
            continue
        kind = "turn"
        if line[0] == "~":
            kind, line = "barge", line[1:]
        elif line[0] == "+":
            kind, line = "back", line[1:]
        name, text = line.split(":", 1)
        name = name.strip()
        assert name in VOICES, name
        calls[current].append(Turn(len(calls[current]), kind, name, text.strip()))
    return calls


if __name__ == "__main__":
    for call, turns in load_calls().items():
        speakers = sorted({t.speaker for t in turns})
        words = sum(len(t.text.split()) for t in turns)
        print(call, len(turns), "turns", words, "words", speakers)
