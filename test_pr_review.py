import contextlib, importlib.machinery, importlib.util, io, json, os, sys, tempfile, threading, unittest
from http.server import BaseHTTPRequestHandler, HTTPServer
from unittest import mock

sys.dont_write_bytecode = True

PATH = os.path.join(os.path.dirname(__file__), "skills/workitem-pipeline/watch/pr-review")
loader = importlib.machinery.SourceFileLoader("pr_review", PATH)
spec = importlib.util.spec_from_loader("pr_review", loader)
pr = importlib.util.module_from_spec(spec)
loader.exec_module(pr)

NOW = "2026-11-05T10:00:10Z"
OPEN = {"agent_status": "ok", "pr_state": "open", "pr_head": "h1", "pr_merge": "mergeable", "pr_ci_fix": "passing",
        "pr_checked_at": "2026-11-05T10:00:00Z"}


def decide(busy=False, **fields):
    return pr.decide({**OPEN, **fields}, busy, 3, NOW)


class StateTest(unittest.TestCase):
    def test_merged_moves_and_closed_blocks_once(self):
        self.assertEqual(decide(pr_state="merged"), ({}, ("move",)))
        self.assertEqual(decide(pr_state="closed"), ({"agent_status": "blocked: PR was closed without merge"}, None))
        self.assertEqual(decide(pr_state="closed", agent_status="blocked: x"), ({}, None))
        self.assertEqual(decide(pr_state="closed", busy=True), ({}, None))

    def test_nothing_before_the_first_reading_or_while_busy(self):
        self.assertEqual(pr.decide({"agent_status": "ok", "pr_merge": "conflicting"}, False, 3, NOW), ({}, None))
        self.assertEqual(decide(busy=True, pr_merge="conflicting"), ({}, None))
        self.assertEqual(decide(agent_status="working", pr_merge="conflicting"), ({}, None))
        self.assertEqual(decide(agent_status='"waiting: ci"', pr_merge="conflicting"), ({}, None))


class FixerTest(unittest.TestCase):
    def test_conflict_comes_before_red_ci_and_resets_the_streak(self):
        updates, action = decide(pr_merge="conflicting", pr_ci_fix="failing", pr_fix_attempts="2")
        self.assertEqual(action, ("fix", "conflict"))
        self.assertEqual(updates, {"pr_conflict_head": "h1", "pr_conflict_attempts": "1", "pr_fix_attempts": None, "pr_fixing": "conflict"})

    def test_a_conflict_on_the_same_head_retries_until_the_cap(self):
        self.assertEqual(decide(pr_merge="conflicting", pr_conflict_head="h1", pr_conflict_attempts="2"),
                         ({"pr_conflict_head": "h1", "pr_conflict_attempts": "3", "pr_fix_attempts": None, "pr_fixing": "conflict"},
                          ("fix", "conflict")))
        updates, action = decide(pr_merge="conflicting", pr_conflict_head="h1", pr_conflict_attempts="3")
        self.assertIsNone(action)
        self.assertTrue(updates["agent_status"].startswith("blocked: merge conflict still open after 3 conflict fixer runs"))
        self.assertEqual((updates["pr_conflict_head"], updates["pr_conflict_attempts"]), (None, None))
        self.assertEqual(decide(pr_merge="conflicting", pr_conflict_head="h0", pr_conflict_attempts="3")[0]["pr_conflict_attempts"], "1",
                         "a new head starts a new count")

    def test_red_ci_counts_attempts_and_stops_at_the_cap(self):
        self.assertEqual(decide(pr_ci_fix="failing", pr_fix_attempts="1"),
                         ({"pr_ci_head": "h1", "pr_fix_attempts": "2", "pr_fixing": "ci"}, ("fix", "ci")))
        self.assertEqual(decide(pr_ci_fix="failing", pr_fix_attempts="3"),
                         ({"agent_status": "blocked: CI still failing after 3 fix attempts"}, None))
        self.assertEqual(decide(pr_ci_fix="pending", pr_fix_attempts="1"), ({}, None), "running checks are left alone")
        self.assertEqual(decide(pr_fix_attempts="2", pr_ci_head="h0"), ({"pr_fix_attempts": None, "pr_ci_head": None}, None), "green ends the streak")

    def test_a_ci_fixer_that_left_the_head_as_it_was_blocks_the_item(self):
        self.assertEqual(decide(pr_ci_fix="failing", pr_ci_head="h1")[0]["agent_status"][:40],
                         "blocked: CI still failing and the CI fix")
        self.assertEqual(decide(pr_ci_fix="failing", pr_ci_head="h0", pr_fix_attempts="1")[1], ("fix", "ci"), "a new head gets a new try")

    def test_a_blocked_item_forgets_heads_and_attempts_so_clearing_retries(self):
        updates, action = decide(agent_status="blocked: x", pr_conflict_head="h1", pr_conflict_attempts="3", pr_ci_head="h1", pr_fix_attempts="3")
        self.assertEqual((updates, action), ({"pr_conflict_head": None, "pr_conflict_attempts": None, "pr_ci_head": None, "pr_fix_attempts": None}, None))

    def test_new_comments_go_to_one_fixer_and_become_seen(self):
        updates, action = decide(pr_seen_comments="a", pr_unseen_comments="a,b,c")
        self.assertEqual(action, ("fix", "comments"))
        self.assertEqual(updates, {"pr_seen_comments": "a,b,c", "pr_unseen_comments": "", "pr_fixing": "comments"})
        self.assertEqual(decide(pr_seen_comments="a,b", pr_unseen_comments="a,b"), ({}, None), "a stale list starts nothing")


