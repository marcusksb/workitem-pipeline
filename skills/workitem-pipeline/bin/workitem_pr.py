"""The PR hosts (decision 83): GitHub through `gh`, Bitbucket Cloud through the `bkt` CLI.
`workitem-pr` (and through it the backend and the skills) and the watchers `pr-status` and
`pr-remind` use it; nothing else talks to a PR host.

A PR is named by its link. `parse` is the one reader of links and its host picks the provider.
Both providers answer in GitHub's vocabulary: Bitbucket's states, build statuses, approvals and
comments are mapped inside `Bitbucket`, so no caller branches on the host. Cloud has no
changes-requested verdict (approvals are the only reviews) and no per-check required flag (every
build status counts as required).

`gh` runs with the workspace's token (`workitem_tracker.gh_env`, decision 67); `bkt` with
`--context` from WORKITEM_BKT_CONTEXT, else `bkt_context` in `workspace.toml` (82).
WORKITEM_GH and WORKITEM_BKT name the binaries.
"""
import json, os, re, shutil, subprocess, sys, tempfile, time
from collections import namedtuple
from urllib.parse import parse_qsl, urlparse

import workitem_tracker as tracker
from workitem_tracker import Problem

BITBUCKET = "bitbucket.org"
TIMEOUT = 30
# A full fresh checkout of a large repo (or one with LFS filters) can take minutes.
CHECKOUT_TIMEOUT = 600
GIT_TIMEOUT = 60

Pr = namedtuple("Pr", "host owner repo number url")
# What a failed host call raises: a setup problem (an account that is not logged in, an
# unreadable workspace.toml) or the host's own error.
ERRORS = (RuntimeError, Problem)


def parse(url):
    """The PR a link names, or None. `bitbucket.org/<workspace>/<slug>/pull-requests/<n>` is
    Bitbucket; `<host>/<owner>/<repo>/pull/<n>` on any other host is GitHub (Enterprise too).
    Host case, a port, user info, a query, a #fragment and anything after the number (/diff,
    /files) are ignored, so a link copied from the browser works."""
    try:
        parts = urlparse((url or "").strip())
        host = parts.hostname or ""
    except ValueError:
        return None
    if parts.scheme not in ("http", "https") or not host:
        return None
    segments = [s for s in parts.path.split("/") if s]
    marker = "pull-requests" if host == BITBUCKET else "pull"
    if marker not in segments:
        return None
    i = segments.index(marker)
    if i < 2 or i + 1 >= len(segments) or not re.fullmatch(r"[0-9]+", segments[i + 1]):
        return None
    return Pr(host, segments[i - 2], segments[i - 1], str(int(segments[i + 1])), url)


def pr_of(url):
    pr = parse(url)
    if pr is None:
        raise Problem(f"{url} is not a pull request link")
    return pr


def provider(pr, workspace):
    """The provider of a parsed PR; `workspace` is the workspace folder (its account and context)."""
    return (Bitbucket if pr.host == BITBUCKET else GitHub)(workspace)


def run(argv, *, env=None, timeout=TIMEOUT, cwd=None, stdin=None, ok=(0,)):
    """stdout of a command; RuntimeError with its message, never an empty one, when it fails."""
    name = os.path.basename(argv[0])
    try:
        # Without input stdin is /dev/null: a host CLI that prompts must fail, not wait forever.
        result = subprocess.run(argv, input=stdin, stdin=None if stdin is not None else subprocess.DEVNULL,
                                capture_output=True, text=True, timeout=timeout, cwd=cwd, env=env)
    except subprocess.TimeoutExpired:
        raise RuntimeError(f"{name} {argv[1]} timed out after {timeout}s")
    except OSError as e:
        raise RuntimeError(f"cannot run {name}: {e}")
    if result.returncode not in ok:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip() or f"{name} {argv[1]} exited with {result.returncode}")
    return result.stdout


def git(cwd, *args, timeout=GIT_TIMEOUT):
    return run(["git", *args], cwd=cwd, timeout=timeout)


def by_provider(urls, workspace):
    """{provider class: [url]} and {url: error} for the links that name no PR."""
    groups, bad = {}, {}
    for url in urls:
        pr = parse(url)
        if pr is None:
            bad[url] = RuntimeError(f"{url} is not a pull request link")
        else:
            groups.setdefault(type(provider(pr, workspace)), []).append(url)
    return groups, bad


