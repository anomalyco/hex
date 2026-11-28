// Hex MCP at /hex/mcp: a static page for the local, read-only MCP server
// (`hex mcp`, src/meeting/mcp.rs), with where `hex` lives on each OS and the
// setup each client needs. The tool names here are the ones the server
// advertises. Every word is in the HTML a crawler or agent fetches: the client
// tabs are radio buttons and CSS, and the page's one script only shows the
// copy buttons and makes them work.
import { withBase } from "./base"
import { escape, shell, type Meta } from "./labs"

export const MCP_PATH = withBase("/mcp")
export const MCP_CARD_PATH = withBase("/mcp.png")

const DESCRIPTION =
  "hex mcp is Hex's local, read-only MCP server. It gives OpenCode, Claude Code, Claude Desktop, and Cursor your recorded meeting transcripts, chapters, and notes. No cloud, no account, no API key."

const MONO = "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace"

const STYLE = `
.mcp { --line: #1d1e21; --edge: rgba(242, 242, 239, 0.1); --rule: rgba(242, 242, 239, 0.07); }
.mcp a { text-decoration-color: #4a4b50; text-underline-offset: 3px; }
.mcp a:hover { text-decoration-color: currentColor; }
.mcp code { font-size: 13px; }
.mcp .lede { margin: 18px 0 0; max-width: 31em; color: #b4b5ba; font-size: 20px; line-height: 1.45; text-wrap: pretty; }
.mcp h2 { margin: 64px 0 0; font-size: 20px; font-weight: 600; letter-spacing: -0.015em; line-height: 1.2; }
.mcp .sub { margin: 8px 0 0; color: #8a8b90; font-size: 15px; }
.mcp .note { margin: 14px 0 0; max-width: none; color: #8a8b90; font-size: 14px; }
.mcp .rows { margin: 20px 0 0; border-bottom: 1px solid var(--line); }
.mcp .row { display: grid; grid-template-columns: 7rem minmax(0, 1fr); align-items: center; gap: 0 16px; padding: 14px 0; border-top: 1px solid var(--line); color: #b4b5ba; font-size: 15px; }
.mcp .row--copy { grid-template-columns: 7rem minmax(0, 1fr) 36px; }
.mcp .row__label { color: #f2f2ef; font-weight: 500; }
.mcp .row__value { display: grid; min-width: 0; gap: 4px; }
.mcp .row__value small { color: #8a8b90; font-size: 14px; }
.mcp .cmd { display: block; overflow-x: auto; padding: 0; border: 0; background: none; color: #d6d7db; font: 13px/1.6 ${MONO}; white-space: pre; scrollbar-width: none; }
.mcp .copy { display: inline-grid; width: 36px; height: 36px; place-items: center; padding: 0; border: 1px solid rgba(242, 242, 239, 0.14); border-radius: 9px; color: #f2f2ef; background: transparent; cursor: pointer; transition: background-color 160ms ease, border-color 160ms ease; }
.mcp .copy[hidden] { display: none; }
.mcp .copy:hover { border-color: rgba(242, 242, 239, 0.3); background: rgba(242, 242, 239, 0.06); }
.mcp .copy:focus-visible, .mcp a:focus-visible { outline: 2px solid #a9b9ff; outline-offset: 3px; }
.mcp .copy svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
.mcp .copy svg + svg, .mcp .copy[data-copied] svg:first-child { display: none; }
.mcp .copy[data-copied] svg + svg { display: block; }
.clients { position: relative; margin: 20px 0 0; }
.clients > input { position: absolute; width: 1px; height: 1px; opacity: 0; pointer-events: none; }
.clients__tabs { display: inline-grid; max-width: 100%; grid-auto-columns: 1fr; grid-auto-flow: column; gap: 4px; padding: 4px; border: 1px solid var(--edge); border-radius: 13px; }
.clients__tabs label { padding: 7px 14px; border-radius: 9px; color: #8a8b90; font-size: 14px; font-weight: 500; text-align: center; white-space: nowrap; cursor: pointer; transition: color 160ms ease, background-color 160ms ease; }
.clients__tabs label:hover { color: #f2f2ef; }
.clients > input:nth-of-type(1):checked ~ .clients__tabs label:nth-of-type(1),
.clients > input:nth-of-type(2):checked ~ .clients__tabs label:nth-of-type(2),
.clients > input:nth-of-type(3):checked ~ .clients__tabs label:nth-of-type(3),
.clients > input:nth-of-type(4):checked ~ .clients__tabs label:nth-of-type(4) { color: #f2f2ef; background: rgba(242, 242, 239, 0.1); }
.clients > input:nth-of-type(1):focus-visible ~ .clients__tabs label:nth-of-type(1),
.clients > input:nth-of-type(2):focus-visible ~ .clients__tabs label:nth-of-type(2),
.clients > input:nth-of-type(3):focus-visible ~ .clients__tabs label:nth-of-type(3),
.clients > input:nth-of-type(4):focus-visible ~ .clients__tabs label:nth-of-type(4) { outline: 2px solid #a9b9ff; outline-offset: 2px; }
/* Every client's card shares one grid cell, so the tallest sets the height and
   switching tabs never moves the page. */
.clients__panels { display: grid; margin: 12px 0 0; }
.clients__panels > section { display: flex; min-width: 0; grid-area: 1 / 1; visibility: hidden; }
.clients > input:nth-of-type(1):checked ~ .clients__panels > :nth-child(1),
.clients > input:nth-of-type(2):checked ~ .clients__panels > :nth-child(2),
.clients > input:nth-of-type(3):checked ~ .clients__panels > :nth-child(3),
.clients > input:nth-of-type(4):checked ~ .clients__panels > :nth-child(4) { visibility: visible; }
.clients h3 { position: absolute; width: 1px; height: 1px; margin: -1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
.code { display: flex; min-width: 0; flex: 1; flex-direction: column; overflow: hidden; border: 1px solid var(--edge); border-radius: 13px; background: rgba(242, 242, 239, 0.025); }
.code__head { display: flex; min-height: 46px; align-items: center; justify-content: space-between; gap: 16px; padding: 4px 4px 4px 18px; border-bottom: 1px solid var(--rule); color: #8a8b90; font: 13px ${MONO}; }
.code pre { flex: 1; margin: 0; padding: 16px 18px; overflow-x: auto; color: #d6d7db; font: 13px/1.6 ${MONO}; }
.code pre code { padding: 0; border: 0; background: none; font: inherit; white-space: pre; }
.code__more { display: grid; grid-template-columns: 7rem minmax(0, 1fr); align-items: baseline; gap: 6px 16px; margin: 0; padding: 14px 18px; border-top: 1px solid var(--rule); font-size: 14px; }
.code__more dt { color: #8a8b90; }
.code__more dd { min-width: 0; margin: 0; color: #b4b5ba; }
.code__more .cmd { color: #b4b5ba; }
.mcp table { width: 100%; margin: 20px 0 0; border-bottom: 1px solid var(--line); border-collapse: collapse; }
.mcp th, .mcp td { padding: 14px 0; border-top: 1px solid var(--line); text-align: left; vertical-align: baseline; }
.mcp th { width: 13.5rem; padding-right: 16px; color: #f2f2ef; font: 13px/1.6 ${MONO}; white-space: nowrap; }
.mcp td { color: #b4b5ba; font-size: 15px; }
.mcp .about { margin-top: 24px; padding-top: 0; border-top: 0; font-size: 14px; }
.mcp td, .mcp .row > span, .mcp .note { text-wrap: pretty; }
@media (max-width: 640px) {
  .clients__tabs { display: grid; grid-auto-flow: row; grid-template-columns: 1fr 1fr; }
  .mcp .row, .mcp .row--copy { grid-template-columns: minmax(0, 1fr) auto; gap: 6px 16px; }
  .mcp .row__label { grid-column: 1 / -1; }
  .mcp th, .mcp td { display: block; width: auto; }
  .mcp th { padding-bottom: 0; }
  .mcp td { padding-top: 4px; border-top: 0; }
  .code__more { grid-template-columns: minmax(0, 1fr); gap: 2px; }
  .code__more dd { margin-bottom: 8px; }
}
`