class FreshnessTest(unittest.TestCase):
    def test_after_a_fixer_it_waits_for_a_reading_newer_than_its_end(self):
        self.assertEqual(decide(pr_fixing="ci", pr_ci_fix="failing"), ({"pr_fixing": None, "pr_fix_ended_at": NOW}, None))
        stale = decide(pr_fix_ended_at=NOW, pr_checked_at="2026-11-05T10:00:05Z", pr_ci_fix="failing", pr_ci_head="h1")
        self.assertEqual(stale, ({}, None), "the reading is from before the fixer ended")
        same = decide(pr_fix_ended_at=NOW, pr_checked_at=NOW, pr_ci_fix="failing", pr_ci_head="h1")
        self.assertEqual(same, ({}, None), "same second is not after")
        fresh = decide(pr_fix_ended_at=NOW, pr_checked_at="2026-11-05T10:01:00Z", pr_ci_fix="failing", pr_ci_head="h1")
        self.assertTrue(fresh[0]["agent_status"].startswith("blocked: CI still failing"))


class Jira(BaseHTTPRequestHandler):
    """Fake Jira Cloud: SHAPE-1 in `status`, with transitions to Waiting for Deployment and Done."""
    status = ("Code Review", "indeterminate")
    fail = False
    requests = []

    def reply(self, code, data=None):
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        if data is not None:
            self.wfile.write(json.dumps(data).encode())

    def do_GET(self):
        Jira.requests.append(("GET", self.path, None))
        if Jira.fail:
            return self.reply(500, {"errorMessages": ["boom"]})
        if self.path == "/rest/api/3/issue/SHAPE-1?fields=status":
            name, category = Jira.status
            return self.reply(200, {"fields": {"status": {"name": name, "statusCategory": {"key": category}}}})
        if self.path == "/rest/api/3/issue/SHAPE-1/transitions":
            return self.reply(200, {"transitions": [{"id": "31", "to": {"name": "Waiting for Deployment"}},
                                                    {"id": "41", "to": {"name": "Done"}}]})
        self.reply(404, {})

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        Jira.requests.append(("POST", self.path, body))
        self.reply(204)

    def log_message(self, *args):
        pass


