set -e
BIN=$(cargo build -q --locked --bin hex --message-format=json | python3 -c "import sys,json;[print(j[\"executable\"]) for j in map(json.loads,sys.stdin) if j.get(\"executable\")]" | tail -1)
echo bin $BIN
mkdir -p captures/merge
for s in ${SCENES}; do
  "$BIN" app -- --snap "$PWD/captures/merge/$s.png" --scene "$s" --theme dark >/dev/null 2>&1 || echo "FAIL $s"
done
ls captures/merge | wc -l
