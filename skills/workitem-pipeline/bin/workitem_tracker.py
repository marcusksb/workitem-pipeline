"""The issue trackers a workspace can use (decisions 62 to 72): Jira, over REST with jira-cli's
credentials, and GitHub Issues, through `gh`. `workitem-issue` and `watch/issue-status` use it;
nothing else talks to a tracker.

A workspace names its tracker in `workspace.toml` (`issue_tracker`, `jira` when missing) and may
name a GitHub account (`github_account`). A board names where new issues go (`issue_project`).
"""
import base64, json, netrc, os, re, subprocess, sys, tomllib, urllib.error, urllib.parse, urllib.request
from pathlib import Path

TRACKERS = ("jira", "github")
JIRA_KEY = re.compile(r"[A-Z][A-Z0-9]+-\d+")
JIRA_PROJECT = re.compile(r"[A-Z][A-Z0-9]+")
GITHUB_REPO = re.compile(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+")
GITHUB_REF = re.compile(rf"({GITHUB_REPO.pattern})#(\d+)")
GITHUB_URL = re.compile(rf"https://github\.com/({GITHUB_REPO.pattern})/issues/(\d+)/?")
# What a stage's `issue_state` may name on a GitHub board.
GITHUB_STATES = ("assigned", "closed")
TIMEOUT = 30
PAGE = 100
LUCENE_SYNTAX = re.compile(r'[-+&|!(){}\[\]^"~*?:\\/]')


class Problem(Exception):
    """A setup or input error: a bad ref, a missing key. Callers stop on it."""


# --- workspace and board ---------------------------------------------------------------------

def settings(workspace):
    """`workspace.toml` of the workspace folder, or {} when there is none."""
    path = Path(workspace, "workspace.toml")
    try:
        return tomllib.loads(path.read_text())
    except FileNotFoundError:
        return {}
    except (OSError, tomllib.TOMLDecodeError) as e:
        raise Problem(f"{path}: {e}")


def tracker_of(workspace):
    name = settings(workspace).get("issue_tracker", "jira")
    if name not in TRACKERS:
        raise Problem(f"issue_tracker '{name}' in {workspace}/workspace.toml is not one of {', '.join(TRACKERS)}")
    return name


def board(workspace, name):
    path = Path(workspace, "boards", f"{name}.json")
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError) as e:
        raise Problem(f"board '{name}': {e}")


def issue_key(issue):
    """The ref of an `issue:` value (`<ref> (<url>)`), or "" for none."""
    key = (issue or "").split()[0] if (issue or "").split() else ""
    return "" if key.lower() == "none" else key


def own_key(tracker, issue):
    """The ref of an `issue:` value, refused when it belongs to the other tracker (62); "" for none."""
    key = issue_key(issue)
    pattern = JIRA_KEY if tracker == "jira" else GITHUB_REF
    if key and not pattern.fullmatch(key):
        raise Problem(f"issue '{key}' is not a {'Jira' if tracker == 'jira' else 'GitHub'} issue, and this workspace uses {tracker}")
    return key


# --- gh ----------------------------------------------------------------------------------------

def gh_env(workspace):
    """The environment for `gh`: the workspace's `github_account` (67) as GH_TOKEN, unless the
    caller (the backend) already set one."""
    env = dict(os.environ)
    account = settings(workspace).get("github_account") if workspace else None
    if account and not env.get("GH_TOKEN"):
        result = subprocess.run([gh_path(), "auth", "token", "--user", account], capture_output=True, text=True, timeout=TIMEOUT)
        if result.returncode != 0 or not result.stdout.strip():
            raise Problem(f"GitHub account {account} is not logged in: run gh auth login")
        env["GH_TOKEN"] = result.stdout.strip()
    return env


def gh_path():
    return os.environ.get("WORKITEM_GH", "gh")


def gh(workspace, *args, cwd=None, partial=False, stdin=None):
    """stdout of `gh <args>` (fed `stdin`); RuntimeError with gh's message when it fails. With `partial`, a
    failed GraphQL query that still answered with data returns that answer: one issue that is
    gone (or a PR) must not hide the others."""
    try:
        result = subprocess.run([gh_path(), *args], input=stdin, capture_output=True, text=True, timeout=TIMEOUT, cwd=cwd, env=gh_env(workspace))
    except subprocess.TimeoutExpired:
        raise RuntimeError(f"gh {args[0]} timed out")
    if result.returncode != 0 and not (partial and result.stdout.lstrip().startswith('{"data"')):
        raise RuntimeError(result.stderr.strip() or f"gh exited with {result.returncode}")
    return result.stdout


