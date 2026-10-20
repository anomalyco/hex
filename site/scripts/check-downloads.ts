// Fails unless every download link on the site serves a file.
import { DOWNLOAD_URLS } from "../src/release"

const failures: string[] = []
await Promise.all(
  DOWNLOAD_URLS.map(async (url) => {
    const response = await fetch(url, { method: "HEAD", redirect: "follow" })
    console.log(`${response.status} ${url}`)
    if (!response.ok) failures.push(`${response.status} ${url}`)
  }),
)
if (failures.length > 0) {
  console.error(`Broken download links:\n${failures.join("\n")}`)
  process.exit(1)
}
