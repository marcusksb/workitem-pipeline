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


BUNDLED = json.loads((Path(__file__).parent / "skills/workitem-pipeline/pipeline.json").read_text())


class WorkitemMoveTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = Path(self.tmp.name, "repo")
        self.repo.mkdir()
        self.workspace = Path(self.tmp.name, "ws")
        self.item = self.workspace / "X-1-test"
        self.item.mkdir(parents=True)
        (self.workspace / "boards").mkdir()
        self.named_board("dev", **{k: v for k, v in BUNDLED.items() if k in ("entry", "stages", "brief_sections")})

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, stage="backlog", approved="false", body="", issue="X-1 (https://example.atlassian.net/browse/X-1)", board="dev", extra=""):
        line = f"board: {board}\n" if board else ""
        (self.item / "workitem.md").write_text(f"---\ntitle: t\nstage: {stage}\n{line}approved: {approved}\nissue: {issue}\n{extra}---\n\n# t\n{body}")

    def run_move(self, *args, **env):
        return subprocess.run(["python3", str(MOVE), *args], cwd=self.tmp.name, capture_output=True, text=True, env={**os.environ, **env})

    def stage(self):
        return next(l for l in (self.item / "workitem.md").read_text().splitlines() if l.startswith("stage:"))

    def named_board(self, name, **over):
        config = {
            "repo": str(self.repo),
            "brief_sections": [],
            "entry": "idea",
            "stages": [
                {"name": "idea", "handler": "think", "auto": True, "next": ["build"]},
                {"name": "build", "on_enter": {"approved": "false"}},
            ],
            **over,
        }
        (self.workspace / "boards" / f"{name}.json").write_text(json.dumps(config))

    def other(self, name, fields):
        folder = self.workspace / name
        folder.mkdir(exist_ok=True)
        (folder / "workitem.md").write_text("---\ntitle: o\n" + "".join(f"{k}: {v}\n" for k, v in fields.items()) + "---\n")

    def test_allowed_move(self):
        self.write("backlog")
        self.assertEqual(self.run_move(str(self.item), "analyze").returncode, 0)
        self.assertEqual(self.stage(), "stage: analyze")

    def test_move_by_relative_path(self):
        self.write("backlog")
        self.assertEqual(self.run_move("ws/X-1-test", "analyze").returncode, 0)
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

    def test_item_without_a_board_is_refused(self):
        self.write("backlog", board="")
        result = self.run_move(str(self.item), "analyze")
        self.assertEqual(result.returncode, 1)
        self.assertIn("board", result.stderr)

    def test_board_without_a_repo_is_valid(self):
        self.write("backlog")
        self.named_board("dev", **{k: v for k, v in BUNDLED.items() if k in ("entry", "stages", "brief_sections")})
        board = self.workspace / "boards/dev.json"
        config = json.loads(board.read_text())
        del config["repo"]
        board.write_text(json.dumps(config))
        result = self.run_move(str(self.item), "analyze")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_board_with_a_bad_repo_is_invalid(self):
        self.write("backlog")
        for repo in (None, "relative", str(self.repo / "gone")):
            self.named_board("dev", **{k: v for k, v in BUNDLED.items() if k in ("entry", "stages", "brief_sections")}, repo=repo)
            result = self.run_move(str(self.item), "analyze")
            self.assertEqual(result.returncode, 1)
            self.assertIn("repo", result.stderr)

    def test_depends_on_merged_waits_for_every_dependency(self):
        self.named_board("api", stages=[{"name": "dev", "next": ["review"]}, {"name": "review", "next": ["done"], "on_merge": "done"}, {"name": "done"}], entry="dev")
        self.named_board("dev", stages=[{"name": "idea", "next": ["build"]}, {"name": "build", "require": {"depends_on": "merged"}}])
        self.other("A-1", {"board": "api", "stage": "review"})
        self.other("A-2", {"board": "api", "stage": "review", "pr_state": "merged"})
        self.write("idea", extra="depends_on: A-1, A-2, A-gone\n")
        result = self.run_move(str(self.item), "build")
        self.assertEqual(result.returncode, 1)
        self.assertIn("A-1", result.stderr)
        self.assertNotIn("A-2", result.stderr)
        self.other("A-1", {"board": "api", "stage": "done"})
        self.assertEqual(self.run_move(str(self.item), "build").returncode, 0, "on_merge stage, pr_state and a gone item all count")

    def test_a_new_stage_starts_the_handler_attempts_over(self):
        self.named_board("dev", stages=[{"name": "idea", "next": ["build"]}, {"name": "build"}])
        self.write("idea", extra="handler_attempts: 2\n")
        self.assertEqual(self.run_move(str(self.item), "build").returncode, 0)
        self.assertNotIn("handler_attempts", (self.item / "workitem.md").read_text())
        self.assertIn("stage: build", (self.item / "workitem.md").read_text())

    def test_invalid_board_file_is_refused(self):
        self.write("idea")
        for text in ("{", json.dumps({"entry": "nope", "stages": [], "brief_sections": [], "repo": str(self.repo)})):
            (self.workspace / "boards/dev.json").write_text(text)
            result = self.run_move(str(self.item), "build")
            self.assertEqual(result.returncode, 1)
            self.assertIn("dev.json", result.stderr)

    def test_next_must_name_a_stage_of_the_board(self):
        self.named_board("dev", stages=[{"name": "idea", "next": ["gone"]}])
        self.write("idea")
        result = self.run_move(str(self.item), "gone")
        self.assertEqual(result.returncode, 1)
        self.assertIn("gone", result.stderr)

    def test_item_off_the_board_can_only_move_to_entry(self):
        self.named_board("dev")
        self.write("removed_stage")
        self.assertEqual(self.run_move(str(self.item), "build").returncode, 1)
        self.assertEqual(self.run_move(str(self.item), "idea").returncode, 0)
        self.assertEqual(self.stage(), "stage: idea")

    def test_require_checks_apply_to_any_stage_name(self):
        self.named_board(
            "dev",
            brief_sections=["Plan"],
            entry="todo",
            stages=[
                {"name": "todo", "next": ["ready"]},
                {"name": "ready", "require": {"issue": "linked", "brief": "complete"}},
            ],
        )
        self.write("todo", issue="none")
        result = self.run_move(str(self.item), "ready")
        self.assertEqual(result.returncode, 1)
        self.assertIn("issue", result.stderr)
        self.write("todo", body="\n## Review brief\n\n### Plan\n")
        result = self.run_move(str(self.item), "ready")
        self.assertEqual(result.returncode, 1)
        self.assertIn("brief", result.stderr)
        self.write("todo", body="\n## Review brief\n\n### Plan\nx\n")
        self.assertEqual(self.run_move(str(self.item), "ready").returncode, 0)

    def test_a_full_stage_refuses_more_items_of_its_board(self):
        self.named_board("research", stages=[
            {"name": "idea", "next": ["build"]},
            {"name": "build", "limit": 1},
        ])
        self.write("idea", board="research")
        self.other("X-2-other", {"stage": "build", "board": "research"})
        result = self.run_move(str(self.item), "build")
        self.assertEqual(result.returncode, 1)
        self.assertIn("full", result.stderr)
        self.assertEqual(self.stage(), "stage: idea")
        self.other("X-2-other", {"stage": "build", "board": "dev"})  # another board: does not count
        self.assertEqual(self.run_move(str(self.item), "build").returncode, 0)

    def test_item_follows_the_board_it_names(self):
        self.named_board("research")
        self.write("idea", board="research")
        self.assertEqual(self.run_move(str(self.item), "build").returncode, 0)
        self.assertEqual(self.stage(), "stage: build")
        self.assertEqual(self.run_move(str(self.item), "analyze").returncode, 1)

    def test_board_without_a_file_or_with_a_bad_name_is_refused(self):
        for name in ("nope", "../dev"):
            self.write("idea", board=name)
            result = self.run_move(str(self.item), "build")
            self.assertEqual(result.returncode, 1)
            self.assertIn("no file", result.stderr)

    def test_switching_boards_lands_on_the_target_entry(self):
        self.named_board("research")
        self.write("analyze", approved="true")
        result = self.run_move(str(self.item), "--board", "research")
        self.assertEqual(result.returncode, 0, result.stderr)
        text = (self.item / "workitem.md").read_text()
        self.assertIn("stage: idea", text)
        self.assertIn("board: research", text)
        self.assertEqual(self.run_move(str(self.item), "--board", "dev").returncode, 0)
        self.assertIn("stage: backlog", (self.item / "workitem.md").read_text())

    def test_switching_works_from_no_board_or_a_board_that_has_no_file(self):
        self.named_board("research")
        for board in ("", "gone"):
            self.write("whatever", board=board)
            self.assertEqual(self.run_move(str(self.item), "--board", "research").returncode, 0)
            self.assertEqual(self.stage(), "stage: idea")

    def test_switching_is_refused_for_a_missing_board_or_a_running_session(self):
        self.named_board("research")
        self.write("analyze")
        self.assertEqual(self.run_move(str(self.item), "--board", "nope").returncode, 1)
        socket = f"wi-move-test-{os.getpid()}"
        subprocess.run(["tmux", "-L", socket, "new-session", "-d", "-s", "wi-X-1-test", "sleep 60"], check=True)
        try:
            result = self.run_move(str(self.item), "--board", "research", WORKITEM_UI_TMUX_SOCKET=socket)
            self.assertEqual(result.returncode, 1)
            self.assertIn("session", result.stderr)
            self.assertEqual(self.stage(), "stage: analyze")
        finally:
            subprocess.run(["tmux", "-L", socket, "kill-server"])


class MigrateTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = Path(self.tmp.name).resolve()
        self.repo = root / "repo"
        self.home = root / "home"
        self.source = self.repo / ".workitems"
        (self.source / "boards/review/skills/check").mkdir(parents=True)
        (self.source / "boards/review.json").write_text(json.dumps({"entry": "a", "stages": [{"name": "a"}]}))
        (self.source / "boards/review/skills/check/SKILL.md").write_text("board level")
        (self.source / "skills/check").mkdir(parents=True)
        (self.source / "skills/check/SKILL.md").write_text("repo level")
        (self.source / "learnings.md").write_text("- rule")
        (self.source / "notes.txt").write_text("x")
        for name, board in (("A-1", None), ("B-2", "review")):
            (self.source / name).mkdir()
            line = f"board: {board}\n" if board else ""
            (self.source / name / "workitem.md").write_text(f"---\ntitle: t\nstage: backlog\n{line}---\n\nbody\n")
        (self.source / "plans").mkdir()
        (self.source / "plans/p1.md").write_text("---\ntopic: t\n---\n")
        self.workspace = self.home / "workspaces/main"

    def tearDown(self):
        self.tmp.cleanup()

    def migrate(self, *args):
        env = {**os.environ, "WORKITEM_UI_HOME": str(self.home), "WORKITEM_UI_TMUX_SOCKET": f"wi-migrate-test-{os.getpid()}"}
        return subprocess.run(["python3", str(MIGRATE), *args], capture_output=True, text=True, env=env)

    def test_moves_items_boards_and_plans_into_the_workspace(self):
        result = self.migrate("--to-workspace", "main", "--board", "api", str(self.repo))
        self.assertEqual(result.returncode, 0, result.stderr)
        api = json.loads((self.workspace / "boards/api.json").read_text())
        self.assertEqual(api["repo"], str(self.repo))
        self.assertEqual(api["entry"], "backlog", "no default board file: the bundled one")
        self.assertEqual(json.loads((self.workspace / "boards/api-review.json").read_text())["repo"], str(self.repo))
        self.assertIn("board: api\n", (self.workspace / "A-1/workitem.md").read_text())
        self.assertIn("board: api-review\n", (self.workspace / "B-2/workitem.md").read_text())
        self.assertNotIn("board: review", (self.workspace / "B-2/workitem.md").read_text())
        self.assertEqual((self.workspace / "boards/api/skills/check/SKILL.md").read_text(), "repo level")
        self.assertEqual((self.workspace / "boards/api-review/skills/check/SKILL.md").read_text(), "board level")
        self.assertEqual((self.workspace / "boards/api/learnings.md").read_text(), "- rule")
        self.assertTrue((self.workspace / "plans/p1.md").exists())
        self.assertEqual(sorted(p.name for p in self.source.iterdir()), ["notes.txt"])
        self.assertIn("notes.txt", result.stdout)
        moved = subprocess.run(["python3", str(MOVE), str(self.workspace / "A-1"), "analyze"], capture_output=True, text=True)
        self.assertEqual(moved.returncode, 0, moved.stderr)

    def test_refuses_without_touching_anything_when_a_target_exists(self):
        (self.workspace / "B-2").mkdir(parents=True)
        result = self.migrate("--to-workspace", "main", "--board", "api", str(self.repo))
        self.assertEqual(result.returncode, 1)
        self.assertIn("B-2", result.stderr)
        self.assertTrue((self.source / "A-1/workitem.md").exists())
        self.assertFalse((self.workspace / "boards").exists())

    def test_a_second_repo_joins_the_same_workspace(self):
        self.assertEqual(self.migrate("--to-workspace", "main", "--board", "api", str(self.repo)).returncode, 0)
        other = Path(self.tmp.name).resolve() / "web"
        (other / ".claude/workitems/C-3").mkdir(parents=True)
        (other / ".claude/workitems/C-3/workitem.md").write_text("---\nstage: backlog\n---\n")
        result = self.migrate("--to-workspace", "main", "--board", "web", str(other))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("board: web", (self.workspace / "C-3/workitem.md").read_text())
        self.assertEqual(self.migrate("--to-workspace", "main", "--board", "web", str(other)).returncode, 1)


if __name__ == "__main__":
    unittest.main()