# --- Jira --------------------------------------------------------------------------------------

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


class Jira:
    """Jira Cloud's REST API with jira-cli's server, login and token."""

    def __init__(self):
        try:
            self.server, self.login = jira_config()
        except OSError as e:
            raise Problem(f"no jira-cli config: {e}")
        self.token = api_token(self.server, self.login)
        if not (self.server and self.login and self.token):
            raise Problem("no Jira server, login or API token (see jira-cli)")

    def call(self, method, path, body=None):
        auth = base64.b64encode(f"{self.login}:{self.token}".encode()).decode()
        headers = {"Authorization": f"Basic {auth}", "Accept": "application/json"}
        data = None
        if body is not None:
            data = json.dumps(body).encode()
            headers["Content-Type"] = "application/json"
        request = urllib.request.Request(f"{self.server}{path}", data=data, method=method, headers=headers)
        try:
            with urllib.request.urlopen(request, timeout=TIMEOUT) as response:
                text = response.read()
        except urllib.error.HTTPError as e:
            raise RuntimeError(f"Jira answered {e.code}: {e.read().decode(errors='replace')[:300]}")
        return json.loads(text) if text else None

    def url(self, key):
        return f"{self.server}/browse/{key}"

    def statuses(self, keys):
        """`{key: (status name, category key)}` of up to PAGE keys; keys Jira does not know are left out."""
        body = {"jql": f"key in ({', '.join(keys)})", "fields": ["status"], "maxResults": PAGE}
        issues = self.call("POST", "/rest/api/3/search/jql", body)["issues"]
        return {i["key"]: (i["fields"]["status"]["name"], i["fields"]["status"]["statusCategory"]["key"]) for i in issues}

    def search(self, project, words):
        """`[(key, status, labels, summary)]` of up to PAGE open issues of `project` matching `words`.
        Lucene syntax characters become spaces and the words lower case (no AND/OR/NOT), so any
        words make a valid query; Jira's text search ignores punctuation and case anyway."""
        text = " ".join(LUCENE_SYNTAX.sub(" ", words).lower().split())
        if not text:
            raise Problem(f"no words to search for in {words!r}")
        body = {"jql": f'project = "{project}" AND statusCategory != Done AND text ~ "{text}"',
                "fields": ["summary", "status", "labels"], "maxResults": PAGE}
        issues = self.call("POST", "/rest/api/3/search/jql", body)["issues"]
        return [(i["key"], i["fields"]["status"]["name"], i["fields"].get("labels") or [], i["fields"]["summary"]) for i in issues]

    def read(self, key):
        """Title, status, body and `[(author, created, id, text)]` of the issue."""
        issue = self.call("GET", f"/rest/api/3/issue/{key}?fields=summary,status,description,comment")
        fields = issue["fields"]
        comments = [(c.get("author", {}).get("displayName", "?"), c.get("created", ""), str(c.get("id", "")), adf_text(c.get("body")))
                    for c in fields.get("comment", {}).get("comments", [])]
        return fields["summary"], fields["status"]["name"], adf_text(fields.get("description")), comments

    def create(self, project, title, body):
        fields = {"project": {"key": project}, "summary": title, "issuetype": {"name": "Task"}, "description": adf_doc(body)}
        return self.call("POST", "/rest/api/3/issue", {"fields": fields})["key"]

    def comment(self, key, body):
        self.call("POST", f"/rest/api/3/issue/{key}/comment", {"body": adf_doc(body)})

    def set_state(self, key, states):
        """Moves the issue to the first of `states` it has a transition to and, when `states` names
        `assigned`, assigns it to the API user (replacing another assignee); what happened, as text.
        It assigns first, so a missing transition does not skip the assignment."""
        fields = self.call("GET", f"/rest/api/3/issue/{key}?fields=status,assignee")["fields"]
        done = []
        if "assigned" in states:
            me = self.call("GET", "/rest/api/3/myself")
            if (fields.get("assignee") or {}).get("accountId") == me["accountId"]:
                done.append(f"already assigned to {me['displayName']}")
            else:
                self.call("PUT", f"/rest/api/3/issue/{key}/assignee", {"accountId": me["accountId"]})
                done.append(f"assigned to {me['displayName']}")
        if statuses := [s for s in states if s != "assigned"]:
            done.insert(0, self.transition(key, fields["status"]["name"], statuses))
        return f"{key}: {', '.join(done)}"

    def transition(self, key, current, states):
        if current.lower() in (s.lower() for s in states):
            return f"already {current}"
        transitions = self.call("GET", f"/rest/api/3/issue/{key}/transitions")["transitions"]
        by_target = {t["to"]["name"].lower(): t for t in transitions}
        for state in states:
            transition = by_target.get(state.lower())
            if transition:
                self.call("POST", f"/rest/api/3/issue/{key}/transitions", {"transition": {"id": transition["id"]}})
                return f"{current} -> {transition['to']['name']}"
        available = ", ".join(t["to"]["name"] for t in transitions) or "none"
        raise RuntimeError(f"{key} has no transition to {' or '.join(states)} (available: {available})")


