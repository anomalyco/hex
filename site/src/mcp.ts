// Hex MCP at /hex/mcp: a static page for the local, read-only MCP server
// (\`hex mcp\`, src/meeting/mcp.rs), with the setup each client needs. The tool
// names and limits here are the ones the server advertises; the page has no
// JavaScript, so every word is in the HTML a crawler or agent fetches.
import { withBase } from "./base"
import { escape, shell, type Meta } from "./labs"

export const MCP_PATH = withBase("/mcp")
export const MCP_CARD_PATH = withBase("/mcp.png")

const DESCRIPTION =
  "hex mcp is Hex's local, read-only MCP server. It gives OpenCode, Claude Code, Claude Desktop, and Cursor your recorded meeting transcripts, chapters, and notes. No cloud, no account, no API key."

const STYLE = `
.mcp h2 { margin: 56px 0 0; font-size: 24px; font-weight: 600; letter-spacing: -0.02em; line-height: 1.2; }
.mcp h2 + p, .mcp h3 + p { margin: 12px 0 0; }
.mcp h3 { margin: 32px 0 0; font-size: 17px; font-weight: 600; letter-spacing: -0.01em; }
.mcp p { margin: 16px 0 0; color: #b4b5ba; }
.mcp p.lede { margin: 18px 0 0; font-size: 20px; line-height: 1.45; max-width: 32em; color: #b4b5ba; text-wrap: pretty; }
.mcp ul { margin: 14px 0 0; padding: 0 0 0 20px; color: #b4b5ba; }
.mcp li { margin: 8px 0 0; }
.mcp li code { white-space: nowrap; }
.mcp pre { margin: 14px 0 0; padding: 16px 18px; overflow-x: auto; border: 1px solid #222326; border-radius: 10px; background: #0d0d0f; color: #d6d7db; font: 13px/1.6 ui-monospace, SFMono-Regular, Menlo, monospace; }
.mcp pre code { padding: 0; border: 0; background: none; font: inherit; white-space: pre; }
.mcp .path { margin: 10px 0 0; color: #8a8b90; font-size: 14px; }
.mcp .path code { white-space: normal; }
.mcp .note { color: #8a8b90; font-size: 15px; }
.mcp table { width: 100%; margin: 16px 0 0; border-collapse: collapse; }
.mcp th, .mcp td { padding: 12px 20px 12px 0; border-top: 1px solid #1d1e21; text-align: left; vertical-align: top; }
.mcp th { color: #8a8b90; font-size: 13px; font-weight: 500; }
.mcp td { color: #b4b5ba; font-size: 15px; }
.mcp td:first-child { width: 34%; color: #f2f2ef; font: 13px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace; }
.mcp .about { margin-top: 64px; }
`

const CONFIG_OPENCODE = `{
  "mcp": {
    "servers": {
      "hex": {
        "type": "local",
        "command": ["hex", "mcp"]
      }
    }
  }
}`

const CONFIG_DESKTOP = `{
  "mcpServers": {
    "hex": {
      "command": "hex",
      "args": ["mcp"]
    }
  }
}`

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

