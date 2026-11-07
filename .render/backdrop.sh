set -e
BIN=$(cargo build -q --locked --bin hex --message-format=json | python3 -c "import sys,json;[print(j[\"executable\"]) for j in map(json.loads,sys.stdin) if j.get(\"executable\")]" | tail -1)
mkdir -p captures/backdrop
for b in none field words horizon; do
  for th in dark light; do
    HEX_BACKDROP=$b HEX_BACKDROP_SECS=1.3 "$BIN" app -- --snap "$PWD/captures/backdrop/$b-$th.png" --scene mac-onboarding --theme $th >/dev/null 2>&1 || echo "FAIL $b $th"
  done
done
ls captures/backdrop
