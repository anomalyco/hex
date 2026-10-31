# Legacy redirects (Kit's account)

One Worker, `hex-legacy-downloads`, takes over Kit's four old Hex hosts:

```ts
downloads.hex.kitlangton.dev/*, .com/*   302 → https://anoma.ly/hex/files<path><query>   (Hex ≤ 2.3.1 feeds, appcast, files)
hex.kitlangton.dev/*, .com/*             301 → https://anoma.ly/hex/
  /download                                  → https://anoma.ly/hex/download/macos-arm64-dmg
  /install-linux.sh, /install-windows.ps1    → https://anoma.ly/hex/files/<same name>
```

The downloads hosts get Worker routes, which run in front of the
`hex-releases` bucket's custom domains, so the bucket keeps its domains. The
website hosts become Custom Domains of this Worker, taken over from the
`hex-voice-site` Worker (a hostname has one Worker). Deploy from the
repository root with Kit's Wrangler login:

```sh
bunx wrangler@latest deploy --config packaging/legacy-downloads/wrangler.jsonc
```

Wrangler asks before moving a hostname from `hex-voice-site`; answer yes. If
it refuses instead (for example because `hex-voice-site` serves the hosts by
routes, not custom domains), remove `hex-voice-site`'s domains and routes in
the dashboard (Workers & Pages → hex-voice-site → Settings → Domains &
Routes) and deploy again. Nothing is deleted: the bucket and
`hex-voice-site` stay as they are.

Roll back with `bunx wrangler@latest delete --config
packaging/legacy-downloads/wrangler.jsonc`, then reattach `hex.kitlangton.com` and `.dev` to
`hex-voice-site` in the dashboard (the downloads hosts fall back to the
bucket by themselves) (the bytes there never
changed, so old installs see exactly what they did before).
