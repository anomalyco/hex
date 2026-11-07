set -e
BIN=$(cargo build -q --locked --bin hex --message-format=json | python3 -c "import sys,json;[print(j[\"executable\"]) for j in map(json.loads,sys.stdin) if j.get(\"executable\")]" | tail -1)
out=$PWD/captures/first; rm -rf $out; mkdir -p $out
sup=$(mktemp -d); cp -R .render/support/ $sup/
for v in "Details:details"; do
  name=${v%%:*}; f=${v##*:}
  for th in dark light; do
    env HEX_APPLICATION_SUPPORT_DIR=$sup "$BIN" app -- --playground "Prototypes/Onboarding/First run" --variant "First screen · $name" --theme $th --size 1446x760 --video $out/$f-$th.mp4 --duration 3 --fps 30 --scale 2 >$out/$f-$th.log 2>&1 || echo "FAIL $f $th"
  done
done
ls -la $out
