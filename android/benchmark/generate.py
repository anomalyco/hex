"""Generate the fixed 16 kHz/mono corpus on macOS. Music is synthesized, not sourced."""
import array, csv, hashlib, math, subprocess, sys, tempfile, wave
from pathlib import Path

dest = Path(sys.argv[1])
dest.mkdir(parents=True, exist_ok=True)
rate = 16000
with open(Path(__file__).with_name("corpus.tsv"), encoding="utf8") as src:
    rows = list(csv.DictReader(src, delimiter="\t"))
for row in rows:
    with tempfile.TemporaryDirectory(dir=dest) as tmp:
        aiff = Path(tmp) / "voice.aiff"
        subprocess.run(["say", "-v", row["voice"], "-r", row["wpm"], "-o", aiff, row["reference"]], check=True)
        raw = subprocess.check_output(["ffmpeg", "-loglevel", "error", "-i", aiff, "-f", "s16le", "-ac", "1", "-ar", str(rate), "-"])
    samples = array.array("h", raw)
    if not samples or max(samples) < 500:
        raise ValueError("Empty synthesis: " + row["id"])
    voice_rms = math.sqrt(sum(s*s for s in samples)/len(samples))
    samples = [0]*3200 + list(samples) + [0]*6400
    if row["snr_db"] != "clean":
        tones = (261.63, 329.63, 392.00, 493.88, 220.00, 261.63, 329.63, 440.00)
        bed = []
        for i in range(len(samples)):
            t = i/rate
            beat = int(t/0.24)
            phase = t%0.24
            f = tones[beat%len(tones)]
            env = min(1.,phase/.012)*math.exp(-phase*8)
            bed.append(env*(math.sin(2*math.pi*f*t)+.25*math.sin(4*math.pi*f*t))+.25*math.sin(2*math.pi*110*t))
        gain = voice_rms / (10**(float(row["snr_db"])/20)*math.sqrt(sum(s*s for s in bed)/len(bed)))
        samples = [a+b*gain for a,b in zip(samples,bed)]
    gain = min(1.,30000/max(abs(s) for s in samples))
    wav = dest / (row["id"]+".wav")
    with wave.open(str(wav), "wb") as out:
        out.setparams((1,2,rate,0,"NONE","not compressed"))
        out.writeframes(array.array("h",(round(s*gain) for s in samples)).tobytes())
    print(row["id"], len(samples), hashlib.sha256(wav.read_bytes()).hexdigest(), sep="\t")
# The long clips (long.tsv): those utterances joined end to end, for the encoder-window check.
with open(Path(__file__).with_name("long.tsv"), encoding="utf8") as src:
    for row in csv.DictReader(src, delimiter="\t"):
        joined = b""
        for part in row["clips"].split(","):
            with wave.open(str(dest / (part+".wav"))) as w: joined += w.readframes(w.getnframes())
        wav = dest / (row["id"]+".wav")
        with wave.open(str(wav), "wb") as out:
            out.setparams((1,2,rate,0,"NONE","not compressed"))
            out.writeframes(joined)
        print(row["id"], len(joined)//2, hashlib.sha256(wav.read_bytes()).hexdigest(), sep="\t")
