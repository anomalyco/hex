#!/bin/sh
# Runs install-linux-release.sh against a fixture release service on
# loopback: install, receipt, uninstall, refused manifests and downloads,
# source installs, and symlink safety. Never touches the real user's systemd
# manager or data.
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
for command in curl jq python3 sha256sum; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "Installer test requires $command." >&2
    exit 1
  fi
done
if [ "$(uname -s)" != Linux ] || [ "$(uname -m)" != x86_64 ]; then
  echo "Installer test requires x86_64 Linux." >&2
  exit 1
fi
if [ "$(id -u)" -eq 0 ]; then
  echo "Installer test must run as an unprivileged user." >&2
  exit 1
fi

temporary=$(mktemp -d)
mkdir -p "$temporary/bin"
printf '#!/bin/sh\nexit 0\n' > "$temporary/bin/systemctl"
chmod 755 "$temporary/bin/systemctl"
export PATH="$temporary/bin:$PATH"
server_pid=
cleanup() {
  if [ -n "$server_pid" ]; then
    kill "$server_pid" >/dev/null 2>&1 || true
    wait "$server_pid" 2>/dev/null || true
  fi
  rm -rf "$temporary"
}
trap cleanup EXIT HUP INT TERM

server="$temporary/server"
installer="$temporary/install-linux.sh"
version=9.8.7
# The fixture service is plain HTTP on loopback.
sed \
  -e "s/--proto '=https'/--proto '=http,https'/g" \
  -e "s/--proto-redir '=https'/--proto-redir '=http,https'/g" \
  -e 's|^    https://\*) ;;$|    http://127.0.0.1:*\|https://*) ;;|' \
  "$root/scripts/install-linux-release.sh" > "$installer"

unsigned="$temporary/hex"
printf '#!/bin/sh\necho "voice-control %s"\n' "$version" > "$unsigned"
chmod 755 "$unsigned"
sha256=$(sha256sum "$unsigned" | awk '{ print $1 }')
bytes=$(wc -c < "$unsigned" | tr -d ' ')
file="hex-$version-linux-x64"
mkdir -p "$server/release/hex/desktop" "$server/release/hex/files/$sha256"
cp "$unsigned" "$server/release/hex/files/$sha256/$file"
jq -n --arg version "$version" --arg url "/release/hex/files/$sha256/$file" --arg sha256 "$sha256" --argjson size "$bytes" \
  '{schema: 1, app: "hex", artifact: "desktop", channel: "stable", sequence: 1,
    published_at: "2026-10-24T00:00:00Z",
    release: {version: $version, rollout_percent: 100, revoked_versions: [],
      targets: {"linux-x64": {url: $url, size: $size, sha256: $sha256}},
      distributions: [{kind: "direct"}]}}' > "$server/release/hex/desktop/stable.json"
cp "$server/release/hex/desktop/stable.json" "$temporary/manifest.json"

port=$((38000 + ($$ % 1000)))
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$server" \
  > "$temporary/server.log" 2>&1 &
server_pid=$!
attempt=0
until curl --fail --silent "http://127.0.0.1:$port/release/hex/desktop/stable.json" >/dev/null; do
  attempt=$((attempt + 1))
  if [ "$attempt" -ge 20 ]; then
    echo "Fixture server did not start." >&2
    exit 1
  fi
  sleep 0.1
done

home="$temporary/home"
export XDG_DATA_HOME="$home/.local/share"
export XDG_CONFIG_HOME="$home/.config"
install_dir="$home/.local/bin"
support_dir="$home/.local/share/voice-control"
mkdir -p "$support_dir"
printf 'preserve me\n' > "$support_dir/settings.json"

install() {
  HOME="$home" HEX_INSTALL_DIR="$install_dir" HEX_APPLICATION_SUPPORT_DIR="$support_dir" \
    HEX_RELEASE_BASE_URL="http://127.0.0.1:$port/release" sh "$installer" "$@"
}