export const CONFIG_OPENCODE = `{
  "mcp": {
    "servers": {
      "hex": { "type": "local", "command": ["hex", "mcp"] }
    }
  }
}`

const CONFIG_DESKTOP = `{
  "mcpServers": {
    "hex": { "command": "hex", "args": ["mcp"] }
  }
}`

const COMMAND_CLAUDE = "claude mcp add hex -- hex mcp"

const OPENCODE_JSONLD = {
  "@context": "https://schema.org",
  "@type": "SoftwareApplication",
  name: "Hex MCP",
  description: DESCRIPTION,
  url: "https://anoma.ly/hex/mcp",
  applicationCategory: "DeveloperApplication",
  operatingSystem: "macOS, Linux, Windows",
  softwareVersion: "__HEX_VERSION__",
  offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
  isPartOf: { "@type": "SoftwareApplication", name: "Hex", url: "https://anoma.ly/hex/" },
  sameAs: ["https://github.com/anomalyco/hex"],
  author: { "@type": "Person", name: "Kit Langton" },
}

const COPY_ICONS =
  '<svg aria-hidden="true" viewBox="0 0 24 24"><rect x="8.5" y="8.5" width="11" height="11" rx="2"/><path d="M15.5 8.5V6.5a2 2 0 0 0-2-2h-7a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h2"/></svg><svg aria-hidden="true" viewBox="0 0 24 24"><path d="m5 12.5 4.5 4.5L19 7.5"/></svg>'