def adf_doc(text):
    """An Atlassian Document Format doc of `text`, one paragraph per blank-line-separated block,
    its lines joined by hard breaks."""
    return {"type": "doc", "version": 1, "content": [adf_paragraph(p) for p in text.split("\n\n") if p.strip()]}

def adf_paragraph(block):
    content = []
    for line in block.strip("\n").split("\n"):
        if content:
            content.append({"type": "hardBreak"})
        if line:
            content.append({"type": "text", "text": line})
    return {"type": "paragraph", "content": content}

def adf_text(node):
    """Plain text of an Atlassian Document Format node: text joined, blocks on their own lines."""
    if not isinstance(node, dict):
        return node if isinstance(node, str) else ""
    if node.get("type") == "text":
        return node.get("text", "")
    if node.get("type") == "hardBreak":
        return "\n"
    inner = "".join(adf_text(child) for child in node.get("content", []))
    blocks = ("paragraph", "heading", "listItem", "codeBlock", "blockquote", "rule")
    return inner + "\n" if node.get("type") in blocks else inner


# --- GitHub Issues -----------------------------------------------------------------------------

def github_status(state, assigned):
    """Status name and category (the Jira category keys the board sorts by) of a GitHub issue (65)."""
    if state.upper() == "CLOSED":
        return "Closed", "done"
    return ("In progress", "indeterminate") if assigned else ("Open", "new")


