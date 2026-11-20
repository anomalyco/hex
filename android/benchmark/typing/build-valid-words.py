"""Offline substitute for iOS checking whether an English literal outside HXTM is valid.
Extract only public SCOWL spellings absent from the unchanged shared frequency model.
    python3 build-valid-words.py scowl-2020.12.07.tar.gz en.hextm en.valid
"""
import hashlib, re, struct, sys, tarfile
from pathlib import Path

source, model, output = map(Path, sys.argv[1:])
assert hashlib.sha256(source.read_bytes()).hexdigest() == "5587667caa20c4891390c2d42dbb4d5c4c3f41bee77af1457ece3ba23fb859cc"
data = model.read_bytes()
assert data[:8] == b"HXTM\x03\0\0\0" and data[24:28] == b"en\0\0"
n, _, size, a = struct.unpack_from("<4I", data, 8)
letters = {data[32+i*8]: chr(struct.unpack_from("<I", data, 28+i*8)[0]) for i in range(a)}
offsets = struct.unpack_from("<" + str(n+1) + "I", data, 28+a*8)
blob = data[-size:]
scored = {"".join(letters[c] for c in blob[offsets[i]:offsets[i+1]]) for i in range(n)}
valid = set()
with tarfile.open(source) as archive:
    for entry in archive:
        m = re.fullmatch(r".*/final/(english|british|american)-(words|contractions|proper-names|upper|abbreviations)\.(\d+)", entry.name)
        if m and int(m[3]) <= 70:
            for w in archive.extractfile(entry).read().decode("latin-1").lower().split():
                if re.fullmatch("[a-z]{2,32}", w) and w not in scored:
                    valid.add(w)
output.write_bytes(("".join(w + "\n" for w in sorted(valid))).encode("ascii"))
print(f"{len(valid)} non-model spellings, {output.stat().st_size} bytes; {hashlib.sha256(output.read_bytes()).hexdigest()}")
