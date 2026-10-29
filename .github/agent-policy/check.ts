// Deterministic policy check for agent-written Hex patches. No AI.
//
//   bun check.ts <patch-file>      (or the diff on stdin)
//
// Prints {"verdict": "clean" | "flagged" | "dax", "findings": [...]} and exits 0, 1 or 2.
// "flagged" patches need the strict reviewer; "dax" patches need Dax and are never auto-merged.

type Finding = { tier: "flagged" | "dax"; rule: string; file: string; line?: number }

const DAX_PATHS: [RegExp, string][] = [
  [/^\.github\//, "CI, release or ownership config"],
  [/(^|\/)CODEOWNERS$/, "code ownership"],
  [/^scripts\/(release|publish|notarize|macos-signing|validate-app|check-ca-roots)/, "release or signing script"],
  [/^src\/updater(\.rs|\/)/, "updater trust"],
  [/^app\/update-requirement\.txt$/, "updater trust"],
  [/\.entitlements$/, "signing entitlements"],
]

const FLAGGED_PATHS: [RegExp, string][] = [
  [/(^|\/)build\.rs$/, "build script"],
  [/^\.cargo\//, "cargo config"],
  [/(^|\/)Cargo\.toml$/, "Cargo manifest (dependencies)"],
  [/(^|\/)Cargo\.lock$/, "Cargo.lock"],
  [/^packaging\//, "packaging"],
  [/^scripts\/(install|build|package|setup|prepare)/, "installer or build script"],
  [/(^|\/)Info\.plist$|\.plist$/, "plist"],
  [/^nix\/|^flake\.(nix|lock)$/, "nix packaging"],
  [/(^|\/)(package\.json|bun\.lockb?|package-lock\.json)$/, "JavaScript dependencies"],
  [/^\.cloudflare\/|cloudflare\.config\.ts$|^site\/worker/, "deployment config"],
  [/(^|\/)(AGENTS|CLAUDE)\.md$|(^|\/)\.opencode\/|(^|\/)opencode\.jsonc?$/, "agent instructions or config"],
  [/^\.config\//, "tool config"],
]

const LINE_RULES: [RegExp, string][] = [
  [/\bhttps?:\/\/|\bwss?:\/\//i, "new URL"],
  [/["'`][^"'`\s]*\b[a-z0-9-]+\.(com|net|org|io|dev|ai|ly|xyz|app|co|sh|me|cloud|rs|gg|so|to|tk|ru|cn)\b[^"'`]*["'`]/i, "new host string"],
  [/\bCommand::new\b|\bstd::process\b|\bprocess::Command\b|\bosascript\b|\bNSTask\b|\bShellExecute|\bCreateProcess|\bposix_spawn|\bexecv|\bBun\.spawn|\bchild_process\b|["'`](\/bin\/)?(ba|z)?sh["'`]\s*,\s*["'`]-c/, "process spawning"],
  [/\breqwest\b|\bureq\b|\bhyper\b|\bTcpStream\b|\bTcpListener\b|\bUdpSocket\b|\btokio::net\b|\bstd::net\b|\bcurl\b|\bwget\b|\bWebSocket\b|\bURLSession\b|\bNSURL|\bWinHttp|\bfetch\(/, "network API"],
  [/\bunsafe\b/, "unsafe code"],
  [/[A-Za-z0-9+/]{100,}={0,2}|\b[0-9a-fA-F]{64,}\b|(\\x[0-9a-fA-F]{2}){16,}|(0x[0-9a-fA-F]{2}\s*,\s*){32,}/, "encoded blob"],
  [/[\u200B-\u200F\u202A-\u202E\u2060-\u2064\u2066-\u2069\uFEFF\u00AD]/, "bidi or zero-width character"],
  [/\b(env::var|getenv|process\.env)\b.*\b(TOKEN|SECRET|KEY|PASSWORD|CREDENTIAL)/i, "secret environment read"],
]

const MAX_CHANGED_LINES = 600
const MAX_FILES = 25

export function check(diff: string) {
  const findings: Finding[] = []
  const add = (f: Finding) => {
    if (!findings.some((x) => x.rule === f.rule && x.file === f.file)) findings.push(f)
  }
  let file = ""
  let newLine = 0
  let changed = 0
  const files = new Set<string>()

  const classify = (path: string) => {
    if (!path || path === "/dev/null") return
    files.add(path)
    for (const [re, rule] of DAX_PATHS) if (re.test(path)) add({ tier: "dax", rule, file: path })
    for (const [re, rule] of FLAGGED_PATHS) if (re.test(path)) add({ tier: "flagged", rule, file: path })
  }

  for (const raw of diff.split("\n")) {
    if (raw.startsWith("diff --git ")) {
      const m = raw.match(/^diff --git a\/(.+) b\/(.+)$/)
      file = m?.[2] ?? raw
      classify(m?.[1] ?? "")
      classify(file)
      continue
    }
    if (raw.startsWith("rename from ")) classify(raw.slice(12))
    if (raw.startsWith("rename to ")) classify((file = raw.slice(10)))
    if (/^(new|deleted) file mode 120000|^new mode 120000/.test(raw)) add({ tier: "flagged", rule: "symlink", file })
    if (/^(new file mode|new mode) 100755/.test(raw)) add({ tier: "flagged", rule: "executable file", file })
    if (/^Binary files |^GIT binary patch/.test(raw)) add({ tier: "flagged", rule: "binary file", file })
    if (raw.startsWith("+++ ") || raw.startsWith("--- ")) {
      const p = raw.slice(4).replace(/^[ab]\//, "")
      if (p !== "/dev/null") classify(p)
      continue
    }
    const hunk = raw.match(/^@@ -\d+(?:,\d+)? \+(\d+)/)
    if (hunk) {
      newLine = Number(hunk[1])
      continue
    }
    if (raw.startsWith("+")) {
      changed++
      const text = raw.slice(1)
      for (const [re, rule] of LINE_RULES) if (re.test(text)) add({ tier: "flagged", rule, file, line: newLine })
      if (/(^|\/)Cargo\.toml$/.test(file) && /^\s*[A-Za-z0-9_-]+\s*=\s*["{]/.test(text))
        add({ tier: "flagged", rule: "dependency added or changed", file, line: newLine })
      if (/(^|\/)Cargo\.lock$/.test(file) && /^name = /.test(text)) add({ tier: "flagged", rule: "new crate in Cargo.lock", file, line: newLine })
      newLine++
    } else if (raw.startsWith("-")) {
      changed++
    } else if (raw.startsWith(" ")) {
      newLine++
    }
    if (/[\u202A-\u202E\u2066-\u2069]/.test(raw)) add({ tier: "flagged", rule: "bidi or zero-width character", file })
  }

  if (changed > MAX_CHANGED_LINES) add({ tier: "flagged", rule: `size: ${changed} changed lines > ${MAX_CHANGED_LINES}`, file: "*" })
  if (files.size > MAX_FILES) add({ tier: "flagged", rule: `size: ${files.size} files > ${MAX_FILES}`, file: "*" })
  if (!files.size) add({ tier: "flagged", rule: "empty or unparseable patch", file: "*" })

  const verdict = findings.some((f) => f.tier === "dax") ? "dax" : findings.length ? "flagged" : "clean"
  return { verdict, changed_lines: changed, files: [...files].sort(), findings } as const
}

if (import.meta.main) {
  const path = process.argv[2]
  const diff = path ? await Bun.file(path).text() : await new Response(Bun.stdin.stream()).text()
  const result = check(diff)
  console.log(JSON.stringify(result, null, 2))
  process.exit(result.verdict === "clean" ? 0 : result.verdict === "flagged" ? 1 : 2)
}
