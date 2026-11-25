import json, os, subprocess, sys, tempfile, threading, unittest
from http.server import BaseHTTPRequestHandler, HTTPServer

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
BIN = os.path.join(HERE, "skills/workitem-pipeline/bin")
ISSUE = os.path.join(BIN, "workitem-issue")
MOVE = os.path.join(BIN, "workitem-move")
STATUS = os.path.join(HERE, "skills/workitem-pipeline/watch/issue-status")

# A stand-in for gh: logs each call (argv and GH_TOKEN) and answers from a table of argv prefixes.
FAKE_GH = """#!/usr/bin/env python3
import json, os, sys
here = os.path.dirname(os.path.abspath(__file__))
args = sys.argv[1:]
with open(os.path.join(here, "calls.jsonl"), "a") as f:
    f.write(json.dumps({"args": args, "token": os.environ.get("GH_TOKEN", "")}) + "\\n")
with open(os.path.join(here, "answers.json")) as f:
    answers = json.load(f)
for prefix, (out, code) in answers:
    if args[:len(prefix)] == prefix:
        sys.stdout.write(out)
        sys.exit(code)
sys.exit(0)
"""


class Jira(BaseHTTPRequestHandler):
    requests = []
    status = "To Do"
    comments = []
    fail = None
    transitions = [{"id": "31", "to": {"name": "In Progress"}}, {"id": "41", "to": {"name": "In Review"}}]

    def answer(self, data, code=200):
        body = json.dumps(data).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        Jira.requests.append(("GET", self.path, None))
        if self.path.endswith("/transitions"):
            self.answer({"transitions": Jira.transitions})
        else:
            self.answer({"fields": {"status": {"name": Jira.status}, "summary": "Fix it", "description": None, "comment": {"comments": Jira.comments}}})

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        Jira.requests.append(("POST", self.path, body))
        if Jira.fail:
            self.answer({"errorMessages": [Jira.fail]}, 400)
        elif self.path == "/rest/api/3/search/jql" and "summary" in body["fields"]:
            def issue(key, labels, summary):
                return {"key": key, "fields": {"status": {"name": "To Do"}, "labels": labels, "summary": summary}}
            self.answer({"issues": [issue("MNT-1", [], "Login\tfails"), issue("MNT-2", ["ui", "jira_escalated"], "Login slow"), issue("MNT-3", [], "Login 500")]})
        elif self.path == "/rest/api/3/search/jql":
            self.answer({"issues": [{"key": "MNT-1", "fields": {"status": {"name": "Ready", "statusCategory": {"key": "indeterminate"}}}}]})
        elif self.path == "/rest/api/3/issue":
            self.answer({"key": "MNT-9"}, 201)
        else:
            self.send_response(204)
            self.end_headers()

    def log_message(self, *args):
        pass


