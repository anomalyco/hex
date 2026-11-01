// The dev channel's changelog at /hex/dev: what each dev build changed, from
// dev-notes.json (written by the Release workflow's dev-notes jobs; see
// scripts/release-dev-notes.sh). The worker renders it per request, so a new
// dev build shows without a site deploy.

export type DevNotes = {
  schema: 1
  builds: { version: string; commit: string; published_at: string; items: { title: string; url: string }[] }[]
}

const ITEM_URL = /^https:\/\/github\.com\/anomalyco\/hex\/(pull\/[0-9]+|commit\/[0-9a-f]{40})$/

function escape(text: string): string {
  return text.replace(/[&<>"']/g, (character) => `&#${character.charCodeAt(0)};`)
}

// Keeps only well-formed builds and items; the page never trusts the file's shape.
export function parseDevNotes(value: unknown): DevNotes {
  const builds: DevNotes["builds"] = []
  const raw = (value as { builds?: unknown })?.builds
  for (const build of Array.isArray(raw) ? raw : []) {
    if (typeof build?.version !== "string" || !/^0\.0\.0-dev-[0-9]+$/.test(build.version)) continue
    const items = (Array.isArray(build.items) ? build.items : []).filter(
      (item: unknown): item is { title: string; url: string } =>
        typeof (item as { title?: unknown })?.title === "string" &&
        typeof (item as { url?: unknown })?.url === "string" &&
        ITEM_URL.test((item as { url: string }).url),
    )
    builds.push({
      version: build.version,
      commit: typeof build.commit === "string" ? build.commit : "",
      published_at: typeof build.published_at === "string" ? build.published_at : "",
      items: items.map((item: { title: string; url: string }) => ({ title: item.title, url: item.url })),
    })
  }
  return { schema: 1, builds }
}

function date(iso: string): string {
  const parsed = new Date(iso)
  if (Number.isNaN(parsed.getTime())) return ""
  return parsed.toLocaleString("en-US", { timeZone: "America/New_York", dateStyle: "medium", timeStyle: "short" }) + " ET"
}

export function renderDevNotes(notes: DevNotes, downloadPath: string): string {
  const builds = notes.builds
    .map((build) => {
      const items = build.items.length
        ? `<ul>${build.items
            .map((item) => `<li><a href="${escape(item.url)}">${escape(item.title)}</a></li>`)
            .join("")}</ul>`
        : `<p class="quiet">Internal changes only.</p>`
      return `<section id="${escape(build.version)}"><h2><a href="#${escape(build.version)}">${escape(build.version)}</a></h2><p class="quiet">${escape(date(build.published_at))}</p>${items}</section>`
    })
    .join("")
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Hex dev builds</title>
<meta name="description" content="What changed in each dev build of Hex.">
<link rel="icon" href="/hex/favicon.svg" type="image/svg+xml">
<style>
body { margin: 0; background: #070707; color: #e8e8e8; font: 15px/1.55 Inter, ui-sans-serif, system-ui, sans-serif; }
main { max-width: 680px; margin: 0 auto; padding: 48px 20px 80px; }
h1 { font-size: 28px; margin: 0 0 6px; }
h2 { font-size: 17px; margin: 0; }
section { border-top: 1px solid #222; padding: 20px 0; }
ul { margin: 10px 0 0; padding-left: 20px; }
li { margin: 4px 0; }
a { color: #e8e8e8; text-decoration-color: #555; text-underline-offset: 3px; }
a:hover { text-decoration-color: #e8e8e8; }
h2 a { text-decoration: none; }
.quiet { color: #8a8a8a; margin: 2px 0 0; }
.intro { margin: 0 0 28px; color: #b0b0b0; }
</style>
</head>
<body>
<main>
<h1>Hex dev builds</h1>
<p class="intro">What changed in each dev build. Dev builds update often and can break; <a href="/hex/">stable</a> is the safe choice. <a href="${escape(downloadPath)}">Download the latest Mac dev build</a>, or switch Settings › Update channel to Dev.</p>
${builds || `<p class="quiet">No dev build notes yet.</p>`}
</main>
</body>
</html>
`
}
