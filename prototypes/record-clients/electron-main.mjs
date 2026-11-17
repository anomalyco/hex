// Electron main process: pair with Hex once, then record twice and revoke.
//
// This runs in the main process (not a renderer), where Node can open Hex's
// owner-only Unix socket. Hex identifies this process by its code signature
// and shows that identity in its pairing dialog; nothing here claims a name.
//
//   node electron-main.mjs                 # macOS, default socket
//   node electron-main.mjs /path/record.sock
//
// Requires Node 18+ and Hex running with Labs › Browser access to the local
// API on (the same gate lists the recording socket's consent). No token from
// Hex's discovery file is used or needed. Pairing asks once and starts
// nothing; each recording is its own session under the grant, and it runs for
// as long as this process checks in (`POST /record/heartbeat`). If the process
// quits or hangs, Hex stops the microphone within one lease (10 s). Grants
// live in Hex's process, so a restart of Hex means pairing again.

import { createHash, randomBytes } from "node:crypto";
import { request } from "node:http";
import { homedir } from "node:os";
import { join } from "node:path";

const socketPath =
  process.argv[2] ??
  join(homedir(), "Library", "Application Support", "hex", "record.sock");
// How long each demo recording runs; your app stops whenever its UI says so.
const recordForMs = Number(process.env.HEX_RECORD_MS ?? 5000);

const base64url = (buffer) =>
  buffer.toString("base64").replaceAll("+", "-").replaceAll("/", "_").replaceAll("=", "");

const pkce = () => {
  const verifier = base64url(randomBytes(48));
  const challenge = base64url(createHash("sha256").update(verifier).digest());
  return { verifier, challenge };
};

/** One JSON or empty POST/GET over Hex's Unix socket. */
function call(path, method, { body, grant, session } = {}) {
  const payload = body === undefined ? "" : JSON.stringify(body);
  return new Promise((resolve, reject) => {
    const outgoing = request(
      {
        socketPath,
        path,
        method,
        headers: {
          ...(body === undefined ? {} : { "Content-Type": "application/json" }),
          "Content-Length": Buffer.byteLength(payload),
          ...(grant ? { "X-Hex-Record-Grant": grant } : {}),
          ...(session ? { "X-Hex-Record-Token": session } : {}),
        },
      },
      (response) => {
        let text = "";
        response.setEncoding("utf8");
        response.on("data", (chunk) => (text += chunk));
        response.on("end", () => {
          let json = null;
          try {
            json = text ? JSON.parse(text) : null;
          } catch {
            // Not JSON: keep the text for the error below.
          }
          resolve({ status: response.statusCode, json, text });
        });
      },
    );
    outgoing.on("error", reject);
    outgoing.end(payload);
  });
}

const { verifier, challenge } = pkce();
const paired = await call("/record/pair", "POST", {
  body: { codeChallenge: challenge, codeChallengeMethod: "S256" },
});
if (paired.status !== 200) {
  // 403 consent-denied means the person pressed Deny (or let it time out).
  console.error("Hex did not allow pairing:", paired.status, paired.text);
  process.exit(1);
}
const { code } = paired.json;
const exchanged = await call("/record/token", "POST", {
  body: { code, codeVerifier: verifier },
});
if (exchanged.status !== 200) {
  console.error("Hex refused the grant:", exchanged.status, exchanged.text);
  process.exit(1);
}
const { grantToken, grantId } = exchanged.json;
console.log(`Paired (grant ${grantId}). Pairing started no recording.`);

/** One recording under the grant: start, check in while it runs, then finish. */
async function recordOnce() {
  const started = await call("/record/start", "POST", { grant: grantToken });
  if (started.status !== 201) {
    console.error("Hex could not start recording:", started.status, started.text);
    return null;
  }
  const { sessionToken, leaseMs } = started.json;
  console.log(`Recording for ${recordForMs} ms (checking in every ${leaseMs / 3} ms)…`);

  // Check in well inside the lease. A refused check-in means the recording
  // already ended: stopped or cancelled in Hex, or the grant was revoked.
  let ended = null;
  const beat = setInterval(async () => {
    const answer = await call("/record/heartbeat", "POST", { session: sessionToken });
    if (answer.status !== 204) {
      ended = answer;
      clearInterval(beat);
    }
  }, leaseMs / 3);
  await new Promise((resolve) => setTimeout(resolve, recordForMs));
  clearInterval(beat);
  if (ended) console.log("Recording ended in Hex:", ended.status, ended.text);

  // Finish keeps the transcript (also after a stop in Hex); `POST
  // /record/cancel` discards instead.
  const finished = await call("/record/finish", "POST", { session: sessionToken });
  if (finished.status !== 200) {
    console.error("No transcript:", finished.status, finished.text);
    return null;
  }
  return finished.json.transcript;
}

const first = await recordOnce();
if (first !== null) console.log("Transcript 1:", first);
if (exchanged.json.duration !== "always") process.exit(0); // Once is spent.
const second = await recordOnce();
if (second !== null) console.log("Transcript 2:", second);

const revoked = await call("/record/revoke", "POST", { grant: grantToken });
if (revoked.status !== 204) {
  console.error("Hex did not revoke the grant:", revoked.status, revoked.text);
  process.exit(1);
}
console.log("Revoked. The next run pairs again with a new ask.");