class WorkitemIssueTest(unittest.TestCase):
    def setUp(self):
        Jira.requests, Jira.status, Jira.comments, Jira.fail = [], "To Do", [], None
        self.server = HTTPServer(("127.0.0.1", 0), Jira)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.tmp = tempfile.TemporaryDirectory()
        self.root = self.tmp.name
        self.workspace = os.path.join(self.root, "ws")
        self.repo = os.path.join(self.root, "repo")
        os.makedirs(os.path.join(self.workspace, "boards"))
        os.makedirs(self.repo)
        self.bin = os.path.join(self.root, "bin")
        os.makedirs(self.bin)
        self.gh = os.path.join(self.bin, "gh")
        with open(self.gh, "w") as f:
            f.write(FAKE_GH)
        os.chmod(self.gh, 0o755)
        self.answers([])
        self.jira_config = os.path.join(self.root, "jira.yml")
        with open(self.jira_config, "w") as f:
            f.write(f"login: me@example.com\nserver: http://127.0.0.1:{self.server.server_port}\n")

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.tmp.cleanup()

    def answers(self, table):
        with open(os.path.join(self.bin, "answers.json"), "w") as f:
            json.dump(table, f)

    def calls(self):
        path = os.path.join(self.bin, "calls.jsonl")
        if not os.path.exists(path):
            return []
        with open(path) as f:
            return [json.loads(line) for line in f]

    def setup(self, tracker, board_extra=None, account=None, stages=None):
        with open(os.path.join(self.workspace, "workspace.toml"), "w") as f:
            f.write(f'issue_tracker = "{tracker}"\n' + (f'github_account = "{account}"\n' if account else ""))
        board = {"repo": self.repo, "entry": "a", "brief_sections": [], **(board_extra or {}),
                 "stages": stages or [{"name": "a", "next": ["b"]}, {"name": "b", "next": []}]}
        with open(os.path.join(self.workspace, "boards", "dev.json"), "w") as f:
            json.dump(board, f)

    def item(self, name, issue="none", extra=""):
        os.makedirs(os.path.join(self.workspace, name))
        path = os.path.join(self.workspace, name, "workitem.md")
        with open(path, "w") as f:
            f.write(f"---\ntitle: {name}\nboard: dev\nstage: a\nissue: {issue}\n{extra}---\nbody\n")
        return path

    def read(self, path):
        with open(path) as f:
            return f.read()

    def run_script(self, script, *args, stdin=None, **env):
        base = {"PATH": f"{self.bin}:{os.environ['PATH']}", "HOME": self.root, "JIRA_CONFIG_FILE": self.jira_config,
                "JIRA_API_TOKEN": "secret", "WORKITEM_GH": self.gh}
        return subprocess.run([sys.executable, script, *args], input=stdin, capture_output=True, text=True, env={**base, **env}, timeout=60)

    # parse

    def test_parse_expands_github_refs_and_refuses_jira_keys(self):
        self.setup("github", {"issue_project": "acme/web"})
        path = self.item("x")
        for given in ["#12", "12", "acme/web#12", "https://github.com/acme/web/issues/12"]:
            result = self.run_script(ISSUE, "parse", path, given)
            self.assertEqual(result.stdout, "issue: acme/web#12 (https://github.com/acme/web/issues/12)\nprefix: web-12\n", given)
        result = self.run_script(ISSUE, "parse", path, "MNT-1")
        self.assertEqual(result.returncode, 2)
        self.assertIn("this workspace uses GitHub", result.stderr)

    def test_the_folder_prefix_keeps_only_letters_digits_and_hyphens(self):
        self.setup("github")
        result = self.run_script(ISSUE, "parse", "--workspace", self.workspace, "--board", "dev", "vercel/next.js#42")
        self.assertEqual(result.stdout.splitlines()[1], "prefix: next-js-42", "tmux turns . into _ in session names")

    def test_a_jira_url_needs_a_server(self):
        self.setup("jira")
        with open(self.jira_config, "w") as f:
            f.write("login: me@example.com\n")
        self.assertEqual(self.run_script(ISSUE, "parse", self.item("x"), "MNT-7").stdout, "issue: MNT-7\nprefix: MNT-7\n")

    def test_an_issue_of_the_other_tracker_is_refused(self):
        self.setup("github")
        path = self.item("x", "SBD-123 (https://jira/browse/SBD-123)")
        for command in (["pr-line"], ["read"], ["set-state", "assigned"]):
            result = self.run_script(ISSUE, command[0], path, *command[1:])
            self.assertEqual(result.returncode, 2, command)
            self.assertIn("is not a GitHub issue", result.stderr)
        self.assertEqual(self.calls(), [])

    def test_parse_without_issue_project_asks_gh_for_the_boards_repo(self):
        self.setup("github")
        self.answers([[["repo", "view"], ["acme/api\n", 0]]])
        result = self.run_script(ISSUE, "parse", "--workspace", self.workspace, "--board", "dev", "#3")
        self.assertEqual(result.stdout.splitlines()[0], "issue: acme/api#3 (https://github.com/acme/api/issues/3)")

    def test_parse_reads_jira_keys_and_urls_and_refuses_github_refs(self):
        self.setup("jira")
        path = self.item("x")
        url = f"http://127.0.0.1:{self.server.server_port}/browse/MNT-7"
        for given in ["MNT-7", url]:
            self.assertEqual(self.run_script(ISSUE, "parse", path, given).stdout, f"issue: MNT-7 ({url})\nprefix: MNT-7\n")
        self.assertEqual(self.run_script(ISSUE, "parse", path, "acme/web#1").returncode, 2)

    def test_read_prints_a_github_issue_before_any_item_exists(self):
        self.setup("github", {"issue_project": "acme/web"})
        comment = {"id": "IC_1", "createdAt": "2026-11-20T10:00:00Z", "author": {"login": "ann"}, "body": "Seen too"}
        issue = {"title": "Fix it", "state": "OPEN", "body": "Steps", "comments": [comment], "assignees": []}
        self.answers([[["issue", "view", "5", "-R", "acme/web"], [json.dumps(issue), 0]]])
        result = self.run_script(ISSUE, "read", "--workspace", self.workspace, "--board", "dev", "#5")
        self.assertEqual(result.stdout, "# acme/web#5: Fix it\n\nStatus: Open\n\nSteps\n\n## Comment by ann\n\n"
                                        "Posted 2026-11-20T10:00:00Z · id IC_1\n\nSeen too\n")

    def test_read_prints_jira_comments_with_time_and_id_under_the_same_heading(self):
        self.setup("jira")
        body = {"type": "doc", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Deployed"}]}]}
        Jira.comments = [{"id": "10001", "created": "2026-11-20T10:00:00.000+0100", "author": {"displayName": "Ann Lee"}, "body": body}]
        result = self.run_script(ISSUE, "read", self.item("x", "MNT-1"))
        self.assertIn("\n## Comment by Ann Lee\n\nPosted 2026-11-20T10:00:00.000+0100 · id 10001\n\nDeployed\n", result.stdout)

    # search

    def test_search_jira_sorts_escalated_first(self):
        self.setup("jira", {"issue_project": "SUP"})
        result = self.run_script(ISSUE, "search", "--workspace", self.workspace, "--board", "dev", "login", 'say "hi"\\')
        self.assertEqual(result.returncode, 0, result.stderr)
        jql = Jira.requests[-1][2]["jql"]
        self.assertEqual(jql, 'project = SUP AND statusCategory != Done AND text ~ "login say \\"hi\\"\\\\"')
        self.assertEqual(result.stdout, "MNT-2\tTo Do\tui,jira_escalated\tLogin slow\nMNT-1\tTo Do\t\tLogin fails\nMNT-3\tTo Do\t\tLogin 500\n")

    def test_search_github_lists_open_issues(self):
        self.setup("github", {"issue_project": "acme/web"})
        issues = [{"number": 4, "title": "Crash", "labels": [], "assignees": [{"login": "me"}]},
                  {"number": 9, "title": "Crash on save", "labels": [{"name": "jira_escalated"}], "assignees": []}]
        self.answers([[["issue", "list"], [json.dumps(issues), 0]]])
        result = self.run_script(ISSUE, "search", self.item("x"), "crash", "save")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.calls()[0]["args"], ["issue", "list", "-R", "acme/web", "--state", "open", "--search", "crash save",
                                                   "--limit", "100", "--json", "number,title,labels,assignees"])
        self.assertEqual(result.stdout, "acme/web#9\tOpen\tjira_escalated\tCrash on save\nacme/web#4\tIn progress\t\tCrash\n")

    def test_search_needs_words_and_a_jira_issue_project(self):
        self.setup("jira")
        path = self.item("x")
        self.assertEqual(self.run_script(ISSUE, "search", path).returncode, 2)
        result = self.run_script(ISSUE, "search", path, "login")
        self.assertEqual(result.returncode, 2)
        self.assertIn("set issue_project", result.stderr)
        self.assertEqual(Jira.requests, [])

    # comment

    def comment_file(self, text):
        path = os.path.join(self.root, "comment.md")
        with open(path, "w") as f:
            f.write(text)
        return path

    def test_comment_posts_adf_paragraphs_on_jira(self):
        self.setup("jira")
        result = self.run_script(ISSUE, "comment", self.item("x", "MNT-1"), "--body-file", self.comment_file("One\n\nTwo\n"))
        self.assertEqual(result.returncode, 0, result.stderr)
        paragraphs = [{"type": "paragraph", "content": [{"type": "text", "text": t}]} for t in ("One", "Two\n")]
        self.assertEqual(Jira.requests[-1], ("POST", "/rest/api/3/issue/MNT-1/comment", {"body": {"type": "doc", "version": 1, "content": paragraphs}}))

    def test_comment_on_github(self):
        self.setup("github")
        result = self.run_script(ISSUE, "comment", self.item("x", "acme/web#7"), "--body-file", self.comment_file("Fixed"))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.calls()[0]["args"], ["issue", "comment", "7", "-R", "acme/web", "--body", "Fixed"])

    def test_comment_without_an_issue_or_with_a_bad_ref_exits_2(self):
        self.setup("github")
        body = self.comment_file("x")
        for name, issue in (("none", "none"), ("jira", "MNT-1")):
            self.assertEqual(self.run_script(ISSUE, "comment", self.item(name, issue), "--body-file", body).returncode, 2, name)
        self.assertEqual(self.calls(), [])

    def test_a_failed_comment_exits_nonzero(self):
        self.setup("jira")
        Jira.fail = "no permission"
        result = self.run_script(ISSUE, "comment", self.item("x", "MNT-1"), "--body-file", self.comment_file("x"))
        self.assertEqual(result.returncode, 1)
        self.assertIn("Jira answered 400", result.stderr)
        self.setup("github")
        self.answers([[["issue", "comment"], ["", 1]]])
        self.assertEqual(self.run_script(ISSUE, "comment", self.item("y", "acme/web#7"), "--body-file", self.comment_file("x")).returncode, 1)

    # set-state

    def test_github_assigns_me_once(self):
        self.setup("github")
        path = self.item("x", "acme/web#7 (https://github.com/acme/web/issues/7)")
        self.answers([[["issue", "view"], ['{"state": "OPEN", "assignees": []}', 0]], [["api", "user"], ["me\n", 0]]])
        result = self.run_script(ISSUE, "set-state", path, "assigned")
        self.assertEqual(result.stdout.strip(), "acme/web#7: assigned to me")
        self.assertIn(["issue", "edit", "7", "-R", "acme/web", "--add-assignee", "@me"], [c["args"] for c in self.calls()])
        os.remove(os.path.join(self.bin, "calls.jsonl"))
        self.answers([[["issue", "view"], ['{"state": "OPEN", "assignees": [{"login": "me"}]}', 0]], [["api", "user"], ["me\n", 0]]])
        self.run_script(ISSUE, "set-state", path, "assigned")
        self.assertNotIn("edit", [c["args"][1] for c in self.calls()])

    def test_github_refuses_jira_states(self):
        self.setup("github")
        path = self.item("x", "acme/web#7")
        self.assertEqual(self.run_script(ISSUE, "set-state", path, "In Review").returncode, 2)

    def test_skips_without_an_issue_and_when_another_item_depends_on_it(self):
        self.setup("github")
        self.assertIn("no issue linked", self.run_script(ISSUE, "set-state", self.item("none"), "assigned").stdout)
        path = self.item("api", "acme/web#7")
        self.item("ui", "acme/web#7", "depends_on: api\n")
        self.assertIn("another item depends on it", self.run_script(ISSUE, "set-state", path, "assigned").stdout)
        self.assertEqual(self.calls(), [])

    def test_jira_takes_the_first_state_with_a_transition(self):
        self.setup("jira")
        path = self.item("x", "MNT-1 (https://jira/browse/MNT-1)")
        result = self.run_script(ISSUE, "set-state", path, "In development", "In Progress")
        self.assertEqual(result.stdout.strip(), "MNT-1: To Do -> In Progress")
        self.assertIn(("POST", "/rest/api/3/issue/MNT-1/transitions", {"transition": {"id": "31"}}), Jira.requests)

    def test_jira_already_there_or_without_a_transition_warns_and_exits_zero(self):
        self.setup("jira")
        path = self.item("x", "MNT-1")
        Jira.status = "In Review"
        self.assertIn("already In Review", self.run_script(ISSUE, "set-state", path, "In Review").stdout)
        Jira.status = "To Do"
        result = self.run_script(ISSUE, "set-state", path, "Done")
        self.assertEqual(result.returncode, 0)
        self.assertIn("no transition to Done (available: In Progress, In Review)", result.stderr)

    # create, pr-line, accounts

    def test_create_files_a_github_issue_and_links_it(self):
        self.setup("github", {"issue_project": "acme/web"})
        path = self.item("x")
        body = os.path.join(self.root, "body.md")
        with open(body, "w") as f:
            f.write("Details")
        self.answers([[["issue", "create"], ["https://github.com/acme/web/issues/42\n", 0]]])
        result = self.run_script(ISSUE, "create", path, "--title", "Fix it", "--body-file", body)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.calls()[0]["args"], ["issue", "create", "-R", "acme/web", "--title", "Fix it", "--body", "Details"])
        self.assertIn("issue: acme/web#42 (https://github.com/acme/web/issues/42)\n", self.read(path))
        self.assertEqual(self.run_script(ISSUE, "pr-line", path).stdout, "Closes acme/web#42\n")

    def test_create_in_jira_needs_issue_project(self):
        self.setup("jira")
        path = self.item("x")
        body = os.path.join(self.root, "body.md")
        with open(body, "w") as f:
            f.write("Details")
        result = self.run_script(ISSUE, "create", path, "--title", "T", "--body-file", body)
        self.assertEqual(result.returncode, 2)
        self.assertIn("set issue_project", result.stderr)
        self.setup("jira", {"issue_project": "MNT"})
        self.assertEqual(self.run_script(ISSUE, "create", path, "--title", "T", "--body-file", body).returncode, 0)
        self.assertIn("issue: MNT-9 (", self.read(path))
        self.assertEqual(self.run_script(ISSUE, "pr-line", path).stdout, "MNT-9\n")

    def test_the_workspace_account_is_used_unless_the_caller_set_a_token(self):
        self.setup("github", account="work-bot")
        path = self.item("x", "acme/web#7")
        self.answers([[["auth", "token", "--user", "work-bot"], ["tok-123\n", 0]], [["issue", "view"], ['{"state": "CLOSED", "assignees": []}', 0]]])
        self.run_script(ISSUE, "set-state", path, "closed")
        self.assertEqual(self.calls()[-1]["token"], "tok-123")
        self.run_script(ISSUE, "set-state", path, "closed", GH_TOKEN="from-backend")
        self.assertEqual(self.calls()[-1]["token"], "from-backend")
        self.answers([[["auth", "token"], ["", 1]]])
        result = self.run_script(ISSUE, "set-state", path, "closed")
        self.assertIn("GitHub account work-bot is not logged in", result.stderr)

    # issue-status and workitem-move

    def test_issue_status_reads_github_issues_per_repo(self):
        self.setup("github")
        a, b = self.item("a", "acme/web#1"), self.item("b", "acme/web#2 (https://github.com/acme/web/issues/2)")
        self.item("c", "MNT-1")
        data = {"data": {"repository": {"i1": {"state": "OPEN", "assignees": {"totalCount": 1}}, "i2": {"state": "CLOSED", "assignees": {"totalCount": 0}}}}}
        self.answers([[["api", "graphql"], [json.dumps(data), 0]]])
        items = [{"dir": d, "issue": i} for d, i in [("a", "acme/web#1"), ("b", "acme/web#2 (x)"), ("c", "MNT-1")]]
        result = self.run_script(STATUS, stdin=json.dumps(items), WORKITEM_DIR_ROOT=self.workspace)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.calls()), 1)
        self.assertIn("issue_status: In progress\nissue_status_category: indeterminate\n", self.read(a))
        self.assertIn("issue_status: Closed\nissue_status_category: done\n", self.read(b))
        self.assertEqual(Jira.requests, [], "a Jira key in a GitHub workspace is left alone")

    def test_issue_status_keeps_the_issues_github_found_when_one_is_gone(self):
        self.setup("github")
        a = self.item("a", "acme/web#1")
        self.item("b", "acme/web#2")
        data = {"data": {"repository": {"i1": {"state": "CLOSED", "assignees": {"totalCount": 0}}, "i2": None}}, "errors": [{"message": "Could not resolve"}]}
        self.answers([[["api", "graphql"], [json.dumps(data), 1]]])
        items = [{"dir": "a", "issue": "acme/web#1"}, {"dir": "b", "issue": "acme/web#2"}]
        self.assertEqual(self.run_script(STATUS, stdin=json.dumps(items), WORKITEM_DIR_ROOT=self.workspace).returncode, 0)
        self.assertIn("issue_status: Closed\n", self.read(a))

    def test_issue_status_reads_the_other_repos_when_one_fails(self):
        self.setup("github")
        a = self.item("a", "acme/api#1")
        self.item("b", "acme/web#2")
        data = {"data": {"repository": {"i1": {"state": "CLOSED", "assignees": {"totalCount": 0}}}}}
        # The fake answers by argv prefix: the web repo's query fails, the api repo's is read.
        self.answers([[["api", "graphql", "-f", "query=query($owner: String!, $name: String!) { repository(owner: $owner, name: $name) { i2: issue(number: 2) { state assignees(first: 1) { totalCount } } } }"], ["", 1]],
                      [["api", "graphql"], [json.dumps(data), 0]]])
        items = [{"dir": "a", "issue": "acme/api#1"}, {"dir": "b", "issue": "acme/web#2"}]
        result = self.run_script(STATUS, stdin=json.dumps(items), WORKITEM_DIR_ROOT=self.workspace)
        self.assertEqual(result.returncode, 0)
        self.assertIn("cannot read the issues of acme/web", result.stderr)
        self.assertIn("issue_status: Closed\n", self.read(a))

    def test_a_move_sets_the_stages_issue_state_and_stands_when_it_fails(self):
        self.setup("github", stages=[{"name": "a", "next": ["b"]}, {"name": "b", "next": [], "issue_state": ["closed"]}])
        path = self.item("x", "acme/web#7")
        self.answers([[["issue", "view"], ['{"state": "OPEN", "assignees": []}', 0]]])
        result = self.run_script(MOVE, path, "b")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(["issue", "close", "7", "-R", "acme/web"], [c["args"] for c in self.calls()])
        self.assertIn("stage: b", self.read(path))
        other = self.item("y", "acme/web#8")
        self.answers([[["issue", "view"], ["", 1]]])
        result = self.run_script(MOVE, other, "b")
        self.assertEqual(result.returncode, 0)
        self.assertIn("warning", result.stderr)
        self.assertIn("stage: b", self.read(other))


if __name__ == "__main__":
    unittest.main()