def read_pulls(urls, workspace):
    """Each PR as a GitHub GraphQL `PULL_FIELDS` node, read in batches per host and repo, and the
    reader's login per PR: ({url: node or error}, {url: login})."""
    groups, found = by_provider(urls, workspace)
    me = {}
    for cls, host_urls in groups.items():
        nodes, login = cls(workspace).read_pulls(host_urls)
        found.update(nodes)
        me.update({url: login for url in host_urls})
    return found, me


def read_threads(urls, workspace):
    """The inline review threads (a THREAD_FIELDS node) of each PR and, where the pull read
    carries no PR-level comments (Bitbucket), those too: ({url: node or error}, {url: [comments]})."""
    groups, threads = by_provider(urls, workspace)
    comments = {}
    for cls, host_urls in groups.items():
        found, tops = cls(workspace).read_threads(host_urls)
        threads.update(found)
        comments.update(tops)
    return threads, comments


def note(kind, id, author, body, at, state=None, path=None):
    """One entry of the PR conversation. Ids are the ones `pr-status` writes into
    `pr_unseen_comments`: `inline-<n>`, `review-<id>`, or the comment's own id."""
    return {"kind": kind, "id": id, "author": author, "body": body or "", "at": at or "", "state": state, "path": path}


# --- GitHub ----------------------------------------------------------------------------------

# NUMBER is the PR's number; isRequired needs it.
PULL_FIELDS = """number state isDraft mergeable reviewDecision headRefOid createdAt updatedAt
  commits(last: 1) { nodes { commit { committedDate statusCheckRollup { contexts(first: 100) { nodes {
    ... on CheckRun { name status conclusion startedAt completedAt isRequired(pullRequestNumber: NUMBER)
                      checkSuite { workflowRun { workflow { name } } } }
    ... on StatusContext { context state createdAt isRequired(pullRequestNumber: NUMBER) }
  } } } } } }
  comments(last: 100) { nodes { id createdAt body author { __typename login } } }
  reviews(last: 100) { nodes { id state submittedAt body author { __typename login } } }"""
THREAD_FIELDS = """reviewThreads(first: 100) { nodes { comments(first: 100) { nodes {
  databaseId body author { login } replyTo { databaseId } } } } }"""
MENTION_QUERY = """query($owner: String!, $repo: String!, $number: Int!) {
  repository(owner: $owner, name: $repo) { pullRequest(number: $number) {
    author { login }
    reviewRequests(first: 50) { nodes { requestedReviewer {
      __typename ... on User { login } ... on Team { combinedSlug } } } }
    latestReviews(first: 50) { nodes { state author { __typename login } } }
  } }
}"""
# PRs of a repo per GraphQL query: GitHub charges a query by the connections it asks for, not
# per PR, so a whole board costs about a point a run.
CHUNK = 25
UNKNOWN_RETRIES = 3
UNKNOWN_RETRY_WAIT = 2
RUN_LINK = re.compile(r"/actions/runs/(\d+)")


def is_bot(author):
    login = (author or {}).get("login") or ""
    return (author or {}).get("__typename") == "Bot" or login.endswith("[bot]")


def mentions_of(pull):
    """@-mentions for a GitHub PR (MENTION_QUERY's pullRequest): pending people, then earlier
    reviewers who did not approve; the requested teams only when that is nobody."""
    me = (pull.get("author") or {}).get("login")
    requested = [n["requestedReviewer"] for n in pull["reviewRequests"]["nodes"] if n.get("requestedReviewer")]
    people = {r["login"]: "" for r in requested if r["__typename"] == "User"}
    for review in pull["latestReviews"]["nodes"]:
        author = review.get("author")
        if not author or is_bot(author) or author["login"] == me:
            continue
        if review["state"] == "CHANGES_REQUESTED":
            people[author["login"]] = " (re-review after your feedback)"
        elif review["state"] == "COMMENTED":
            people.setdefault(author["login"], "")
    if people:
        return [f"@{login}{note}" for login, note in people.items()]
    return [f"@{r['combinedSlug']}" for r in requested if r["__typename"] == "Team"]


