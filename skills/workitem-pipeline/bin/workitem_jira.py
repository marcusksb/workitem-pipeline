"""Jira Cloud REST access for the watch scripts, with the setup of jira-cli.

`server` and `login` come from jira-cli's config (`JIRA_CONFIG_FILE`, else
`~/.config/.jira/.config.yml`), the API token from `JIRA_API_TOKEN`, else `~/.netrc`, else the OS
keyring (service `jira-cli`).
"""
import base64, json, netrc, os, re, subprocess, sys, urllib.parse, urllib.request

KEY = re.compile(r"[A-Z][A-Z0-9]+-\d+")


def issue_key(value):
    """The Jira key of an `issue:` value (`KEY (url)`, `KEY` or `none`), or None."""
    key = ((value or "").split() or [""])[0]
    return key if KEY.fullmatch(key) else None


def jira_config():
    """`server` and `login`, the top-level keys of jira-cli's YAML config."""
    base = os.environ.get("XDG_CONFIG_HOME") or os.path.expanduser("~/.config")
    path = os.environ.get("JIRA_CONFIG_FILE") or os.path.join(base, ".jira", ".config.yml")
    with open(path) as f:
        top = dict(m.groups() for m in re.finditer(r"^(server|login):[ \t]*['\"]?([^'\"\n]*?)['\"]?[ \t]*$", f.read(), re.M))
    return top.get("server", "").rstrip("/"), top.get("login", "")


def keyring_token(login):
    commands = {
        "darwin": ["security", "find-generic-password", "-s", "jira-cli", "-a", login, "-w"],
        "linux": ["secret-tool", "lookup", "service", "jira-cli", "username", login],
    }
    command = commands.get(sys.platform)
    if not command:
        return ""
    try:
        result = subprocess.run(command, capture_output=True, text=True, timeout=10)
    except (OSError, subprocess.TimeoutExpired):
        return ""
    return result.stdout.strip() if result.returncode == 0 else ""


def api_token(server, login):
    """Same order as jira-cli: the env var, then `~/.netrc`, then the keyring."""
    token = os.environ.get("JIRA_API_TOKEN", "")
    if not token:
        try:
            entry = netrc.netrc().authenticators(urllib.parse.urlparse(server).hostname or "")
            token = entry[2] if entry else ""
        except (OSError, netrc.NetrcParseError):
            pass
    return token or keyring_token(login)


def credentials():
    """(server, login, token); raises RuntimeError when any of them is missing."""
    try:
        server, login = jira_config()
    except OSError as e:
        raise RuntimeError(f"no jira-cli config: {e}") from e
    token = api_token(server, login)
    if not (server and login and token):
        raise RuntimeError("no Jira server, login or API token (see jira-cli)")
    return server, login, token


def request(server, login, token, method, path, body=None):
    """One JSON request; returns the decoded body, or None when it is empty (204)."""
    auth = base64.b64encode(f"{login}:{token}".encode()).decode()
    req = urllib.request.Request(f"{server}{path}", data=None if body is None else json.dumps(body).encode(), method=method,
                                 headers={"Authorization": f"Basic {auth}", "Content-Type": "application/json", "Accept": "application/json"})
    with urllib.request.urlopen(req, timeout=30) as response:
        data = response.read()
    return json.loads(data) if data else None


def transition_to(server, login, token, key, target):
    """Moves `key` to the status named `target` (case-insensitive). Returns "moved", or "already"
    when the issue is in that status or in a done one. Raises RuntimeError when no transition
    leads there, naming the ones that do exist."""
    status = request(server, login, token, "GET", f"/rest/api/3/issue/{key}?fields=status")["fields"]["status"]
    if status["statusCategory"]["key"] == "done" or status["name"].casefold() == target.casefold():
        return "already"
    transitions = request(server, login, token, "GET", f"/rest/api/3/issue/{key}/transitions")["transitions"]
    match = next((t for t in transitions if t["to"]["name"].casefold() == target.casefold()), None)
    if not match:
        names = ", ".join(t["to"]["name"] for t in transitions) or "none"
        raise RuntimeError(f"{key} has no transition from {status['name']} to {target}; available: {names}")
    request(server, login, token, "POST", f"/rest/api/3/issue/{key}/transitions", {"transition": {"id": match["id"]}})
    return "moved"
