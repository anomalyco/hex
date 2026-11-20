#!/bin/sh
# Run in the Android project on the Studio. The debug APK is already installed.
set -eu
: "${ANDROID_HOME:?}" "${JAVA_HOME:?}" "${T:?Studio phase scratch}" "${ANDROID_SERIAL:?}"
cd "$(dirname "$0")/../.."
lib="$ANDROID_HOME/platforms/android-35/android.jar"
bt="$ANDROID_HOME/build-tools/35.0.0"
work="$T/replay"; dev=/data/local/tmp/hex3
mkdir -p "$work/classes"; adb shell mkdir -p "$dev"
"$JAVA_HOME/bin/javac" --release 8 -cp "$lib:app/build/tmp/kotlin-classes/debug" -d "$work/classes" benchmark/typing/Replay.java
"$JAVA_HOME/bin/jar" cf "$work/replay-classes.jar" -C "$work/classes" .
"$bt/d8" --min-api 31 --lib "$lib" --output "$work/replay-dex.jar" "$work/replay-classes.jar"
adb push "$work/replay-dex.jar" "$dev/replay.jar" >/dev/null
adb push app/src/main/assets/typing/en.hextm "$dev/en.hextm" >/dev/null
adb push "$T/synthetic.jsonl" "$dev/synthetic.jsonl" >/dev/null
adb shell chmod 444 "$dev/replay.jar"
apk=$(adb shell pm path ly.anoma.hex | tr -d '\r' | sed 's/^package://')
adb shell "CLASSPATH=$dev/replay.jar:$apk app_process /system/bin local.hex.bench.Replay $dev/en.hextm $dev/synthetic.jsonl"