class GitHub:
    def __init__(self, workspace):
        self.workspace = workspace

    def gh(self, *args, timeout=TIMEOUT, stdin=None, ok=(0,)):
        return run([tracker.gh_path(), *args], env=tracker.gh_env(self.workspace), timeout=timeout, stdin=stdin, ok=ok)

    def graphql(self, query):
        """The data of a GraphQL query and the messages of its partial errors (a missing PR, say):
        gh exits 1 on those but still prints the data it got."""
        try:
            result = subprocess.run([tracker.gh_path(), "api", "graphql", "-f", f"query={query}"], stdin=subprocess.DEVNULL,
                                    capture_output=True, text=True, timeout=60, env=tracker.gh_env(self.workspace))
        except subprocess.TimeoutExpired:
            raise RuntimeError("gh api graphql timed out after 60s")
        except OSError as e:
            raise RuntimeError(f"cannot run gh: {e}")
        try:
            body = json.loads(result.stdout or "{}")
        except ValueError:
            body = {}
        errors = [e.get("message", "") for e in body.get("errors") or []]
        if not body.get("data"):
            raise RuntimeError("; ".join(errors) or result.stderr.strip() or f"gh api graphql exited with {result.returncode}")
        return body["data"], errors

    def read_nodes(self, urls, fields):
        """The `fields` of each PR, CHUNK PRs of a repo per query: ({url: node or error}, my login)."""
        by_repo = {}
        for url in urls:
            pr = pr_of(url)
            by_repo.setdefault((pr.owner, pr.repo), {}).setdefault(int(pr.number), []).append(url)
        found, me = {}, None
        for (owner, repo), prs in by_repo.items():
            numbers = sorted(prs)
            for chunk in (numbers[i:i + CHUNK] for i in range(0, len(numbers), CHUNK)):
                pulls = " ".join(f"p{n}: pullRequest(number: {n}) {{ {fields.replace('NUMBER', str(n))} }}" for n in chunk)
                query = (f"query {{ viewer {{ login }} repository(owner: {json.dumps(owner)}, name: {json.dumps(repo)}) "
                         f"{{ {pulls} }} }}")
                try:
                    data, errors = self.graphql(query)
                except ERRORS as e:
                    found.update({url: e for n in chunk for url in prs[n]})
                    continue
                me = data["viewer"]["login"]
                for n in chunk:
                    node = (data.get("repository") or {}).get(f"p{n}")
                    found.update({url: node or RuntimeError("; ".join(errors) or "no such PR") for url in prs[n]})
        return found, me

    def read_pulls(self, urls):
        """GitHub says UNKNOWN while it computes mergeability; asking again a moment later helps."""
        pulls, me = self.read_nodes(urls, PULL_FIELDS)
        for _ in range(UNKNOWN_RETRIES):
            computing = [u for u, n in pulls.items() if isinstance(n, dict) and n["state"] == "OPEN" and n["mergeable"] == "UNKNOWN"]
            if not computing:
                break
            time.sleep(UNKNOWN_RETRY_WAIT)
            again, _ = self.read_nodes(computing, PULL_FIELDS)
            pulls.update({u: n for u, n in again.items() if isinstance(n, dict)})
        return pulls, me

    def read_threads(self, urls):
        """The pull read carries the PR-level comments, so only the threads."""
        return self.read_nodes(urls, THREAD_FIELDS)[0], {}

    def view(self, pr, fields):
        return json.loads(self.gh("pr", "view", pr.url, "--json", fields))

    def conversation(self, pr):
        """The description, the comments, the reviews with a verdict or text, and the inline comments."""
        raw = self.view(pr, "body,comments,reviews")
        login = lambda a: (a or {}).get("login") or "ghost"
        notes = [note("comment", c.get("id"), login(c.get("author")), c.get("body"), c.get("createdAt")) for c in raw.get("comments") or []]
        for r in raw.get("reviews") or []:
            if r.get("state") in ("APPROVED", "CHANGES_REQUESTED") or r.get("body"):
                notes.append(note("review", f"review-{r.get('id')}", login(r.get("author")), r.get("body"), r.get("submittedAt"), state=r.get("state")))
        pages = json.loads(self.gh("api", f"repos/{pr.owner}/{pr.repo}/pulls/{pr.number}/comments", "--paginate", "--slurp"))
        for c in (c for page in pages for c in page):
            notes.append(note("inline", f"inline-{c.get('id')}", login(c.get("user")), c.get("body"), c.get("created_at"), path=c.get("path")))
        notes.sort(key=lambda n: n["at"], reverse=True)
        return {"body": raw.get("body") or "", "notes": notes}

    def mentions(self, pr):
        out = self.gh("api", "graphql", "-f", f"query={MENTION_QUERY}", "-f", f"owner={pr.owner}",
                      "-f", f"repo={pr.repo}", "-F", f"number={pr.number}")
        return mentions_of(json.loads(out)["data"]["repository"]["pullRequest"])

    def comment(self, pr, body):
        self.gh("pr", "comment", pr.url, "--body-file", "-", stdin=body)

    def reply(self, pr, comment_id, body):
        """An inline comment gets a reply in its thread; anything else a new PR comment."""
        if comment_id.startswith("inline-"):
            self.gh("api", f"repos/{pr.owner}/{pr.repo}/pulls/{pr.number}/comments/{comment_id[len('inline-'):]}/replies",
                    "-F", "body=@-", stdin=body)
        else:
            self.comment(pr, body)

    def checks(self, pr):
        """`[{name, state, link}]`, state one of pass, fail, pending, skipping, cancel."""
        # gh pr checks exits 1 when a check fails and 8 while one is pending.
        out = self.gh("pr", "checks", pr.url, "--json", "name,bucket,link", ok=(0, 1, 8))
        return [{"name": c.get("name") or "", "state": c.get("bucket") or "", "link": c.get("link") or ""} for c in json.loads(out or "[]")]

    def failed_log(self, pr):
        """The failed steps' log of every red workflow run; a red check outside Actions only by its link."""
        parts, runs = [], []
        for check in self.checks(pr):
            if check["state"] != "fail":
                continue
            match = RUN_LINK.search(check["link"])
            if not match:
                parts.append(f"## {check['name']}\n\nNot a GitHub Actions run, its log is at {check['link'] or 'its CI'}\n")
            elif match.group(1) not in runs:
                runs.append(match.group(1))
        for run_id in runs:
            log = self.gh("run", "view", run_id, "-R", f"{pr.owner}/{pr.repo}", "--log-failed", timeout=120)
            parts.append(f"## Run {run_id}\n\n{log}")
        return "\n".join(parts)

    def base(self, pr):
        return self.view(pr, "baseRefName")["baseRefName"]

    def state(self, pr):
        return self.view(pr, "state")["state"].lower()

    def ready(self, pr):
        self.gh("pr", "ready", pr.url)

    def add_reviewers(self, pr, reviewers):
        self.gh("pr", "edit", pr.url, "--add-reviewer", ",".join(reviewers))

    def merge(self, pr):
        self.gh("pr", "merge", pr.url, "--squash", timeout=60)

    def rebase(self, pr, repo):
        """Server-side: no local checkout is touched. GitHub refuses when the rebase conflicts."""
        self.gh("pr", "update-branch", pr.url, "--rebase", timeout=60)