class WatcherJiraTest(unittest.TestCase):
    TARGET = "Waiting for Deployment"

    def setUp(self):
        Jira.status, Jira.fail, Jira.requests = ("Code Review", "indeterminate"), False, []
        self.server = HTTPServer(("127.0.0.1", 0), Jira)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.tmp = tempfile.TemporaryDirectory()
        self.root = self.tmp.name
        config = os.path.join(self.root, "jira.yml")
        with open(config, "w") as f:
            f.write(f"login: me@example.com\nserver: http://127.0.0.1:{self.server.server_port}\n")
        self.move = os.path.join(self.root, "move-stub")
        self.moves = os.path.join(self.root, "moves")
        with open(self.move, "w") as f:
            f.write("import os, sys\nopen(os.environ['MOVES'], 'a').write(sys.argv[2] + '\\n')\nsys.exit(int(os.environ.get('MOVE_EXIT', '0')))\n")
        self.env = {"WORKITEM_DIR_ROOT": self.root, "WORKITEM_MOVE": self.move, "WORKITEM_ON_MERGE": "done", "WORKITEM_BUSY": "",
                    "JIRA_CONFIG_FILE": config, "JIRA_API_TOKEN": "secret", "MOVES": self.moves}

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.tmp.cleanup()

    def item(self, name, extra="issue: SHAPE-1 (https://jira/SHAPE-1)\npr_state: merged\n"):
        os.makedirs(os.path.join(self.root, name))
        with open(os.path.join(self.root, name, "workitem.md"), "w") as f:
            f.write(f"---\ntitle: {name}\nagent_status: ok\n{extra}---\nbody\n")
        return {"dir": name, "pr": "https://github.com/o/r/pull/1"}

    def check(self, item, issue_status=TARGET, **env):
        stderr = io.StringIO()
        with mock.patch.dict(os.environ, {**self.env, **env}), contextlib.redirect_stderr(stderr):
            pr.Watcher(3, issue_status).check(item)
        return stderr.getvalue()

    def moved(self):
        if not os.path.exists(self.moves):
            return []
        with open(self.moves) as f:
            return f.read().split()

    def posts(self):
        return [r for r in Jira.requests if r[0] == "POST"]

    def test_a_merged_item_moves_and_its_issue_goes_to_the_target(self):
        log = self.check(self.item("a"))
        self.assertEqual(self.moved(), ["done"])
        self.assertEqual(self.posts(), [("POST", "/rest/api/3/issue/SHAPE-1/transitions", {"transition": {"id": "31"}})])
        self.assertIn("moved SHAPE-1 of a to Waiting for Deployment", log)

    def test_an_issue_already_there_or_done_is_left_alone(self):
        for status in [("waiting for deployment", "indeterminate"), ("Closed", "done")]:
            Jira.status, Jira.requests = status, []
            self.check(self.item(f"a-{status[0]}"))
            self.assertEqual(self.posts(), [], status)

    def test_no_jira_call_without_an_issue_the_flag_or_a_successful_move(self):
        self.check(self.item("none", "issue: none\npr_state: merged\n"))
        self.check(self.item("no-flag"), issue_status=None)
        self.check(self.item("refused"), MOVE_EXIT="1")
        self.assertEqual(self.moved(), ["done", "done", "done"])
        self.assertEqual(Jira.requests, [])

    def test_an_item_another_one_depends_on_leaves_the_ticket_to_the_last(self):
        self.item("later", "issue: SHAPE-1\ndepends_on: other, first\n")
        self.check(self.item("first"))
        self.assertEqual(self.moved(), ["done"])
        self.assertEqual(Jira.requests, [])

    def test_jira_errors_are_logged_and_the_move_still_happens(self):
        Jira.status = ("In Progress", "indeterminate")
        log = self.check(self.item("a"), issue_status="Released")
        self.assertIn("SHAPE-1 has no transition from In Progress to Released; available: Waiting for Deployment, Done", log)
        Jira.fail = True
        log = self.check(self.item("b"))
        self.assertIn("cannot move SHAPE-1 of b to Waiting for Deployment: HTTP Error 500", log)
        self.assertEqual(self.moved(), ["done", "done"])
        self.assertEqual(self.posts(), [])

    def test_main_accepts_the_flag(self):
        with mock.patch.object(pr, "Watcher") as watcher, mock.patch.object(sys, "stdin", io.StringIO("[]")):
            pr.main(["--issue-status", self.TARGET, "--max-attempts", "2"])
        watcher.assert_called_once_with(2, self.TARGET)


if __name__ == "__main__":
    unittest.main()
