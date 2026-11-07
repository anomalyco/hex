set -e
BIN=$(cargo build -q --locked --bin hex --message-format=json | python3 -c "import sys,json;[print(j[\"executable\"]) for j in map(json.loads,sys.stdin) if j.get(\"executable\")]" | tail -1)
mkdir -p captures/sketch
"$BIN" app -- --snap "$PWD/captures/sketch/waiting-dark.png" --scene mac-onboarding-waiting --theme dark >/dev/null 2>&1 || echo FAIL
"$BIN" app -- --snap "$PWD/captures/sketch/waiting-light.png" --scene mac-onboarding-waiting --theme light >/dev/null 2>&1 || echo FAIL
for t in ${TIMES}; do
  HEX_SKETCH_SECS=$t "$BIN" app -- --snap "$PWD/captures/sketch/sketch-$t.png" --scene mac-onboarding-sketch --theme dark >/dev/null 2>&1 || echo "FAIL $t"
done
ls captures/sketch