# --- Bitbucket Cloud ---------------------------------------------------------------------------

# Cloud's states and build states in GitHub's words. "conflicted" and "not_mergeable" are the
# only merge states that mean a git conflict; the others are approvals or branch settings, not
# something a rebase fixes.
BB_STATES = {"OPEN": "OPEN", "MERGED": "MERGED", "DECLINED": "CLOSED", "SUPERSEDED": "CLOSED"}
BB_CHECKS = {"SUCCESSFUL": "SUCCESS", "FAILED": "FAILURE", "INPROGRESS": "IN_PROGRESS", "STOPPED": "CANCELLED"}
BB_BUCKETS = {"SUCCESSFUL": "pass", "FAILED": "fail", "INPROGRESS": "pending", "STOPPED": "cancel"}
BB_CONFLICTS = ("conflicted", "not_mergeable")


def bb_login(user):
    """A Cloud user's @-handle in the facts: the nickname, else the display name."""
    return (user or {}).get("nickname") or (user or {}).get("display_name") or ""


def bb_name(user):
    """A Cloud user as the PR tab shows them."""
    user = user or {}
    return user.get("display_name") or user.get("username") or user.get("nickname") or "ghost"


def bb_mention(user):
    """A mention the Cloud API notifies: @{account id}, the uuid without its braces as a
    fallback. A plain @nickname in an API comment notifies nobody."""
    id = (user or {}).get("account_id") or ((user or {}).get("uuid") or "").strip("{}")
    return f"@{{{id}}}" if id else ""


