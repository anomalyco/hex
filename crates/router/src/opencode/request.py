"""One authenticated loopback request. Never starts OpenCode or prints secrets."""
import base64
import glob
import http.client
import json
import os
import sys
import time
import urllib.parse


def main():
    request = json.load(sys.stdin)
    deadline = time.monotonic() + request["timeout"]
    state = os.environ.get("XDG_STATE_HOME", os.path.expanduser("~/.local/state"))
    config = os.environ.get("OPENCODE_CONFIG_DIR", os.path.expanduser("~/.config/opencode"))
    paths = [os.path.join(state, "opencode/service.json")]
    paths += sorted(glob.glob(os.path.join(state, "opencode/service-*.json")))
    paths += [os.path.join(config, "service.json")]
    service = None
    for path in paths:
        try:
            with open(path, encoding="utf-8") as source:
                candidate = json.load(source)
            if candidate.get("url") or candidate.get("port"):
                service = candidate
                break
        except (OSError, ValueError):
            continue
    if service is None:
        raise RuntimeError("OpenCode isn't running")
    url = urllib.parse.urlsplit(service.get("url") or "http://%s:%s" % (
        service.get("hostname", "127.0.0.1"), service["port"]))
    if url.scheme != "http" or url.hostname not in ("localhost", "127.0.0.1", "::1"):
        raise RuntimeError("OpenCode registration must name loopback HTTP")
    if not request["path"].startswith("/api/") or "\r" in request["path"] or "\n" in request["path"]:
        raise RuntimeError("Invalid API path")
    headers = {"Content-Type": "application/json"}
    password = service.get("password") or os.environ.get("OPENCODE_PASSWORD") or os.environ.get("OPENCODE_SERVER_PASSWORD")
    if password:
        headers["Authorization"] = "Basic " + base64.b64encode(("opencode:" + password).encode()).decode()
    connection = http.client.HTTPConnection(url.hostname, url.port, timeout=max(.001, request["timeout"]))
    if service.get("pid"):
        connection.request("GET", "/api/info", headers=headers)
        info = connection.getresponse()
        data = info.read(65537)
        if info.status != 200 or len(data) > 65536 or json.loads(data).get("pid") != service["pid"]:
            raise RuntimeError("OpenCode registration is stale")
    left = deadline - time.monotonic()
    if left <= 0:
        raise TimeoutError()
    connection.timeout = left
    if connection.sock:
        connection.sock.settimeout(left)
    body = None if request["body"] is None else json.dumps(request["body"]).encode()
    connection.request(request["method"], request["path"], body, headers)
    response = connection.getresponse()
    data = response.read(16 * 1024 * 1024 + 1)
    if len(data) > 16 * 1024 * 1024:
        raise RuntimeError("OpenCode response too large")
    print(json.dumps({"status": response.status, "body": data.decode()}))
    connection.close()


try:
    main()
except Exception:
    # Neither request data nor credential-bearing registration errors belong in logs.
    sys.stderr.write("Remote OpenCode HTTP request failed\n")
    sys.exit(1)