// Hidden until the page's script runs, so a page without scripts shows no
// button that cannot work.
function copy(text: string, name: string): string {
  return `<button type="button" class="copy" data-copy="${escape(text)}" aria-label="Copy ${escape(name)}" title="Copy" hidden>${COPY_ICONS}</button>`
}

const COPY_SCRIPT = `<script>
for (const button of document.querySelectorAll("[data-copy]")) {
  button.hidden = false
  button.addEventListener("click", () => {
    void navigator.clipboard?.writeText(button.dataset.copy).then(() => {
      button.dataset.copied = ""
      setTimeout(() => delete button.dataset.copied, 1600)
    })
  })
}
</script>`

// Where the installed `hex` is on each OS: packaging/homebrew/hex.rb and the
// README (macOS), docs/windows.md, and docs/linux.md with packaging/linux.
const INSTALLS: { os: string; path: string; kind?: string; note: string }[] = [
  {
    os: "macOS",
    path: "ln -s /Applications/Hex.app/Contents/MacOS/hex ~/.local/bin/hex",
    kind: "command",
    note: "After a DMG install, link it once. Homebrew links it for you.",
  },
  { os: "Windows", path: "%APPDATA%\\hex\\bin\\hex.com", note: "On your <code>PATH</code> after install. Reopen the terminal." },
  { os: "Linux", path: "~/.local/bin/hex", note: "From the install script. Packages put it at <code>/usr/bin/hex</code>." },
]

type Client = { id: string; name: string; file: string; code: string; more: [label: string, value: string][] }

const mono = (text: string) => `<code class="cmd">${escape(text)}</code>`

const CLIENTS: Client[] = [
  {
    id: "opencode",
    name: "OpenCode",
    file: "opencode.json",
    code: CONFIG_OPENCODE,
    more: [
      ["Every project", mono("~/.config/opencode/opencode.json")],
      ["Or run", mono("opencode mcp add hex -- hex mcp")],
    ],
  },
  {
    id: "claude-code",
    name: "Claude Code",
    file: "Terminal",
    code: COMMAND_CLAUDE,
    more: [["Every project", mono("claude mcp add --scope user hex -- hex mcp")]],
  },
  {
    id: "claude-desktop",
    name: "Claude Desktop",
    file: "claude_desktop_config.json",
    code: CONFIG_DESKTOP,
    more: [
      ["macOS", mono("~/Library/Application Support/Claude/")],
      ["Windows", mono("%APPDATA%\\Claude\\")],
      ["Then", "Restart Claude Desktop."],
    ],
  },
  {
    id: "cursor",
    name: "Cursor",
    file: ".cursor/mcp.json",
    code: CONFIG_DESKTOP,
    more: [["Every project", mono("~/.cursor/mcp.json")]],
  },
]

