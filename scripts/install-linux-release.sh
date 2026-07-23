#!/bin/sh
set -eu

base_url=${HEX_RELEASE_BASE_URL:-https://pub-089d681d41754031a4aefa7017d8c2fb.r2.dev}
install_dir=${HEX_INSTALL_DIR:-"$HOME/.local/bin"}
data_home=${XDG_DATA_HOME:-$HOME/.local/share}
config_home=${XDG_CONFIG_HOME:-$HOME/.config}
support_dir=${HEX_APPLICATION_SUPPORT_DIR:-"$data_home/voice-control"}
applications="$data_home/applications"
autostart="$config_home/autostart"
expected_public_key=bfad02e62208ff144b5c9d21c7e79c7c16c6904299a437d857303007cd4ff7d8
public_key_prefix=302a300506032b6570032100

require() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "HEX installation requires $1." >&2
    exit 1
  fi
}

fail() {
  echo "$1" >&2
  exit 1
}

safe_user_directory() {
  path=$1
  if [ -L "$path" ]; then
    fail "Refusing directory symlink: $path"
  fi
  mkdir -p "$path"
  if [ "$(stat -c %u "$path")" != "$(id -u)" ]; then
    fail "HEX requires a user-owned directory: $path"
  fi
  if find "$path" -maxdepth 0 -perm /022 -print -quit | grep -q .; then
    fail "HEX refuses a group- or world-writable directory: $path"
  fi
}

managed_directory() {
  path=$1
  if [ -L "$path" ]; then
    fail "Refusing managed symlink: $path"
  fi
  mkdir -p "$path"
  if [ "$(stat -c %u "$path")" != "$(id -u)" ]; then
    fail "HEX requires a user-owned directory: $path"
  fi
  chmod 700 "$path"
}

uninstall() {
  if [ -L "$support_dir" ]; then
    fail "Refusing managed symlink: $support_dir"
  fi
  if [ -e "$support_dir" ]; then
    managed_directory "$support_dir"
    managed_directory "$support_dir/updates"
    exec 9> "$support_dir/updates/update.lock"
    if ! flock -n 9; then
      fail "A HEX install or update is already in progress."
    fi
  fi
  binary="$install_dir/hex"
  expected="$support_dir/current/hex"
  if [ -L "$binary" ] && [ "$(readlink "$binary")" = "$expected" ]; then
    rm -f "$binary"
  fi
  for desktop in \
    "$applications/hex.desktop" \
    "$autostart/HEX.desktop"
  do
    if [ -f "$desktop" ] && grep -Fxq 'X-HEX-Managed=true' "$desktop"; then
      rm -f "$desktop"
    fi
  done
  rm -f "$support_dir/current" "$support_dir/.current.next"
  rm -rf "$support_dir/versions"
  echo "Removed HEX. Logs and settings remain in $support_dir."
}

if [ "$(uname -s)" != Linux ] || [ "$(uname -m)" != x86_64 ]; then
  echo "HEX currently supports x86_64 Linux." >&2
  exit 1
fi
if [ "$(id -u)" -eq 0 ]; then
  echo "Install HEX as your desktop user, not root." >&2
  exit 1
fi
umask 022
for command in find flock grep readlink stat; do
  require "$command"
done

case ${1:-install} in
  install) ;;
  uninstall)
    uninstall
    exit 0
    ;;
  *)
    echo "Usage: $0 [install|uninstall]" >&2
    exit 1
    ;;
esac

for command in sha256sum sort sync; do
  require "$command"
done

