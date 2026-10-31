// Kit's old Hex hosts, all redirected to anoma.ly:
// - downloads.hex.kitlangton.dev and .com (the bucket Hex 2.3.1 and earlier
//   read): every path, 302, to https://anoma.ly/hex/files, where
//   hex-production-files holds byte copies of everything they served and the
//   Legacy appcast workflow publishes.
// - hex.kitlangton.com and .dev (the old website): 301 to https://anoma.ly/hex,
//   with the download and installer paths kept working.
const WEBSITE = {
  "/download": "/hex/download/macos-arm64-dmg",
  "/install-linux.sh": "/hex/files/install-linux.sh",
  "/install-windows.ps1": "/hex/files/install-windows.ps1",
}

export default {
  fetch(request) {
    const { hostname, pathname, search } = new URL(request.url)
    if (hostname.startsWith("downloads.")) {
      return Response.redirect(`https://anoma.ly/hex/files${pathname}${search}`, 302)
    }
    const path = WEBSITE[pathname.replace(/\/+$/, "")] ?? "/hex/"
    return Response.redirect(`https://anoma.ly${path}`, 301)
  },
}
