import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

BIN = Path(__file__).parent / "skills/workitem-pipeline/bin"
RECORD = "---\ntopic: t\nsummary: \nstatus: working\n---\n\n## Prompt\n\nt\n"


class PlanTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = Path(self.tmp.name).resolve()
        self.record = self.repo / ".workitems/plans/abc123.md"
        self.record.parent.mkdir(parents=True)
        self.record.write_text(RECORD)
        self.env = {**os.environ, "WORKITEM_PLAN": str(self.record), "WORKITEM_REPO": str(self.repo)}

    def tearDown(self):
        self.tmp.cleanup()

    def run_script(self, name, *args, stdin="", env=None):
        return subprocess.run(["python3", str(BIN / name), *args], input=stdin, capture_output=True, text=True, env=env or self.env)

    def test_note_sets_summary_status_and_workitem(self):
        result = self.run_script("workitem-plan-note", "Comparing\n two  options", "--status", "waiting", "--workitem", "X-1-a")
        self.assertEqual(result.returncode, 0, result.stderr)
        text = self.record.read_text()
        self.assertIn("summary: Comparing two options\n", text)
        self.assertIn("status: waiting\n", text)
        self.assertIn("workitem: X-1-a\n---\n", text)
        self.assertIn("## Prompt\n\nt\n", text)

    def test_note_caps_the_summary(self):
        self.run_script("workitem-plan-note", "x" * 500)
        summary = next(l for l in self.record.read_text().splitlines() if l.startswith("summary:"))
        self.assertEqual(len(summary), len("summary: ") + 160)

    def test_plan_is_added_then_replaced(self):
        self.run_script("workitem-plan-note", "--plan", stdin="first\n")
        self.run_script("workitem-plan-note", "--plan", stdin="second\n")
        text = self.record.read_text()
        self.assertTrue(text.endswith("## Prompt\n\nt\n\n## Plan\n\nsecond\n"), text)

    def test_empty_plan_is_refused(self):
        self.assertEqual(self.run_script("workitem-plan-note", "--plan", stdin="  \n").returncode, 1)
        self.assertNotIn("## Plan", self.record.read_text())

    def test_note_outside_a_planning_session_fails(self):
        env = {k: v for k, v in self.env.items() if k != "WORKITEM_PLAN"}
        result = self.run_script("workitem-plan-note", "x", env=env)
        self.assertEqual(result.returncode, 1)
        self.assertIn("WORKITEM_PLAN", result.stderr)

    def guard(self, tool, target, env=None):
        call = {"tool_name": tool, "cwd": str(self.repo), "tool_input": {"file_path": target}}
        return self.run_script("workitem-plan-guard", stdin=json.dumps(call), env=env)

    def test_guard_allows_the_workitems_folder(self):
        self.assertEqual(self.guard("Write", ".workitems/X-1/workitem.md").returncode, 0)
        self.assertEqual(self.guard("Edit", str(self.record)).returncode, 0)

    def test_guard_refuses_code_and_escapes(self):
        for target in ("app.py", str(self.repo / "src/a.rs"), ".workitems/../app.py", "/etc/hosts"):
            result = self.guard("Edit", target)
            self.assertEqual(result.returncode, 2, target)
            self.assertIn("planning session", result.stderr)

    def test_guard_is_off_outside_a_planning_session(self):
        env = {k: v for k, v in self.env.items() if k != "WORKITEM_PLAN"}
        self.assertEqual(self.guard("Edit", "app.py", env=env).returncode, 0)

    def test_notify_marks_a_working_plan_waiting_only(self):
        self.run_script("workitem-notify")
        self.assertIn("status: waiting\n", self.record.read_text())
        self.record.write_text(RECORD.replace("status: working", "status: ended"))
        self.run_script("workitem-notify")
        self.assertIn("status: ended\n", self.record.read_text())


if __name__ == "__main__":
    unittest.main()
