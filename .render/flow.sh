set -e
BIN=$(cargo build -q --locked --bin hex --message-format=json | python3 -c "import sys,json;[print(j[\"executable\"]) for j in map(json.loads,sys.stdin) if j.get(\"executable\")]" | tail -1)
out=$PWD/captures/flow; rm -rf $out; mkdir -p $out
sup=$(mktemp -d); cp -R .render/support/ $sup/
env HEX_APPLICATION_SUPPORT_DIR=$sup HEX_ONBOARDING_AUDIO_OUT=$out/flow.wav HEX_ONBOARDING_AUDIO_SECS=99 HEX_ONBOARDING_AUDIO_GAIN=0.8 \
  "$BIN" app -- --playground "Prototypes/Onboarding/First run" --variant "Whole run, slow download, a sketch" --theme dark --size 1446x760 --video $out/flow-silent.mp4 --duration 99 --fps 60 --scale 2 >$out/flow.log 2>&1
echo "exit $?"
ffmpeg -loglevel error -y -i $out/flow-silent.mp4 -i $out/flow.wav -c:v copy -c:a aac -b:a 160k -shortest $out/flow-dark.mp4 || echo "mux failed"
ls -la $out
