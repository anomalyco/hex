# Security

Report vulnerabilities through
[GitHub private vulnerability reporting](https://github.com/anomalyco/hex/security/advisories/new).
Do not include secrets, private transcripts, browsing history, or unredacted HEX
diagnostic logs in a public issue.

Custom command handlers and npm dependencies execute locally with the user's
normal permissions. They are supervised for lifecycle and bounded IPC, but they
are not sandboxed.