# Refuses a manifest, then checks that nothing was installed.
refuses() {
  if install > "$temporary/refused.log" 2>&1; then
    echo "Installer accepted $1." >&2
    exit 1
  fi
  test ! -e "$install_dir/hex"
  test ! -e "$support_dir/versions"
  cp "$temporary/manifest.json" "$server/release/hex/desktop/stable.json"
}

install
test -L "$install_dir/hex"
test "$("$install_dir/hex" --version)" = "voice-control $version"
grep -Fq 'X-HEX-Managed=true' "$home/.local/share/applications/hex.desktop"
grep -Fq 'X-HEX-Managed=true' "$home/.config/autostart/HEX.desktop"
grep -Fq '" start' "$home/.config/autostart/HEX.desktop"
grep -Fq '# X-HEX-Managed=true' "$home/.config/systemd/user/hex.service"
grep -Fq '" service' "$home/.config/systemd/user/hex.service"
grep -Fxq 'Type=notify' "$home/.config/systemd/user/hex.service"
grep -Fxq 'NotifyAccess=main' "$home/.config/systemd/user/hex.service"
grep -Fxq "Exec=\"$install_dir/hex\"" "$home/.local/share/applications/hex.desktop"
grep -Fxq "Exec=\"$install_dir/hex\" quit" "$home/.local/share/applications/hex.desktop"
grep -Fxq "EnvironmentFile=-$support_dir/session.env" "$home/.config/systemd/user/hex.service"
test ! -e "$support_dir/versions/$version/source-build"
test "$(jq -c . "$support_dir/install.json")" = \
  "$(jq -cn --arg root "$support_dir" '{app: "hex", artifact: "desktop", channel: "stable", method: "direct", root: $root}')"

install uninstall
test ! -e "$install_dir/hex"
test ! -e "$support_dir/versions"
test ! -e "$support_dir/install.json"
test ! -e "$home/.config/systemd/user/hex.service"
test -f "$support_dir/settings.json"

# The dev channel: its manifest, its receipt.
jq '.channel = "dev"' "$temporary/manifest.json" > "$server/release/hex/desktop/dev.json"
HOME="$home" HEX_INSTALL_DIR="$install_dir" HEX_APPLICATION_SUPPORT_DIR="$support_dir" \
  HEX_RELEASE_BASE_URL="http://127.0.0.1:$port/release" HEX_INSTALL_CHANNEL=dev sh "$installer" > /dev/null
test "$(jq -r .channel "$support_dir/install.json")" = dev
install uninstall

jq '.app = "other"' "$temporary/manifest.json" > "$server/release/hex/desktop/stable.json"
refuses "another app's manifest"
jq '.release.targets = {}' "$temporary/manifest.json" > "$server/release/hex/desktop/stable.json"
refuses "a release without a linux-x64 build"
jq '.release.targets["linux-x64"].url = "https://example.com/hex"' "$temporary/manifest.json" > "$server/release/hex/desktop/stable.json"
refuses "a file outside the service"
printf 'X' | dd of="$server/release/hex/files/$sha256/$file" bs=1 seek=0 conv=notrunc >/dev/null 2>&1
refuses "a same-size corrupted download"

HOME="$home" HEX_INSTALL_DIR="$install_dir" HEX_APPLICATION_SUPPORT_DIR="$support_dir" \
  HEX_INSTALL_ARTIFACT="$unsigned" HEX_INSTALL_VERSION="$version" HEX_INSTALL_SOURCE=1 \
  sh "$installer"
test "$("$install_dir/hex" --version)" = "voice-control $version"
test -e "$support_dir/versions/$version/source-build"
test ! -e "$support_dir/install.json"
install uninstall

important="$temporary/important"
support_link="$temporary/support-link"
mkdir -p "$important/versions"
touch "$important/versions/preserve"
ln -s "$important" "$support_link"
if HOME="$home" HEX_INSTALL_DIR="$install_dir" HEX_APPLICATION_SUPPORT_DIR="$support_link" \
  sh "$installer" uninstall > "$temporary/symlink.log" 2>&1
then
  echo "Uninstaller accepted a managed support symlink." >&2
  exit 1
fi
test -f "$important/versions/preserve"

echo "Linux release installer smoke test passed."