temporary=$(mktemp -d)
application_staged=
autostart_staged=
binary_staged=
current_staged=
version_staged=
cleanup() {
  rm -rf "$temporary"
  for path in \
    "$application_staged" \
    "$autostart_staged" \
    "$binary_staged" \
    "$current_staged" \
    "$version_staged"
  do
    if [ -n "$path" ]; then
      rm -f "$path"
    fi
  done
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
feed="$temporary/linux-update.json"
payload="$temporary/payload.json"
signature="$temporary/signature"
public_der="$temporary/public.der"
public_pem="$temporary/public.pem"
artifact_path=${HEX_INSTALL_ARTIFACT:-$temporary/hex}
version=${HEX_INSTALL_VERSION:-}

if [ -z "${HEX_INSTALL_ARTIFACT:-}" ]; then
  for command in curl jq openssl xxd; do
    require "$command"
  done
  curl_version=$(curl --version | awk 'NR == 1 { print $2 }')
  oldest=$(printf '%s\n%s\n' 8.4.0 "$curl_version" | sort -V | head -1)
  test "$oldest" = 8.4.0 || fail "HEX installation requires curl 8.4.0 or newer."
  curl --fail --location --proto '=https' --proto-redir '=https' \
    --silent --show-error --connect-timeout 10 --max-time 60 \
    --max-filesize 65536 "$base_url/linux-update.json" -o "$feed"
  printf '%s%s' "$public_key_prefix" "$expected_public_key" | xxd -r -p > "$public_der"
  openssl pkey -pubin -inform DER -in "$public_der" -out "$public_pem"
  jq -er .payload "$feed" | openssl base64 -d -A > "$payload"
  jq -er .signature "$feed" | openssl base64 -d -A > "$signature"
  openssl pkeyutl -verify -rawin -pubin -inkey "$public_pem" \
    -in "$payload" -sigfile "$signature" >/dev/null

  schema=$(jq -er .schema_version "$payload")
  channel=$(jq -er .channel "$payload")
  target=$(jq -er .target "$payload")
  version=$(jq -er .version "$payload")
  artifact=$(jq -er .artifact "$payload")
  bytes=$(jq -er .bytes "$payload")
  sha256=$(jq -er .sha256 "$payload")
  test "$schema" = 1 || fail "Unsupported Linux release schema."
  test "$channel" = stable || fail "Unsupported Linux release channel."
  test "$target" = x86_64-unknown-linux-gnu || fail "Linux release targets the wrong platform."
  printf '%s\n' "$bytes" | grep -Eq '^[1-9][0-9]*$' \
    || fail "Linux release has an invalid size."
  printf '%s\n' "$sha256" | grep -Eq '^[0-9a-f]{64}$' \
    || fail "Linux release has an invalid checksum."
  test "$bytes" -le 536870912 || fail "Linux release exceeds the 512 MiB limit."
  test "$artifact" = "HEX-$version-$sha256-x86_64-linux" \
    || fail "Linux release is not content-addressed."

  curl --fail --location --proto '=https' --proto-redir '=https' \
    --silent --show-error --connect-timeout 10 --max-time 600 \
    --max-filesize "$bytes" "$base_url/releases/$artifact" -o "$artifact_path"
  test "$(wc -c < "$artifact_path" | tr -d ' ')" = "$bytes" \
    || fail "Downloaded Linux release has the wrong size."
  test "$(sha256sum "$artifact_path" | awk '{ print $1 }')" = "$sha256" \
    || fail "Downloaded Linux release checksum does not match."
  chmod 755 "$artifact_path"
fi
printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' \
  || fail "Linux release has an invalid version."
test -x "$artifact_path" || fail "HEX executable is missing: $artifact_path"
if ! reported_version=$("$artifact_path" --version 2>/dev/null \
  | awk 'NF { value=$NF } END { print value }')
then
  fail "HEX could not run. Install the required Arch runtime libraries."
fi
test "$reported_version" = "$version" \
  || fail "Downloaded executable reports the wrong version."

managed_directory "$support_dir"
managed_directory "$support_dir/versions"
managed_directory "$support_dir/updates"
exec 9> "$support_dir/updates/update.lock"
if ! flock -n 9; then
  echo "A HEX install or update is already in progress." >&2
  exit 1
fi
current="$support_dir/current"
current_executable=
if [ -e "$current" ] || [ -L "$current" ]; then
  if [ ! -L "$current" ]; then
    fail "Refusing unmanaged current version path: $current"
  fi
  current_target=$(readlink "$current")
  printf '%s\n' "$current_target" \
    | grep -Eq '^versions/[0-9]+\.[0-9]+\.[0-9]+$' \
    || fail "Installed HEX has an invalid current version link."
  current_executable="$support_dir/$current_target/hex"
fi
if [ -n "$current_executable" ] && [ -x "$current_executable" ]; then
  current_version=$("$current_executable" --version \
    | awk 'NF { value=$NF } END { print value }')
  printf '%s\n' "$current_version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' \
    || fail "Installed HEX reports an invalid version."
  newest=$(printf '%s\n%s\n' "$current_version" "$version" | sort -V | tail -1)
  if [ "$newest" != "$version" ]; then
    fail "Refusing to downgrade HEX from $current_version to $version."
  fi
fi
version_dir="$support_dir/versions/$version"
managed_directory "$version_dir"

safe_user_directory "$install_dir"
safe_user_directory "$applications"
safe_user_directory "$autostart"
binary="$install_dir/hex"
if printf '%s' "$binary" | LC_ALL=C grep -Eq '["\\[:cntrl:]]'; then
  fail "HEX install path contains unsupported characters."
fi
expected_binary="$support_dir/current/hex"
if { [ -e "$binary" ] || [ -L "$binary" ]; } \
  && { [ ! -L "$binary" ] || [ "$(readlink "$binary")" != "$expected_binary" ]; }
then
  fail "Refusing to replace an unmanaged file: $binary"
fi
if [ -e "$support_dir/current" ] && [ ! -L "$support_dir/current" ]; then
  fail "Refusing to replace an unmanaged file: $support_dir/current"
fi
application_entry="$applications/hex.desktop"
autostart_entry="$autostart/HEX.desktop"
for desktop in "$application_entry" "$autostart_entry"; do
  if [ -L "$desktop" ]; then
    fail "Refusing to replace a desktop-entry symlink: $desktop"
  fi
  if [ -e "$desktop" ] && ! grep -Fxq 'X-HEX-Managed=true' "$desktop"; then
    fail "Refusing to replace an unmanaged desktop entry: $desktop"
  fi
done

application_staged=$(mktemp "$applications/.hex.desktop.XXXXXX")
autostart_staged=$(mktemp "$autostart/.HEX.desktop.XXXXXX")
umask 077
cat > "$application_staged" <<EOF
[Desktop Entry]
Type=Application
Name=HEX
Comment=Local voice dictation
Exec="$binary" app
Icon=audio-input-microphone
Terminal=false
Categories=Utility;Accessibility;
StartupNotify=true
X-HEX-Managed=true
EOF
cat > "$autostart_staged" <<EOF
[Desktop Entry]
Type=Application
Name=HEX
Comment=Local voice dictation
Exec="$binary" app --hidden
Icon=audio-input-microphone
Terminal=false
X-GNOME-Autostart-enabled=true
X-HEX-Managed=true
EOF
chmod 644 "$application_staged" "$autostart_staged"

version_staged="$version_dir/.hex.install.$$"
current_staged="$support_dir/.current.next"
binary_staged="$install_dir/.hex.next"
install -m755 "$artifact_path" "$version_staged"
sync -f "$version_staged"
mv -Tf "$version_staged" "$version_dir/hex"
version_staged=
sync -f "$version_dir"
rm -f "$current_staged" "$binary_staged"
ln -s "versions/$version" "$current_staged"
mv -Tf "$current_staged" "$support_dir/current"
current_staged=
sync -f "$support_dir"
ln -s "$support_dir/current/hex" "$binary_staged"
mv -Tf "$binary_staged" "$binary"
binary_staged=
sync -f "$install_dir"
mv -Tf "$application_staged" "$application_entry"
application_staged=
mv -Tf "$autostart_staged" "$autostart_entry"
autostart_staged=
sync -f "$applications"
sync -f "$autostart"
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$applications" >/dev/null 2>&1 || true
fi

printf 'Installed HEX %s to %s\n' "$version" "$binary"
printf 'Install a model: %s model install\n' "$binary"
printf 'Launch HEX: %s app\n' "$binary"
printf 'Uninstall: sh install-linux.sh uninstall\n'
