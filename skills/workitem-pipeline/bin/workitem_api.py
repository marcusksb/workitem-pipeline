"""Calls to the running backend (workitem-ui) for scripts it starts.

Uses WORKITEM_PROJECT, WORKITEM_UI_PORT and WORKITEM_UI_CONFIG, which the backend sets for a
watcher. The token comes from the config file and goes in an Authorization header.
"""
import json, os, re, sys, urllib.error, urllib.parse, urllib.request


def request(method, path, body=None):
    """Returns (status, message). `message` is the backend's `error` text when it sent one."""
    project = os.environ.get("WORKITEM_PROJECT") or sys.exit("WORKITEM_PROJECT is not set (run this from a watcher)")
    port = os.environ.get("WORKITEM_UI_PORT", "7373")
    config = os.environ.get("WORKITEM_UI_CONFIG") or os.path.expanduser("~/.config/workitem-ui/config.toml")
    with open(config) as f:
        match = re.search(r'^token\s*=\s*"([^"]+)"', f.read(), re.M)
    if not match:
        sys.exit(f"no token in {config}")
    quote = lambda text: urllib.parse.quote(text, safe="")
    url = f"http://127.0.0.1:{port}/api/workitems/{quote(project)}/{path}"
    headers = {"Authorization": f"Bearer {match.group(1)}"}
    data = None
    if body is not None:
        data = json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    try:
        with urllib.request.urlopen(urllib.request.Request(url, data=data, method=method, headers=headers)) as response:
            return response.status, response.read().decode()
    except urllib.error.HTTPError as e:
        text = e.read().decode()
        try:
            text = json.loads(text)["error"]
        except (ValueError, KeyError, TypeError):
            pass
        return e.code, text or str(e.reason)
    except urllib.error.URLError as e:
        sys.exit(f"cannot reach workitem-ui on port {port}: {e.reason}")


def item_path(dir, *rest):
    return "/".join([urllib.parse.quote(dir, safe=""), *rest])
