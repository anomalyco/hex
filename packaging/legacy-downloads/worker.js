// downloads.hex.kitlangton.dev and .com: every path to https://anoma.ly/hex/files,
// so Hex 2.3.1 and earlier read the appcast and feeds the Legacy appcast
// workflow publishes there. hex-production-files holds byte copies of
// everything this host served, so the redirect alone changes nothing they get.
export default {
  fetch(request) {
    const { pathname, search } = new URL(request.url)
    return Response.redirect(`https://anoma.ly/hex/files${pathname}${search}`, 302)
  },
}
