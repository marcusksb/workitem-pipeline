import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

MOVE = Path(__file__).parent / "skills/workitem-pipeline/bin/workitem-move"
MIGRATE = Path(__file__).parent / "skills/workitem-pipeline/bin/workitem-migrate"
BRIEF = "\n## Review brief\n\n" + "".join(
    f"### {name}\nx\n" for name in ["Findings", "Plan", "Risk", "Size", "Test plan", "Assumptions", "Links"]
)


class WorkitemMoveTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = Path(self.tmp.name)
        subprocess.run(["git", "init", "-q"], cwd=self.repo, check=True)
        self.item = self.repo / ".claude/workitems/X-1-test"
        self.item.mkdir(parents=True)

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, stage="backlog", approved="false", body="", issue="X-1 (https://example.atlassian.net/browse/X-1)"):
        (self.item / "workitem.md").write_text(f"---\ntitle: t\nstage: {stage}\napproved: {approved}\nissue: {issue}\n---\n\n# t\n{body}")

    def run_move(self, *args, **env):
        return subprocess.run(["python3", str(MOVE), *args], cwd=self.repo, capture_output=True, text=True, env={**os.environ, **env})

    def stage(self):
        return next(l for l in (self.item / "workitem.md").read_text().splitlines() if l.startswith("stage:"))

    def test_allowed_move(self):
        self.write("backlog")
        self.assertEqual(self.run_move(str(self.item), "analyze").returncode, 0)
        self.assertEqual(self.stage(), "stage: analyze")

    def test_move_by_bare_directory_name(self):
        self.write("backlog")
        self.assertEqual(self.run_move("X-1-test", "analyze").returncode, 0)
        self.assertEqual(self.stage(), "stage: analyze")

    def test_disallowed_move(self):
        self.write("backlog")
        result = self.run_move(str(self.item), "in_development")
        self.assertEqual(result.returncode, 1)
        self.assertEqual(self.stage(), "stage: backlog")

    def test_ready_without_brief_is_refused(self):
        self.write("analyze")
        self.assertEqual(self.run_move(str(self.item), "ready_for_development").returncode, 1)

    def test_ready_without_issue_is_refused(self):
        for issue in ("none", ""):
            self.write("analyze", body=BRIEF, issue=issue)
            result = self.run_move(str(self.item), "ready_for_development")
            self.assertEqual(result.returncode, 1)
            self.assertIn("issue", result.stderr)
            self.assertEqual(self.stage(), "stage: analyze")

    def test_ready_with_brief_resets_approval(self):
        self.write("analyze", approved="true", body=BRIEF)
        self.assertEqual(self.run_move(str(self.item), "ready_for_development").returncode, 0)
        self.assertIn("approved: false", (self.item / "workitem.md").read_text())

    def test_development_needs_approval(self):
        self.write("ready_for_development", approved="false", body=BRIEF)
        self.assertEqual(self.run_move(str(self.item), "in_development").returncode, 1)
        self.write("ready_for_development", approved="true", body=BRIEF)
        self.assertEqual(self.run_move(str(self.item), "in_development").returncode, 0)

    def test_list_auto_only(self):
        self.write("backlog")
        listed = self.run_move("--list", "--auto").stdout
        self.assertIn("X-1-test\tbacklog\tworkitem-triage", listed)
        self.write("ready_for_development", body=BRIEF)
        self.assertEqual(self.run_move("--list", "--auto").stdout, "")

    def test_new_folder_wins_over_the_old_one(self):
        self.write("backlog")
        new = self.repo / ".workitems/X-1-test"
        new.mkdir(parents=True)
        (new / "workitem.md").write_text((self.item / "workitem.md").read_text())
        self.assertEqual(self.run_move("X-1-test", "analyze").returncode, 0)
        self.assertIn("stage: analyze", (new / "workitem.md").read_text())
        self.assertEqual(self.stage(), "stage: backlog")

    def test_migrate_moves_the_old_folder(self):
        self.write("backlog")
        result = subprocess.run(["python3", str(MIGRATE)], cwd=self.repo, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0)
        self.assertFalse((self.repo / ".claude/workitems").exists())
        self.assertTrue((self.repo / ".workitems/X-1-test/workitem.md").exists())
        self.assertEqual(self.run_move("X-1-test", "analyze").returncode, 0)

    def test_migrate_refuses_when_the_new_folder_exists(self):
        self.write("backlog")
        (self.repo / ".workitems").mkdir()
        result = subprocess.run(["python3", str(MIGRATE)], cwd=self.repo, capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertTrue(self.item.exists())

    def board(self, **over):
        config = {
            "brief_sections": [],
            "entry": "todo",
            "stages": [
                {"name": "todo", "next": ["doing"]},
                {"name": "doing", "next": ["todo"], "on_enter": {"approved": "false"}},
            ],
            **over,
        }
        (self.item.parent / "pipeline.json").write_text(json.dumps(config))

    def test_project_board_replaces_the_bundled_one(self):
        self.board()
        self.write("todo")
        self.assertEqual(self.run_move("X-1-test", "doing").returncode, 0)
        self.assertEqual(self.stage(), "stage: doing")
        self.assertEqual(self.run_move("X-1-test", "analyze").returncode, 1)

    def test_default_board_can_live_in_the_boards_folder_and_wins_over_pipeline_json(self):
        self.board()
        boards = self.item.parent / "boards"
        boards.mkdir()
        (boards / "default.json").write_text(json.dumps({
            "brief_sections": [],
            "entry": "queued",
            "stages": [{"name": "queued", "next": ["run"]}, {"name": "run", "next": []}],
        }))
        self.write("queued")
        self.assertEqual(self.run_move("X-1-test", "run").returncode, 0)
        self.assertEqual(self.run_move("X-1-test", "doing").returncode, 1, "pipeline.json is not used")

    def test_a_new_stage_starts_the_handler_attempts_over(self):
        self.board()
        self.write("todo")
        text = (self.item / "workitem.md").read_text().replace("\n---\n", "\nhandler_attempts: 2\n---\n", 1)
        (self.item / "workitem.md").write_text(text)
        self.assertEqual(self.run_move("X-1-test", "doing").returncode, 0)
        self.assertNotIn("handler_attempts", (self.item / "workitem.md").read_text())
        self.assertIn("stage: doing", (self.item / "workitem.md").read_text())

    def test_invalid_board_file_is_refused(self):
        self.write("todo")
        for text in ("{", json.dumps({"entry": "nope", "stages": [], "brief_sections": []})):
            (self.item.parent / "pipeline.json").write_text(text)
            result = self.run_move("X-1-test", "doing")
            self.assertEqual(result.returncode, 1)
            self.assertIn("pipeline.json", result.stderr)

    def test_next_must_name_a_stage_of_the_board(self):
        self.board(stages=[{"name": "todo", "next": ["gone"]}])
        self.write("todo")
        result = self.run_move("X-1-test", "gone")
        self.assertEqual(result.returncode, 1)
        self.assertIn("gone", result.stderr)

    def test_item_off_the_board_can_only_move_to_entry(self):
        self.board()
        self.write("removed_stage")
        self.assertEqual(self.run_move("X-1-test", "doing").returncode, 1)
        self.assertEqual(self.run_move("X-1-test", "todo").returncode, 0)
        self.assertEqual(self.stage(), "stage: todo")

    def test_require_checks_apply_to_any_stage_name(self):
        self.board(
            brief_sections=["Plan"],
            stages=[
                {"name": "todo", "next": ["ready"]},
                {"name": "ready", "require": {"issue": "linked", "brief": "complete"}},
            ],
        )
        self.write("todo", issue="none")
        result = self.run_move("X-1-test", "ready")
        self.assertEqual(result.returncode, 1)
        self.assertIn("issue", result.stderr)
        self.write("todo", body="\n## Review brief\n\n### Plan\n")
        result = self.run_move("X-1-test", "ready")
        self.assertEqual(result.returncode, 1)
        self.assertIn("brief", result.stderr)
        self.write("todo", body="\n## Review brief\n\n### Plan\nx\n")
        self.assertEqual(self.run_move("X-1-test", "ready").returncode, 0)

    def named_board(self, name, **over):
        config = {
            "brief_sections": [],
            "entry": "idea",
            "stages": [
                {"name": "idea", "handler": "think", "auto": True, "next": ["build"]},
                {"name": "build", "on_enter": {"approved": "false"}},
            ],
            **over,
        }
        (self.item.parent / "boards").mkdir(exist_ok=True)
        (self.item.parent / "boards" / f"{name}.json").write_text(json.dumps(config))

    def set_board(self, name):
        path = self.item / "workitem.md"
        path.write_text(path.read_text().replace("\n---\n", f"\nboard: {name}\n---\n", 1))

    def test_a_full_stage_refuses_more_items_of_its_board(self):
        self.named_board("research", stages=[
            {"name": "idea", "next": ["build"]},
            {"name": "build", "limit": 1},
        ])
        self.write("idea")
        self.set_board("research")
        other = self.item.parent / "X-2-other"
        other.mkdir()
        (other / "workitem.md").write_text("---\ntitle: o\nstage: build\nboard: research\n---\n")
        result = self.run_move("X-1-test", "build")
        self.assertEqual(result.returncode, 1)
        self.assertIn("full", result.stderr)
        self.assertEqual(self.stage(), "stage: idea")
        (other / "workitem.md").write_text("---\ntitle: o\nstage: build\n---\n")  # default board: does not count
        self.assertEqual(self.run_move("X-1-test", "build").returncode, 0)

    def test_item_follows_the_board_it_names(self):
        self.named_board("research")
        self.write("idea")
        self.set_board("research")
        self.assertEqual(self.run_move("X-1-test", "build").returncode, 0)
        self.assertEqual(self.stage(), "stage: build")
        self.assertEqual(self.run_move("X-1-test", "analyze").returncode, 1)

    def test_list_reads_each_items_own_board(self):
        self.named_board("research")
        self.write("idea")
        self.set_board("research")
        self.assertIn("X-1-test\tidea\tthink", self.run_move("--list", "--auto").stdout)

    def test_board_without_a_file_or_with_a_bad_name_is_refused(self):
        self.write("idea")
        for name in ("nope", "../pipeline"):
            self.set_board(name)
            result = self.run_move("X-1-test", "build")
            self.assertEqual(result.returncode, 1)
            self.assertIn("no file", result.stderr)
            self.write("idea")

    def test_switching_boards_lands_on_the_target_entry(self):
        self.named_board("research")
        self.write("analyze", approved="true")
        result = self.run_move("X-1-test", "--board", "research")
        self.assertEqual(result.returncode, 0, result.stderr)
        text = (self.item / "workitem.md").read_text()
        self.assertIn("stage: idea", text)
        self.assertIn("board: research", text)
        self.assertEqual(self.run_move("X-1-test", "--board", "default").returncode, 0)
        self.assertIn("stage: backlog", (self.item / "workitem.md").read_text())

    def test_switching_works_from_a_board_that_has_no_file(self):
        self.named_board("research")
        self.write("whatever")
        self.set_board("gone")
        self.assertEqual(self.run_move("X-1-test", "--board", "research").returncode, 0)
        self.assertEqual(self.stage(), "stage: idea")

    def test_switching_is_refused_for_a_missing_board_or_a_running_session(self):
        self.named_board("research")
        self.write("analyze")
        self.assertEqual(self.run_move("X-1-test", "--board", "nope").returncode, 1)
        socket = f"wi-move-test-{os.getpid()}"
        subprocess.run(["tmux", "-L", socket, "new-session", "-d", "-s", "wi-X-1-test", "sleep 60"], check=True)
        try:
            result = self.run_move("X-1-test", "--board", "research", WORKITEM_UI_TMUX_SOCKET=socket)
            self.assertEqual(result.returncode, 1)
            self.assertIn("session", result.stderr)
            self.assertEqual(self.stage(), "stage: analyze")
        finally:
            subprocess.run(["tmux", "-L", socket, "kill-server"])


if __name__ == "__main__":
    unittest.main()
