import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

NOTIFY = Path(__file__).parent / "skills/workitem-pipeline/bin/workitem-notify"
BLOCKED = "blocked: waiting for input, tmux attach -t wi-T-1-a"


class NotifyTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        repo = Path(self.tmp.name).resolve()
        self.file = repo / ".workitems/T-1-a/workitem.md"
        self.file.parent.mkdir(parents=True)
        self.env = {**os.environ, "WORKITEM": "T-1-a", "WORKITEM_REPO": str(repo)}

    def tearDown(self):
        self.tmp.cleanup()

    def notify(self, status, kind):
        self.file.write_text(f"---\ntitle: t\nagent_status: {status}\n---\n")
        stdin = json.dumps({"hook_event_name": "Notification", "notification_type": kind})
        subprocess.run(["python3", str(NOTIFY)], input=stdin, text=True, env=self.env, check=True)
        return next(l for l in self.file.read_text().splitlines() if l.startswith("agent_status:")).removeprefix("agent_status: ")

    def test_working_blocks_on_any_prompt(self):
        self.assertEqual(self.notify("working", "idle_prompt"), BLOCKED)
        self.assertEqual(self.notify("working", "permission_prompt"), BLOCKED)

    def test_waiting_survives_idle_but_blocks_when_a_human_is_needed(self):
        self.assertEqual(self.notify("waiting: CI on PR #1", "idle_prompt"), "waiting: CI on PR #1")
        self.assertEqual(self.notify("waiting: CI on PR #1", "permission_prompt"), BLOCKED)
        self.assertEqual(self.notify("waiting: CI on PR #1", "elicitation_dialog"), BLOCKED)

    def test_other_statuses_are_left_alone(self):
        self.assertEqual(self.notify("ok", "permission_prompt"), "ok")
        self.assertEqual(self.notify("blocked: why", "permission_prompt"), "blocked: why")


if __name__ == "__main__":
    unittest.main()
