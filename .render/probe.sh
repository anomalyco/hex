set -e
BIN=$(cargo build -q --locked --bin hex --message-format=json | python3 -c "import sys,json;[print(j[\"executable\"]) for j in map(json.loads,sys.stdin) if j.get(\"executable\")]" | tail -1)
out=$PWD/captures/probe; rm -rf $out; mkdir -p $out
cache=$(getconf DARWIN_USER_CACHE_DIR)
ls "$cache/com.apple.metal" 2>/dev/null | head -20 > $out/metal-dirs.txt || true
for run in cold warm; do
  rm -rf "$cache/com.apple.metal"
  warm=; [ $run = warm ] && warm=1
  sup=$(mktemp -d); cp -R .render/support/ $sup/
  start=$(python3 -c 'import time;print(time.time())')
  env ${warm:+HEX_PROBE_WARM=1} HEX_PROBE_FRAMES=$out/$run.txt HEX_APPLICATION_SUPPORT_DIR=$sup \
    "$BIN" app -- --playground "Prototypes/Onboarding/First run" --variant "Whole run, slow download, a sketch" --theme dark --size 1446x760 --video $out/$run.mp4 --duration 12 --fps 60 --scale 2 >$out/$run.log 2>&1
  python3 -c "import time;print('$run wall', time.time()-$start)"
done
python3 - $out <<'PY'
import sys,re
out=sys.argv[1]
for run in ("cold","warm"):
    rows=[dict(kv.split("=") for kv in l.split()) for l in open(f"{out}/{run}.txt")]
    rows=[{k:float(v) for k,v in r.items()} for r in rows]
    print(run, "frames", len(rows))
    for i,r in enumerate(rows[:4]): print("  first", i, r)
    gpu=[l.strip() for l in open(f"{out}/{run}.txt.gpu")]
    for i,r in sorted(enumerate(rows), key=lambda x:-(x[1]["update"]+x[1]["compose"]))[:6]: print("  slow", i, r, gpu[i] if i < len(gpu) else "")
PY

