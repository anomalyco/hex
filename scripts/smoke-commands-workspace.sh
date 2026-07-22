#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/hex-commands.XXXXXX")
trap 'rm -rf "$temporary"' EXIT

(cd "$root/sdk/commands" && bun run build)
cp -R "$root/sdk/commands/workspace-template/." "$temporary/"
mkdir "$temporary/.hex-sdk"
cp "$root/sdk/commands/package.json" "$temporary/.hex-sdk/"
cp -R "$root/sdk/commands/dist" "$temporary/.hex-sdk/dist"
(cd "$temporary" && bun install && bun run check)

resolved=$(cd "$temporary" && bun -e \
  'import sdk from "@hex/commands/package.json" with { type: "json" }; import effect from "effect/package.json" with { type: "json" }; console.log(`${sdk.version}:${effect.version}`)')
test "$resolved" = "0.0.0:4.0.0-beta.97"
echo "$temporary workspace passed"
