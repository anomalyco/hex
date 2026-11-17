// Electron main process: ask Hex for one recording, then read its transcript.
//
// This runs in the main process (not a renderer), where Node can open Hex's
// owner-only Unix socket. Hex identifies this process by its code signature
// and shows that identity in its consent dialog; nothing here claims a name.
//
//   node electron-main.mjs                 # macOS, default socket
//   node electron-main.mjs /path/record.sock
//
// Requires Node 18+ and Hex running with Labs › Browser access to the local
// API on (the same gate lists the recording socket's consent). No token from
// Hex's discovery file is used or needed.

import { createHash, randomBytes } from "node:crypto";
import { request } from "node:http";
import { homedir } from "node:os";
import { join } from "node:path";

const socketPath =
  process.argv[2] ??
  join(homedir(), "Library", "Application Support", "hex", "record.sock");

const base64url = (buffer) =>
  buffer.toString("base64").replaceAll("+", "-").replaceAll("/", "_").replaceAll("=", "");

const pkce = () => {
  const verifier = base64url(randomBytes(48));
  const challenge = base64url(createHash("sha256").update(verifier).digest());
  return { verifier, challenge };
};

/** One JSON or empty POST/GET over Hex's Unix socket. */
function call(path, method, { body, token } = {}) {
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
          ...(token ? { "X-Hex-Record-Token": token } : {}),
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
const authorized = await call("/record/authorize", "POST", {
  body: { codeChallenge: challenge, codeChallengeMethod: "S256" },
});
if (authorized.status !== 200) {
  // 403 consent-denied means the person pressed Deny (or let it time out).
  console.error("Hex did not allow the recording:", authorized.status, authorized.text);
  process.exit(1);
}
const { code } = authorized.json;
const exchanged = await call("/record/token", "POST", {
  body: { code, codeVerifier: verifier },
});
if (exchanged.status !== 200) {
  console.error("Hex refused the session:", exchanged.status, exchanged.text);
  process.exit(1);
}
const { sessionToken, maxRecordingMs } = exchanged.json;
console.log(`Allowed. Recording for at most ${maxRecordingMs} ms…`);

const started = await call("/record/start", "POST", { token: sessionToken });
if (started.status !== 201) {
  console.error("Hex could not start recording:", started.status, started.text);
  process.exit(1);
}

// Stop whenever your UI says so. `POST /record/cancel` discards instead.
const finished = await call("/record/finish", "POST", { token: sessionToken });
if (finished.status !== 200) {
  console.error("No transcript:", finished.status, finished.text);
  process.exit(1);
}
console.log("Transcript:", finished.json.transcript);
