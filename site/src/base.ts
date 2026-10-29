// The site lives under /hex on anoma.ly (production) and dev.anoma.ly (dev), and
// the release files under /hex/files. Pages keep root-relative paths internally;
// these helpers add and remove the prefix at the edges.
export const BASE_PATH = "/hex"
export const FILES_PATH = `${BASE_PATH}/files`
// The Anomaly release service. Its dev stage (dev.anoma.ly/release) is only for
// testing the service itself, so both site environments read production.
export const RELEASE_SERVICE = "https://anoma.ly/release"
// Redirects to the latest dev channel Mac download (worker.ts).
export const DEV_DOWNLOAD_PATH = `${BASE_PATH}/download/dev`

// The build mode (`--mode dev`) picks the environment; every other mode builds production.
export function hostForMode(mode: string | undefined): string {
  return mode === "dev" ? "dev.anoma.ly" : "anoma.ly"
}

export function originForMode(mode: string | undefined): string {
  return `https://${hostForMode(mode)}${BASE_PATH}`
}

export function withBase(path: string): string {
  return `${BASE_PATH}${path}`
}

// Returns the site path for a request path, or undefined when it is outside the base.
export function stripBase(pathname: string): string | undefined {
  if (pathname === BASE_PATH) return "/"
  if (!pathname.startsWith(`${BASE_PATH}/`)) return undefined
  return pathname.slice(BASE_PATH.length)
}
