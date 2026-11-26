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
// The dev builds' changelog (worker.ts, src/dev-notes.ts).
export const DEV_NOTES_PATH = `${BASE_PATH}/dev`
// The stable releases' changelog, built from docs/releases (src/changelog.ts).
export const CHANGELOG_PATH = `${BASE_PATH}/changelog`
// The Release workflow publishes the dev builds' notes to this one-commit
// branch (publish-dev-notes.sh), which the Worker serves at
// `${FILES_PATH}/dev-notes.json` and renders at `DEV_NOTES_PATH`, so neither
// the app nor the changelog waits for a site deploy.
export const DEV_NOTES_URL = "https://raw.githubusercontent.com/anomalyco/hex/dev-notes/dev-notes.json"
// Redirects to the current release's file for a target (worker.ts).
export const DOWNLOAD_PATH = `${BASE_PATH}/download`
// Short link to Hex for iPhone on the App Store (worker.ts).
export const IOS_PATH = withBase("/ios")
// Hex for Android page, with Hex Preview's download and setup (src/iphone.ts).
export const ANDROID_PATH = withBase("/android")
// Redirects to the newest Hex Preview APK, Hex for Android outside Google Play (worker.ts).
export const ANDROID_DOWNLOAD_PATH = `${DOWNLOAD_PATH}/android`
// android/scripts/preview-release.sh publishes each Hex Preview build's
// latest.json to this branch. The app's updater reads it, and so does the
// Worker, so the download link never waits for a site deploy.
export const ANDROID_PREVIEW_URL = "https://raw.githubusercontent.com/anomalyco/hex/android-preview/latest.json"

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
