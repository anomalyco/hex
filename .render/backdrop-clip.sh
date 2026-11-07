set -e
BIN=$(cargo build -q --locked --bin hex --message-format=json | python3 -c "import sys,json;[print(j[\"executable\"]) for j in map(json.loads,sys.stdin) if j.get(\"executable\")]" | tail -1)
for b in field words horizon; do
  d=captures/backdrop/frames-$b; rm -rf $d; mkdir -p $d
  for i in $(seq 0 89); do
    secs=$(python3 -c "print(1.0+$i/30)")
    HEX_BACKDROP=$b HEX_BACKDROP_SECS=$secs "$BIN" app -- --snap "$PWD/$d/$(printf %03d $i).png" --scene mac-onboarding --theme dark >/dev/null 2>&1 &
    if (( i % 8 == 7 )); then wait; fi
  done
  wait
  ffmpeg -loglevel error -y -framerate 30 -i $d/%03d.png -vf "scale=980:-2" -c:v libx264 -pix_fmt yuv420p -crf 20 captures/backdrop/$b-dark.mp4
  rm -rf $d
done
ls -la captures/backdrop/*.mp4
