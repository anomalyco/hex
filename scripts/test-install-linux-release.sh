#!/bin/sh
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
for command in curl jq openssl python3 sha256sum xxd; do
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
mkdir -p "$server/releases"
private_key="$temporary/private.pem"
public_der="$temporary/public.der"
payload="$temporary/payload.json"
signature="$temporary/signature"
installer="$temporary/install-linux.sh"
version=9.8.7

openssl genpkey -algorithm Ed25519 -out "$private_key"
openssl pkey -in "$private_key" -pubout -outform DER -out "$public_der"
public_key=$(tail -c 32 "$public_der" | xxd -p -c 64)
sed \
  -e "s/^expected_public_key=.*/expected_public_key=$public_key/" \
  -e "s/--proto '=https'/--proto '=http,https'/g" \
  -e "s/--proto-redir '=https'/--proto-redir '=http,https'/g" \
  "$root/scripts/install-linux-release.sh" > "$installer"

unsigned="$temporary/hex"
cat > "$unsigned" <<EOF
#!/bin/sh
echo "voice-control $version"
EOF
chmod 755 "$unsigned"
sha256=$(sha256sum "$unsigned" | awk '{ print $1 }')
bytes=$(wc -c < "$unsigned" | tr -d ' ')
artifact="HEX-$version-$sha256-x86_64-linux"
cp "$unsigned" "$server/releases/$artifact"
jq -cn \
  --arg version "$version" \
  --arg artifact "$artifact" \
  --arg sha256 "$sha256" \
  --argjson bytes "$bytes" \
  '{schema_version:1,channel:"stable",target:"x86_64-unknown-linux-gnu",version:$version,artifact:$artifact,bytes:$bytes,sha256:$sha256}' \
  > "$payload"
openssl pkeyutl -sign -rawin -inkey "$private_key" \
  -in "$payload" -out "$signature"
jq -n \
  --arg payload "$(openssl base64 -A -in "$payload")" \
  --arg signature "$(openssl base64 -A -in "$signature")" \
  '{payload:$payload,signature:$signature}' > "$server/linux-update.json"

port=$((38000 + ($$ % 1000)))
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$server" \
  > "$temporary/server.log" 2>&1 &
server_pid=$!
attempt=0
until curl --fail --silent "http://127.0.0.1:$port/linux-update.json" >/dev/null; do
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
HOME="$home" \
  HEX_INSTALL_DIR="$install_dir" \
  HEX_APPLICATION_SUPPORT_DIR="$support_dir" \
  HEX_RELEASE_BASE_URL="http://127.0.0.1:$port" \
  sh "$installer"

test -L "$install_dir/hex"
test "$("$install_dir/hex" --version)" = "voice-control $version"
grep -Fq 'X-HEX-Managed=true' "$home/.local/share/applications/hex.desktop"
grep -Fq 'X-HEX-Managed=true' "$home/.config/autostart/HEX.desktop"

HOME="$home" \
  HEX_INSTALL_DIR="$install_dir" \
  HEX_APPLICATION_SUPPORT_DIR="$support_dir" \
  sh "$installer" uninstall
test ! -e "$install_dir/hex"
test ! -e "$support_dir/versions"
test -f "$support_dir/settings.json"

cp "$server/linux-update.json" "$temporary/feed.valid.json"
jq '.signature = "AAAA"' "$temporary/feed.valid.json" > "$server/linux-update.json"
if HOME="$home" \
  HEX_INSTALL_DIR="$install_dir" \
  HEX_APPLICATION_SUPPORT_DIR="$support_dir" \
  HEX_RELEASE_BASE_URL="http://127.0.0.1:$port" \
  sh "$installer" > "$temporary/signature.log" 2>&1
then
  echo "Installer accepted an invalid feed signature." >&2
  exit 1
fi
test ! -e "$install_dir/hex"
test ! -e "$support_dir/versions"
cp "$temporary/feed.valid.json" "$server/linux-update.json"

printf 'X' | dd of="$server/releases/$artifact" bs=1 seek=0 conv=notrunc \
  >/dev/null 2>&1
if HOME="$home" \
  HEX_INSTALL_DIR="$install_dir" \
  HEX_APPLICATION_SUPPORT_DIR="$support_dir" \
  HEX_RELEASE_BASE_URL="http://127.0.0.1:$port" \
  sh "$installer" > "$temporary/tampered.log" 2>&1
then
  echo "Installer accepted a same-size corrupted artifact." >&2
  exit 1
fi
test ! -e "$install_dir/hex"
test ! -e "$support_dir/versions"

HOME="$home" \
  HEX_INSTALL_DIR="$install_dir" \
  HEX_APPLICATION_SUPPORT_DIR="$support_dir" \
  HEX_INSTALL_ARTIFACT="$unsigned" \
  HEX_INSTALL_VERSION="$version" \
  sh "$installer"
test "$("$install_dir/hex" --version)" = "voice-control $version"
HOME="$home" \
  HEX_INSTALL_DIR="$install_dir" \
  HEX_APPLICATION_SUPPORT_DIR="$support_dir" \
  sh "$installer" uninstall

important="$temporary/important"
support_link="$temporary/support-link"
mkdir -p "$important/versions"
touch "$important/versions/preserve"
ln -s "$important" "$support_link"
if HOME="$home" \
  HEX_INSTALL_DIR="$install_dir" \
  HEX_APPLICATION_SUPPORT_DIR="$support_link" \
  sh "$installer" uninstall > "$temporary/symlink.log" 2>&1
then
  echo "Uninstaller accepted a managed support symlink." >&2
  exit 1
fi
test -f "$important/versions/preserve"

echo "Linux release installer smoke test passed."
