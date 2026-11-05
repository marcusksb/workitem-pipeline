import importlib.machinery, importlib.util, os, sys, unittest

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
        self.assertEqual(updates, {"pr_conflict_head": "h1", "pr_fix_attempts": None, "pr_fixing": "conflict"})

    def test_red_ci_counts_attempts_and_stops_at_the_cap(self):
        self.assertEqual(decide(pr_ci_fix="failing", pr_fix_attempts="1"),
                         ({"pr_ci_head": "h1", "pr_fix_attempts": "2", "pr_fixing": "ci"}, ("fix", "ci")))
        self.assertEqual(decide(pr_ci_fix="failing", pr_fix_attempts="3"),
                         ({"agent_status": "blocked: CI still failing after 3 fix attempts"}, None))
        self.assertEqual(decide(pr_ci_fix="pending", pr_fix_attempts="1"), ({}, None), "running checks are left alone")
        self.assertEqual(decide(pr_fix_attempts="2", pr_ci_head="h0"), ({"pr_fix_attempts": None, "pr_ci_head": None}, None), "green ends the streak")

    def test_a_fixer_that_left_the_head_as_it_was_blocks_the_item(self):
        self.assertEqual(decide(pr_merge="conflicting", pr_conflict_head="h1")[0]["agent_status"][:40],
                         "blocked: merge conflict still open after")
        self.assertEqual(decide(pr_ci_fix="failing", pr_ci_head="h1")[0]["agent_status"][:40],
                         "blocked: CI still failing and the CI fix")
        self.assertEqual(decide(pr_ci_fix="failing", pr_ci_head="h0", pr_fix_attempts="1")[1], ("fix", "ci"), "a new head gets a new try")

    def test_a_blocked_item_forgets_heads_and_attempts_so_clearing_retries(self):
        updates, action = decide(agent_status="blocked: x", pr_conflict_head="h1", pr_ci_head="h1", pr_fix_attempts="3")
        self.assertEqual((updates, action), ({"pr_conflict_head": None, "pr_ci_head": None, "pr_fix_attempts": None}, None))

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


if __name__ == "__main__":
    unittest.main()
