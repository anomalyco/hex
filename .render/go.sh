#!/bin/zsh
# Renders the unified first run, played by its own script, into captures/render.
here=${0:a:h}
out=$here/../captures/render
mkdir -p $out
sup=$(mktemp -d); cp -R $here/support/ $sup/
env HEX_APPLICATION_SUPPORT_DIR=$sup HEX_ONBOARDING_AUDIO_OUT=$out/unified.wav HEX_ONBOARDING_AUDIO_SECS=66 HEX_ONBOARDING_AUDIO_GAIN=0.8 \
  $here/hex app -- --playground "Prototypes/Onboarding/First run" --variant "Whole run, scripted game" --theme dark --video $out/unified-silent.mp4 --duration 66 --fps 60 --scale 2 >$out/unified.log 2>&1
echo "unified exit $?"
ls -la $out