def bb_checks(statuses):
    """Cloud build statuses as GitHub check nodes, each grouped by its name so a re-run replaces
    the older result. Every status counts as required: Cloud has no per-check required flag."""
    checks = []
    for s in statuses or []:
        state = s.get("state")
        name = s.get("name") or s.get("key") or ""
        checks.append({"name": name,
                       "status": "COMPLETED" if state != "INPROGRESS" else "IN_PROGRESS",
                       "conclusion": BB_CHECKS.get(state) or ("FAILURE" if state else None),
                       "startedAt": s.get("created_on") or "", "completedAt": s.get("updated_on") or "",
                       "isRequired": True,
                       "checkSuite": {"workflowRun": {"workflow": {"name": name}}}})
    return checks


def bb_approvals(value):
    """The approvals of a Cloud PR, its author's own aside, as GitHub review nodes, timed by
    participated_on (null for a reviewer who has not acted: no time)."""
    author = ((value.get("author") or {}).get("user") or {}).get("uuid")
    reviews = []
    for p in value.get("participants") or []:
        user = p.get("user") or {}
        if user.get("uuid") == author or not p.get("approved"):
            continue
        reviews.append({"id": f"approval-{user.get('uuid')}", "state": "APPROVED", "body": "",
                        "submittedAt": p.get("participated_on") or "",
                        "author": {"__typename": "User", "login": bb_login(user)}, "name": bb_name(user)})
    return reviews


def bb_node(value, commit_date, checks):
    """A Cloud pull request as a PULL_FIELDS node: approvals become the reviews (Cloud has no
    changes-requested verdict), build statuses the checks. Its comments are None: the pull read
    carries none, and `pr-status` must leave the comment lists alone when they were not read."""
    reviews = bb_approvals(value)
    return {
        "number": value["id"],
        "state": BB_STATES.get(value.get("state"), value.get("state")),
        "isDraft": bool(value.get("draft")),
        "mergeable": "CONFLICTING" if value.get("merge_state") in BB_CONFLICTS else
                     ("MERGEABLE" if value.get("merge_state") else "UNKNOWN"),
        "reviewDecision": "APPROVED" if reviews else "",
        "headRefOid": ((value.get("source") or {}).get("commit") or {}).get("hash") or "",
        "createdAt": value.get("created_on") or "",
        "updatedAt": value.get("updated_on") or "",
        "commits": {"nodes": [{"commit": {"committedDate": commit_date or "",
                                          "statusCheckRollup": {"contexts": {"nodes": checks}} if checks else None}}]},
        "comments": None,
        "reviews": {"nodes": reviews},
    }


def bb_branch(value, side):
    name = (((value.get(side) or {}).get("branch")) or {}).get("name")
    if not name:
        raise RuntimeError(f"the pull request has no {side} branch")
    return name


