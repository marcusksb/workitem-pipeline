import http.server, os, subprocess, tempfile, threading, unittest

SCRIPT = os.path.join(os.path.dirname(__file__), "skills/workitem-pipeline/bin/workitem-run-item")


class Backend(http.server.BaseHTTPRequestHandler):
    seen = []
    status = 202

    def do_POST(self):
        Backend.seen.append((self.path, self.headers.get("Authorization")))
        body = b'{"session":"wi-x"}' if Backend.status == 202 else b'{"error":"x is blocked"}'
        self.send_response(Backend.status)
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


class WorkitemRunItemTest(unittest.TestCase):
    def setUp(self):
        Backend.seen, Backend.status = [], 202
        self.server = http.server.HTTPServer(("127.0.0.1", 0), Backend)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.dir = tempfile.TemporaryDirectory()
        self.config = os.path.join(self.dir.name, "config.toml")
        with open(self.config, "w") as f:
            f.write('token = "secret"\nport = 7373\n')

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.dir.cleanup()

    def run_item(self, *args, project="my app"):
        env = {**os.environ, "WORKITEM_UI_PORT": str(self.server.server_port), "WORKITEM_UI_CONFIG": self.config}
        env.pop("WORKITEM_PROJECT", None)
        if project:
            env["WORKITEM_PROJECT"] = project
        return subprocess.run([SCRIPT, *args], env=env, capture_output=True, text=True)

    def test_posts_to_the_run_endpoint_with_the_token(self):
        result = self.run_item("T-1-a")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(Backend.seen, [("/api/workitems/my%20app/T-1-a/run", "Bearer secret")])

    def test_fails_with_the_backends_reason(self):
        Backend.status = 409
        result = self.run_item("T-1-a")
        self.assertEqual(result.returncode, 1)
        self.assertIn("T-1-a: x is blocked", result.stderr)

    def test_exits_2_when_the_handler_cannot_be_used(self):
        Backend.status = 422
        self.assertEqual(self.run_item("T-1-a").returncode, 2)

    def test_needs_a_project_and_one_item(self):
        self.assertNotEqual(self.run_item("T-1-a", project=None).returncode, 0)
        self.assertNotEqual(self.run_item().returncode, 0)


if __name__ == "__main__":
    unittest.main()
