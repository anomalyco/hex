import { bindings, defineConfig, triggers } from "cf/config"
import { BASE_PATH, hostForMode } from "./src/base.ts"

// `--mode dev` builds and deploys the dev Worker on dev.anoma.ly with the dev
// files bucket; any other mode is production on anoma.ly. One Worker serves the
// site at /hex and the bucket at /hex/files. The account comes from
// CLOUDFLARE_ACCOUNT_ID.
export default defineConfig(({ mode }) => {
  const dev = mode === "dev"
  const host = hostForMode(mode)
  return {
    worker: {
      name: dev ? "hex-dev-site" : "hex-production-site",
      compatibilityDate: "2026-10-13",
      entrypoint: "./worker.ts",
      assets: { runWorkerFirst: true },
      env: {
        ASSETS: bindings.assets(),
        FILES: bindings.r2({ name: dev ? "hex-dev-files" : "hex-production-files" }),
        CANONICAL_HOST: bindings.text(host),
      },
      triggers: [
        triggers.fetch({ pattern: `${host}${BASE_PATH}`, zone: "anoma.ly" }),
        triggers.fetch({ pattern: `${host}${BASE_PATH}/*`, zone: "anoma.ly" }),
      ],
      workersDev: false,
      previewUrls: false,
    },
  }
})
