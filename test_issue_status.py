import json, os, subprocess, sys, tempfile, threading, unittest
from http.server import BaseHTTPRequestHandler, HTTPServer

SCRIPT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "skills/workitem-pipeline/watch/issue-status")
ISSUES = {
    "MNT-1": ("Ready for QA", "indeterminate"),
    "SHAPE-2": ("Done", "done"),
}


class Jira(BaseHTTPRequestHandler):
    requests = []

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        Jira.requests.append((self.path, self.headers["Authorization"], body))
        keys = body["jql"].removeprefix("key in (").removesuffix(")").split(", ")
        issues = [{"key": k, "fields": {"status": {"name": ISSUES[k][0], "statusCategory": {"key": ISSUES[k][1]}}}} for k in keys if k in ISSUES]
        data = json.dumps({"issues": issues}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *args):
        pass


class IssueStatusTest(unittest.TestCase):
    def setUp(self):
        Jira.requests = []
        self.server = HTTPServer(("127.0.0.1", 0), Jira)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.tmp = tempfile.TemporaryDirectory()
        self.root = self.tmp.name
        self.config = os.path.join(self.root, "jira.yml")
        with open(self.config, "w") as f:
            f.write(f"auth_type: basic\nlogin: me@example.com\nproject:\n    key: X\nserver: http://127.0.0.1:{self.server.server_port}\n")

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.tmp.cleanup()

    def item(self, name, issue, extra=""):
        os.makedirs(os.path.join(self.root, name))
        with open(os.path.join(self.root, name, "workitem.md"), "w") as f:
            f.write(f"---\ntitle: {name}\nissue: {issue}\n{extra}---\nbody\n")
        return {"dir": name, "issue": issue}

    def read(self, name):
        with open(os.path.join(self.root, name, "workitem.md")) as f:
            return f.read()

    def run_script(self, items, **env):
        base = {"PATH": os.environ["PATH"], "HOME": self.root, "WORKITEM_DIR_ROOT": self.root, "JIRA_CONFIG_FILE": self.config, "JIRA_API_TOKEN": "secret"}
        return subprocess.run([sys.executable, SCRIPT], input=json.dumps(items), capture_output=True, text=True, env={**base, **env}, timeout=30)

    def test_reads_every_project_in_one_request_and_writes_only_changes(self):
        items = [self.item("a", "MNT-1 (https://jira/MNT-1)"), self.item("b", "SHAPE-2", "issue_status: Code Review\n"), self.item("c", "NOPE-3"), self.item("d", "")]
        self.assertEqual(self.run_script(items).returncode, 0)
        self.assertEqual(len(Jira.requests), 1)
        path, auth, body = Jira.requests[0]
        self.assertEqual(path, "/rest/api/3/search/jql")
        self.assertEqual(auth, "Basic bWVAZXhhbXBsZS5jb206c2VjcmV0")  # me@example.com:secret
        self.assertEqual(body["jql"], "key in (MNT-1, NOPE-3, SHAPE-2)")
        self.assertIn("issue_status: Ready for QA\nissue_status_category: indeterminate\n---\nbody", self.read("a"))
        self.assertIn("issue_status: Done\n", self.read("b"))
        self.assertNotIn("Code Review", self.read("b"))
        self.assertNotIn("issue_status", self.read("c"), "a key Jira does not know is left alone")

    def test_splits_more_than_a_hundred_keys_into_several_requests(self):
        items = [{"dir": f"i{n}", "issue": f"MNT-{n}"} for n in range(1000, 1150)]
        self.assertEqual(self.run_script(items).returncode, 0)
        self.assertEqual([body["jql"].count(",") + 1 for _, _, body in Jira.requests], [100, 50])

    def test_without_a_token_it_skips_quietly(self):
        items = [self.item("a", "MNT-1")]
        result = self.run_script(items, JIRA_API_TOKEN="", PATH="/nonexistent")
        self.assertEqual(result.returncode, 0)
        self.assertIn("no Jira server, login or API token", result.stderr)
        self.assertEqual(Jira.requests, [])

    def test_reads_the_token_from_netrc(self):
        with open(os.path.join(self.root, ".netrc"), "w") as f:
            f.write("machine 127.0.0.1 login me@example.com password fromnetrc\n")
        os.chmod(os.path.join(self.root, ".netrc"), 0o600)
        self.run_script([self.item("a", "MNT-1")], JIRA_API_TOKEN="")
        self.assertEqual(Jira.requests[0][1], "Basic bWVAZXhhbXBsZS5jb206ZnJvbW5ldHJj")  # me@example.com:fromnetrc


if __name__ == "__main__":
    unittest.main()