class Bitbucket:
    def __init__(self, workspace):
        self.workspace = workspace

    def context(self):
        return os.environ.get("WORKITEM_BKT_CONTEXT") or (tracker.settings(self.workspace).get("bkt_context") if self.workspace else None)

    def bkt(self, *args, timeout=TIMEOUT):
        argv = [os.environ.get("WORKITEM_BKT", "bkt"), *args]
        if context := self.context():
            argv += ["--context", context]
        return run(argv, timeout=timeout)

    def pr_args(self, pr):
        return [pr.number, "--workspace", pr.owner, "--repo", pr.repo]

    def api(self, path, params=()):
        """One Cloud REST call through `bkt api`: the JSON body."""
        out = self.bkt("api", path, "--json", *[arg for key, value in params for arg in ("--param", f"{key}={value}")], timeout=60)
        try:
            return json.loads(out or "{}")
        except ValueError:
            raise RuntimeError(f"bkt printed no JSON for {path}")

    def paged(self, path, params):
        """Every value of a paginated list endpoint, following its next links."""
        values = []
        while path:
            data = self.api(path, params)
            values += data.get("values") or []
            next_url = data.get("next") or ""
            path = urlparse(next_url).path if next_url else ""
            params = parse_qsl(urlparse(next_url).query) if next_url else []
        return values

    def pull(self, pr):
        return self.api(f"/repositories/{pr.owner}/{pr.repo}/pullrequests/{pr.number}")

    def comments(self, pr):
        """The comments of the PR, deleted ones left out (Cloud keeps them in the list)."""
        values = self.paged(f"/repositories/{pr.owner}/{pr.repo}/pullrequests/{pr.number}/comments", [("pagelen", "100")])
        return [c for c in values if not c.get("deleted")]

    def statuses(self, pr, head):
        return self.paged(f"/repositories/{pr.owner}/{pr.repo}/commit/{head}/statuses", [("pagelen", "100")])

    def read_pulls(self, urls):
        """One list call per repo for the open PRs, one per-PR call for anything the list lacks
        (merged, declined) or that misses a field the facts need, one per head commit for its
        date, one per PR for its build statuses. ({url: node or error}, my nickname)."""
        found, me = {}, None
        by_repo = {}
        for url in urls:
            pr = pr_of(url)
            # One PR can sit behind two links (a query or /diff suffix): both get the same reading.
            by_repo.setdefault((pr.owner, pr.repo), {}).setdefault(int(pr.number), []).append(pr)
        for (ws, slug), prs in by_repo.items():
            urls_of = lambda n: [p.url for p in prs[n]]
            try:
                if me is None:
                    me = bb_login(self.api("/user"))
                listed = {p["id"]: p for p in self.paged(f"/repositories/{ws}/{slug}/pullrequests", [("state", "OPEN"), ("pagelen", "50")])}
            except ERRORS as e:
                found.update({url: e for n in prs for url in urls_of(n)})
                continue
            values = {}
            for n in prs:
                value = listed.get(n)
                if value is None or not value.get("participants") or "merge_state" not in value:
                    try:
                        value = self.pull(prs[n][0])
                    except ERRORS as e:
                        found.update({url: e for url in urls_of(n)})
                        continue
                values[n] = value
            head_of = lambda n: ((values[n].get("source") or {}).get("commit") or {}).get("hash") or ""
            dates = {}
            # One call per head: a `commits?include=` list would page through the repo's whole
            # history, hundreds of calls on a large repo.
            for head in sorted({head_of(n) for n, v in values.items() if v.get("state") == "OPEN"} - {""}):
                try:
                    dates[head] = self.api(f"/repositories/{ws}/{slug}/commit/{head}").get("date")
                except ERRORS as e:
                    dates[head] = e
            for n in values:
                head = head_of(n)
                if isinstance(dates.get(head), Exception):
                    found.update({url: dates[head] for url in urls_of(n)})
                    continue
                if values[n].get("state") != "OPEN":
                    found.update({url: bb_node(values[n], None, []) for url in urls_of(n)})
                    continue
                try:
                    checks = bb_checks(self.statuses(prs[n][0], head)) if head else []
                except ERRORS as e:
                    found.update({url: e for url in urls_of(n)})
                    continue
                found.update({url: bb_node(values[n], dates.get(head), checks) for url in urls_of(n)})
        return found, me

    def read_threads(self, urls):
        """The inline comments as one-comment threads (a reply names its parent) and the
        PR-level comments as GitHub comment nodes, split from the one comments endpoint."""
        threads, comments = {}, {}
        for url in urls:
            try:
                values = self.comments(pr_of(url))
            except ERRORS as e:
                threads[url] = e
                continue
            top, inline = [], []
            for c in values:
                login = bb_login(c.get("user"))
                body = (c.get("content") or {}).get("raw") or ""
                if c.get("inline"):
                    inline.append({"databaseId": c["id"], "body": body, "author": {"login": login},
                                   "replyTo": {"databaseId": (c.get("parent") or {}).get("id")} if c.get("parent") else None})
                else:
                    top.append({"id": str(c["id"]), "createdAt": c.get("created_on") or "", "body": body,
                                "author": {"__typename": "User", "login": login}})
            threads[url] = {"reviewThreads": {"nodes": [{"comments": {"nodes": [c]}} for c in inline]}}
            comments[url] = top
        return threads, comments

    def conversation(self, pr):
        value = self.pull(pr)
        # The PR's update time would pin an old approval to a later push, so only without its own.
        notes = [note("review", r["id"], r["name"], "", r["submittedAt"] or value.get("updated_on"), state="APPROVED")
                 for r in bb_approvals(value)]
        for c in self.comments(pr):
            inline = c.get("inline")
            notes.append(note("inline" if inline else "comment", f"inline-{c['id']}" if inline else str(c["id"]),
                              bb_name(c.get("user")), (c.get("content") or {}).get("raw"), c.get("created_on"),
                              path=(inline or {}).get("path")))
        notes.sort(key=lambda n: n["at"], reverse=True)
        return {"body": value.get("description") or "", "notes": notes}

    def mentions(self, pr):
        """The reviewers who have not approved. Cloud has no changes-requested verdict, so there
        is no re-review note."""
        value = self.pull(pr)
        approved = {(p.get("user") or {}).get("uuid") for p in value.get("participants") or [] if p.get("approved")}
        who = []
        for r in value.get("reviewers") or []:
            user = r.get("user") or r
            mention = bb_mention(user)
            if user.get("uuid") not in approved and mention and mention not in who:
                who.append(mention)
        return who

    def comment(self, pr, body):
        self.bkt("pr", "comment", *self.pr_args(pr), "--text", body)

    def reply(self, pr, comment_id, body):
        """Every Cloud comment takes a threaded reply."""
        parent = comment_id[len("inline-"):] if comment_id.startswith("inline-") else comment_id
        if not parent.isdigit():
            raise Problem(f"'{comment_id}' is not a Bitbucket comment id")
        self.bkt("pr", "comment", *self.pr_args(pr), "--text", body, "--parent", parent)

    def checks(self, pr):
        head = ((self.pull(pr).get("source") or {}).get("commit") or {}).get("hash")
        return [{"name": s.get("name") or s.get("key") or "", "state": BB_BUCKETS.get(s.get("state"), "fail"), "link": s.get("url") or ""}
                for s in (self.statuses(pr, head) if head else [])]

    def failed_log(self, pr):
        """Cloud build statuses carry a link to the CI, not a log."""
        return "\n".join(f"## {c['name']}\n\nIts log is at {c['link'] or 'its CI'}\n" for c in self.checks(pr) if c["state"] == "fail")

    def base(self, pr):
        return bb_branch(self.pull(pr), "destination")

    def state(self, pr):
        state = self.pull(pr).get("state")
        return BB_STATES.get(state, state or "").lower()

    def ready(self, pr):
        self.bkt("pr", "publish", *self.pr_args(pr))

    def add_reviewers(self, pr, reviewers):
        self.bkt("pr", "edit", *self.pr_args(pr), *[arg for r in reviewers for arg in ("--reviewer", r)])

    def merge(self, pr):
        self.bkt("pr", "merge", *self.pr_args(pr), "--strategy", "squash", timeout=60)

    def rebase(self, pr, repo):
        """Cloud has no server-side rebase: rebase locally and force-push with lease. It runs in a
        temporary detached worktree, never in the repo's own checkout (a rebase there would detach
        its HEAD and fail on uncommitted changes); a conflict aborts it and nothing is left behind."""
        value = self.pull(pr)
        source, destination = bb_branch(value, "source"), bb_branch(value, "destination")
        git(repo, "fetch", "origin", destination, source)
        worktree = tempfile.mkdtemp(prefix="wi-rebase-")
        os.rmdir(worktree)
        try:
            git(repo, "worktree", "add", "--detach", worktree, f"origin/{source}", timeout=CHECKOUT_TIMEOUT)
            try:
                git(worktree, "rebase", f"origin/{destination}")
            except RuntimeError:
                try:
                    git(worktree, "rebase", "--abort")
                except RuntimeError:
                    pass
                raise
            git(worktree, "push", "--force-with-lease", "origin", f"HEAD:{source}")
        finally:
            try:
                git(repo, "worktree", "remove", "--force", worktree)
            except RuntimeError:
                shutil.rmtree(worktree, ignore_errors=True)
                try:
                    git(repo, "worktree", "prune")
                except RuntimeError:
                    pass