const TOOLS: [name: string, returns: string][] = [
  ["list_meetings", "Meetings, newest first. Filter by date or words."],
  ["search_transcripts", "Ranked, fuzzy search across transcripts and notes."],
  ["get_transcript", "Turns with <code>mm:ss</code> offsets, by time range or speaker."],
  ["get_meeting_notes", "The stored title and a two-sentence summary."],
  ["get_chapters", "Topics with start and end times."],
  ["hex-meeting://<meeting_id>", "A resource: one meeting as Markdown."],
]

const FACTS: [label: string, value: string][] = [
  ["Read-only", "It never records, edits, or deletes a meeting."],
  ["Local", "Reads the meetings folder on disk. No network, login, or running Hex."],
  ["Speakers", "Your microphone is <code>You</code>. System audio is <code>Them</code>."],
  ["Meetings", `On macOS, Windows, and Linux. Turn it on in Hex's <a href="${withBase("/meetings/")}">Meetings</a> tab.`],
  ["Elsewhere", "<code>hex mcp --meetings-dir &lt;path&gt;</code> reads a copied meetings folder."],
]

function clientCard(client: Client): string {
  const more = client.more.map(([label, value]) => `<dt>${label}</dt><dd>${value}</dd>`).join("")
  return `<section>
<h3>${client.name}</h3>
<div class="code">
<div class="code__head"><span>${escape(client.file)}</span>${copy(client.code, `the ${client.name} setup`)}</div>
<pre><code>${escape(client.code)}</code></pre>
<dl class="code__more">${more}</dl>
</div>
</section>`
}

export function renderMcp(origin: string): string {
  const installs = INSTALLS.map(
    (install) =>
      `<div class="row row--copy"><span class="row__label">${install.os}</span><span class="row__value">${mono(install.path)}<small>${install.note}</small></span>${copy(install.path, `the ${install.os} ${install.kind ?? "path"}`)}</div>`,
  ).join("\n")
  const radios = CLIENTS.map((client, index) => `<input type="radio" name="client" id="client-${client.id}"${index === 0 ? " checked" : ""}>`).join("")
  const tabs = CLIENTS.map((client) => `<label for="client-${client.id}">${client.name}</label>`).join("")
  const tools = TOOLS.map(([name, returns]) => `<tr><th scope="row">${escape(name)}</th><td>${returns}</td></tr>`).join("\n")
  const facts = FACTS.map(([label, value]) => `<div class="row"><span class="row__label">${label}</span><span>${value}</span></div>`).join("\n")

  const body = `<div class="mcp">
<p class="eyebrow">Hex · MCP server</p>
<h1>Your meetings, readable by your agent.</h1>
<p class="lede"><code>hex mcp</code> is a read-only MCP server for the meetings Hex records. It runs over stdio on your machine, with no account, API key, or network.</p>

<h2>Install</h2>
<p class="sub">Get <a href="${withBase("/")}">Hex</a>, then check that <code>hex --version</code> runs in a terminal.</p>
<div class="rows">
${installs}
</div>

<h2>Connect</h2>
<div class="clients">
${radios}
<div class="clients__tabs">${tabs}</div>
<div class="clients__panels">
${CLIENTS.map(clientCard).join("\n")}
</div>
</div>
<p class="note">On Windows, <code>~</code> is <code>%USERPROFILE%</code>. A client that can't find <code>hex</code> takes its full path as the command.</p>

<h2>Tools</h2>
<table>
<tbody>
${tools}
</tbody>
</table>

<h2>Good to know</h2>
<div class="rows">
${facts}
</div>

<section class="about">
<p><a href="${withBase("/meetings/")}">How Hex records a meeting</a> · <a href="https://github.com/anomalyco/hex/blob/dev/docs/transcript-cli.md">Transcript CLI docs</a> · <a href="${withBase("/")}">Hex for Mac, Windows, and Linux</a></p>
</section>
</div>
${COPY_SCRIPT}`

  const meta: Meta & { jsonLd?: unknown } = {
    title: "Hex MCP: meeting transcripts for your agent",
    description: DESCRIPTION,
    url: MCP_PATH,
    image: MCP_CARD_PATH,
    imageAlt: "Hex MCP: your agent can read your meetings.",
    jsonLd: OPENCODE_JSONLD,
  }
  return shell(origin, meta, body, { crumb: { href: MCP_PATH, label: "MCP" }, style: STYLE })
}