export function renderMcp(origin: string): string {
  const body = `<div class="mcp">
<p class="eyebrow">Hex · MCP server</p>
<h1>Your meetings, readable by your agent.</h1>
<p class="lede">Hex records calls on your machine. <code>hex mcp</code> is a read-only MCP server over stdio that serves those meetings to an AI agent, from the same files Hex writes. No account, no API key, and no network request.</p>

<h2>Set it up</h2>
<p>Install Hex first. On macOS the <code>hex</code> command lives inside the app: Homebrew links it, and otherwise you link it once.</p>
<pre><code>ln -s /Applications/Hex.app/Contents/MacOS/hex ~/.local/bin/hex</code></pre>
<p class="note">Linux and Windows installs put <code>hex</code> on your <code>PATH</code>. Record a meeting before you start, so there is something to read: Hex asks when it sees a call, or press Record in the menu bar.</p>

<h3>OpenCode</h3>
<p>Add Hex to <code>opencode.json</code> in a project, or to <code>~/.config/opencode/opencode.json</code> for every project.</p>
<pre><code>${escape(CONFIG_OPENCODE)}</code></pre>

<h3>Claude Code</h3>
<p>One command adds the server to the project you are in. Add <code>--scope user</code> to make it available everywhere.</p>
<pre><code>claude mcp add hex -- hex mcp</code></pre>

<h3>Claude Desktop</h3>
<p>Add Hex to <code>claude_desktop_config.json</code>, then restart Claude Desktop. On macOS the file is at <code>~/Library/Application Support/Claude/claude_desktop_config.json</code>; on Windows it is at <code>%APPDATA%\\Claude\\claude_desktop_config.json</code>.</p>
<pre><code>${escape(CONFIG_DESKTOP)}</code></pre>

<h3>Cursor</h3>
<p>Add Hex to <code>.cursor/mcp.json</code> in a project, or to <code>~/.cursor/mcp.json</code> for every project. Cursor reads the same <code>mcpServers</code> shape as Claude Desktop.</p>
<pre><code>${escape(CONFIG_DESKTOP)}</code></pre>

<h2>What it can read</h2>
<table>
<thead><tr><th>Tool</th><th>What it returns</th></tr></thead>
<tbody>
<tr><td>list_meetings</td><td>Recorded meetings, newest first: id, title, start time, duration, status, transcript kind, participants, and whether notes exist. Filter by <code>since</code>, <code>until</code>, or words in the title or app; 20 by default, 200 at most.</td></tr>
<tr><td>search_transcripts</td><td>Full-text search across transcripts and stored notes, ranked, with the meeting id, speaker, offset, and a snippet. Stemmed and fuzzy, so a misheard name still matches; quote a phrase to require it exactly.</td></tr>
<tr><td>get_transcript</td><td>The transcript as merged <code>You</code>/<code>Them</code> turns with <code>mm:ss</code> offsets. Read a time range or one speaker; long results stop with a cursor to continue.</td></tr>
<tr><td>get_meeting_notes</td><td>The stored title and a summary of at most two sentences. It never writes or regenerates notes.</td></tr>
<tr><td>get_chapters</td><td>The topics a meeting moved through, each with a name, one line, and its start and end times, when the recording had live chapters.</td></tr>
</tbody>
</table>
<p>It also serves one resource per meeting at <code>hex-meeting://&lt;meeting_id&gt;</code>: the metadata, stored notes, and transcript as Markdown.</p>

<h2>What it can't do</h2>
<ul>
<li>Record, stop, edit, or delete a meeting. Every tool is read-only and says so.</li>
<li>Write notes or chapters. It returns what was stored when the meeting was recorded.</li>
<li>Tell remote speakers apart. The microphone is <code>You</code> and everything from system audio is <code>Them</code>.</li>
<li>Read anything off this machine. There is no cloud service, and the server answers from the meetings folder on disk.</li>
</ul>

<h2>Privacy</h2>
<p>The server reads the same files the app writes and logs only to stderr. It needs no running Hex, no login, and no network. By default it reads this account's meetings; <code>hex mcp --meetings-dir &lt;path&gt;</code> points it at a folder you copied elsewhere, such as a mirror of a laptop's meetings. The setup, the queries, and the retention rules are in the <a href="https://github.com/anomalyco/hex/blob/dev/docs/transcript-cli.md">transcript CLI docs</a>.</p>
<p class="note">Meetings record on the Mac today. The read commands run on macOS, Linux, and Windows.</p>

<section class="about">
<p><a href="${withBase("/meetings/")}">How Hex records a meeting</a> · <a href="${withBase("/")}">Hex for Mac, Windows, and Linux</a></p>
</section>
</div>`

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
