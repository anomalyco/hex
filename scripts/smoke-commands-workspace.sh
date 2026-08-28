#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/hex-commands.XXXXXX")
trap 'rm -rf "$temporary"' EXIT

(cd "$root/sdk/commands" && bun run build)

for fixture in fresh 4.0.0-beta.97 4.0.0-beta.107; do
  workspace="$temporary/$fixture"
  mkdir "$workspace"
  cp -R "$root/sdk/commands/workspace-template/." "$workspace/"
  mkdir "$workspace/.hex-sdk"
  cp "$root/sdk/commands/package.json" "$workspace/.hex-sdk/"
  cp -R "$root/sdk/commands/dist" "$workspace/.hex-sdk/dist"

  if [ "$fixture" != fresh ]; then
    # Rust tests cover reconciliation. Exercise Bun upgrading an existing lockfile
    # and node_modules here, rather than only installing into an empty workspace.
    (cd "$workspace" && bun -e '
      const manifest = await Bun.file("package.json").json()
      const sdk = await Bun.file(".hex-sdk/package.json").json()
      manifest.dependencies.effect = sdk.peerDependencies.effect = process.argv[1]
      manifest.scripts.custom = "echo preserved"
      manifest.userMetadata = { keep: true }
      await Bun.write("package.json", JSON.stringify(manifest, null, 2))
      await Bun.write(".hex-sdk/package.json", JSON.stringify(sdk, null, 2))
    ' "$fixture" && bun install)
    cp "$workspace/hex.config.ts" "$workspace/config-before.ts"
    cp "$root/sdk/commands/package.json" "$workspace/.hex-sdk/package.json"
    rm -rf "$workspace/node_modules/@hex/commands"
    (cd "$workspace" && bun -e '
      const manifest = await Bun.file("package.json").json()
      const sdk = await Bun.file(".hex-sdk/package.json").json()
      manifest.dependencies.effect = sdk.peerDependencies.effect
      await Bun.write("package.json", JSON.stringify(manifest, null, 2))
    ')
  fi

  (cd "$workspace" && bun install && bun run check && bun -e '
    import assert from "node:assert/strict"
    import { Effect } from "effect"
    import { ToolCallError } from "@hex/commands/effect"
    import { evaluateConfig, runHost } from "./node_modules/@hex/commands/dist/host.js"
    import sdk from "@hex/commands/package.json" with { type: "json" }
    import effect from "effect/package.json" with { type: "json" }

    const manifest = await Bun.file("package.json").json()
    assert.equal(effect.version, sdk.peerDependencies.effect)
    assert.equal(manifest.dependencies.effect, sdk.peerDependencies.effect)
    assert.equal(new ToolCallError({ message: "smoke" })._tag, "Hex.ToolCallError")
    assert.equal(await Effect.runPromise(Effect.succeed("ready")), "ready")
    const frames = []
    await runHost({
      config: await evaluateConfig(`${process.cwd()}/hex.config.ts`),
      input: (async function* () { yield "{\"type\":\"shutdown\"}\n" })(),
      write: (frame) => { frames.push(frame) },
    })
    assert.equal(frames[0]?.type, "registration")
    if (await Bun.file("config-before.ts").exists()) {
      assert.equal(await Bun.file("hex.config.ts").text(), await Bun.file("config-before.ts").text())
      assert.equal(manifest.scripts.custom, "echo preserved")
      assert.deepEqual(manifest.userMetadata, { keep: true })
    }
  ')
  echo "$fixture workspace passed"
done