class GitHub:
    def __init__(self, workspace):
        self.workspace = workspace

    def url(self, ref):
        repo, number = ref.split("#")
        return f"https://github.com/{repo}/issues/{number}"

    def statuses(self, refs):
        """`{ref: (status name, category)}`; one GraphQL query per repo and up to PAGE issues."""
        by_repo = {}
        for ref in refs:
            repo, number = ref.split("#")
            by_repo.setdefault(repo, []).append(int(number))
        found = {}
        for repo, numbers in sorted(by_repo.items()):
            owner, name = repo.split("/")
            for start in range(0, len(numbers), PAGE):
                part = sorted(numbers)[start:start + PAGE]
                fields = " ".join(f"i{n}: issue(number: {n}) {{ state assignees(first: 1) {{ totalCount }} }}" for n in part)
                query = f"query($owner: String!, $name: String!) {{ repository(owner: $owner, name: $name) {{ {fields} }} }}"
                try:
                    out = gh(self.workspace, "api", "graphql", "-f", f"query={query}", "-f", f"owner={owner}", "-f", f"name={name}", partial=True)
                    repository = (json.loads(out).get("data") or {}).get("repository") or {}
                except (RuntimeError, ValueError) as e:
                    # One repo that cannot be read (no access, rate limit) must not hide the others.
                    print(f"cannot read the issues of {repo}: {e}", file=sys.stderr)
                    continue
                for n in part:
                    issue = repository.get(f"i{n}")
                    if issue:
                        found[f"{repo}#{n}"] = github_status(issue["state"], issue["assignees"]["totalCount"] > 0)
        return found

    def view(self, ref, fields):
        repo, number = ref.split("#")
        return json.loads(gh(self.workspace, "issue", "view", number, "-R", repo, "--json", fields))

    def search(self, project, words):
        """`[(ref, status, labels, summary)]` of up to PAGE open issues of `project` matching `words`."""
        out = gh(self.workspace, "issue", "list", "-R", project, "--state", "open", "--search", words,
                 "--limit", str(PAGE), "--json", "number,title,labels,assignees")
        return [(f"{project}#{i['number']}", github_status("OPEN", bool(i["assignees"]))[0], [l["name"] for l in i["labels"]], i["title"])
                for i in json.loads(out)]

    def read(self, ref):
        """Title, status, body and `[(author, created, id, text)]` of the issue."""
        issue = self.view(ref, "title,state,body,comments,assignees")
        status = github_status(issue["state"], bool(issue["assignees"]))[0]
        comments = [((c.get("author") or {}).get("login", "?"), c.get("createdAt", ""), str(c.get("id", "")), c.get("body", ""))
                    for c in issue["comments"]]
        return issue["title"], status, issue["body"], comments

    def comment(self, ref, body):
        repo, number = ref.split("#")
        gh(self.workspace, "issue", "comment", number, "-R", repo, "--body-file", "-", stdin=body)

    def create(self, project, title, body):
        url = gh(self.workspace, "issue", "create", "-R", project, "--title", title, "--body", body).strip().splitlines()[-1]
        match = GITHUB_URL.fullmatch(url)
        if not match:
            raise RuntimeError(f"gh issue create printed no issue URL: {url}")
        return f"{match.group(1)}#{match.group(2)}"

    def set_state(self, ref, states):
        state = states[0]
        repo, number = ref.split("#")
        issue = self.view(ref, "state,assignees")
        if state == "closed":
            if issue["state"].upper() == "CLOSED":
                return f"{ref} is already closed"
            gh(self.workspace, "issue", "close", number, "-R", repo)
            return f"{ref}: closed"
        me = gh(self.workspace, "api", "user", "--jq", ".login").strip()
        if any(a.get("login") == me for a in issue["assignees"]):
            return f"{ref} is already assigned to {me}"
        gh(self.workspace, "issue", "edit", number, "-R", repo, "--add-assignee", "@me")
        return f"{ref}: assigned to {me}"


def client(tracker, workspace):
    return Jira() if tracker == "jira" else GitHub(workspace)


# --- refs --------------------------------------------------------------------------------------

def github_project(workspace, config):
    """Where a board's GitHub issues live: its `issue_project`, else its repo's GitHub remote."""
    if config.get("issue_project"):
        return config["issue_project"]
    repo = config.get("repo")
    if not repo:
        raise Problem("the board has no repo, so set issue_project (owner/repo) on it")
    try:
        return gh(workspace, "repo", "view", "--json", "nameWithOwner", "--jq", ".nameWithOwner", cwd=os.path.expanduser(repo)).strip()
    except RuntimeError as e:
        raise Problem(f"cannot find the GitHub repo of {repo}; set issue_project on the board: {e}")


def parse(workspace, config, text):
    """`(ref, url, folder prefix)` of what a user or agent gave: a key, `#123`, `owner/repo#123` or
    an issue URL. Refuses a ref of the other tracker (62)."""
    text = text.strip()
    tracker = tracker_of(workspace)
    if tracker == "jira":
        match = re.fullmatch(rf"(?:https?://\S+/browse/)?({JIRA_KEY.pattern})/?", text)
        if not match:
            raise Problem(f"'{text}' is not a Jira key or Jira issue URL, and this workspace uses Jira")
        key = match.group(1)
        try:
            server = jira_config()[0]
        except OSError:
            server = ""
        return key, f"{server}/browse/{key}" if server else "", key
    match = GITHUB_URL.fullmatch(text) or GITHUB_REF.fullmatch(text)
    if match:
        repo, number = match.group(1), match.group(2)
    elif re.fullmatch(r"#?\d+", text):
        repo, number = github_project(workspace, config), text.lstrip("#")
    else:
        raise Problem(f"'{text}' is not a GitHub issue (#123, owner/repo#123 or an issue URL), and this workspace uses GitHub")
    ref = f"{repo}#{number}"
    # The prefix goes into the folder name and so the tmux session `wi-<dir>`, where tmux would
    # turn a `.` (`next.js`) into `_` and the session would no longer be found.
    return ref, GitHub(workspace).url(ref), f"{re.sub(r'[^A-Za-z0-9-]', '-', repo.split('/')[1])}-{number}"
