import importlib.machinery, importlib.util, os, sys, tempfile, unittest

sys.dont_write_bytecode = True

PATH = os.path.join(os.path.dirname(__file__), "skills/workitem-pipeline/watch/pr-review")
loader = importlib.machinery.SourceFileLoader("pr_review", PATH)
spec = importlib.util.spec_from_loader("pr_review", loader)
pr = importlib.util.module_from_spec(spec)
loader.exec_module(pr)

IGNORE = ["dockerized", "e2e", "playwright"]


def run(name, status, conclusion):
    return {"name": name, "status": status, "conclusion": conclusion}


class CiTest(unittest.TestCase):
    def test_pending_until_every_relevant_check_settled_then_failing_or_passing(self):
        ok, red, running = run("rspec", "COMPLETED", "SUCCESS"), run("lint", "COMPLETED", "FAILURE"), run("build", "IN_PROGRESS", "")
        self.assertEqual(pr.ci_of([], IGNORE), "none")
        self.assertEqual(pr.ci_of([ok], IGNORE), "passing")
        self.assertEqual(pr.ci_of([ok, red, running], IGNORE), "pending")
        self.assertEqual(pr.ci_of([run("lint", "COMPLETED", "TIMED_OUT")], IGNORE), "failing")

    def test_fixer_ci_ignores_end_to_end_suites_and_commit_statuses_are_read(self):
        e2e = run("Playwright E2E", "COMPLETED", "FAILURE")
        self.assertEqual(pr.ci_of([e2e], IGNORE), "none")
        self.assertEqual(pr.ci_of([e2e]), "failing")
        self.assertEqual(pr.ci_of([{"context": "ci/build", "state": "FAILURE"}]), "failing")

    def test_failed_checks_lists_each_red_check_once_with_its_link(self):
        red = {**run("RSpec", "COMPLETED", "FAILURE"), "detailsUrl": "https://ci/1"}
        failed = pr.failed_checks([run("lint", "COMPLETED", "SUCCESS"), red, run("RSpec", "COMPLETED", "FAILURE")])
        self.assertEqual(failed, [{"name": "RSpec", "url": "https://ci/1"}])


class StatusTest(unittest.TestCase):
    def test_approval_and_merge_map_from_github_values(self):
        shown, fix_ci = pr.status_of({"state": "OPEN", "mergeable": "CONFLICTING", "reviewDecision": "", "statusCheckRollup": None}, "me", IGNORE)
        self.assertEqual((shown["approval"], shown["merge"], shown["ci"], fix_ci), ("none", "conflicting", "none", "none"))
        shown, _ = pr.status_of({"state": "OPEN", "mergeable": "MERGEABLE", "reviewDecision": "CHANGES_REQUESTED"}, "me", IGNORE)
        self.assertEqual(shown["approval"], "changes_requested")

    def test_approved_by_me_follows_my_latest_verdict(self):
        def reviews(*pairs):
            return {"reviews": [{"id": "x", "state": state, "author": {"login": who}} for who, state in pairs]}
        self.assertTrue(pr.approved_by(reviews(("me", "APPROVED")), "me"))
        self.assertTrue(pr.approved_by(reviews(("me", "APPROVED"), ("me", "COMMENTED")), "me"), "a comment keeps the approval")
        self.assertFalse(pr.approved_by(reviews(("me", "APPROVED"), ("me", "CHANGES_REQUESTED")), "me"))
        self.assertFalse(pr.approved_by(reviews(("me", "APPROVED"), ("me", "DISMISSED")), "me"))
        self.assertFalse(pr.approved_by(reviews(("me", "COMMENTED")), "me"))
        self.assertFalse(pr.approved_by(reviews(("other", "APPROVED")), "me"), "others do not count")

    def test_changes_review_is_the_newest_review_asking_for_changes(self):
        raw = {"reviews": [{"id": "a", "state": "CHANGES_REQUESTED"}, {"id": "b", "state": "CHANGES_REQUESTED"}, {"id": "c", "state": "COMMENTED"}]}
        self.assertEqual(pr.changes_review(raw), "b")
        self.assertIsNone(pr.changes_review({}))


class CommentsTest(unittest.TestCase):
    def test_only_feedback_asks_for_a_change(self):
        self.assertTrue(pr.asks_for_change("alice", "rename", "me", True, False))
        self.assertFalse(pr.asks_for_change("alice", "looks good", "me", False, False))
        self.assertTrue(pr.asks_for_change("alice", "```suggestion\nx\n```", "me", False, False))
        self.assertTrue(pr.asks_for_change("me", "why not remove this?", "me", True, False))
        self.assertFalse(pr.asks_for_change("me", "will do", "me", True, True))
        self.assertFalse(pr.asks_for_change("me", "will do", "me", False, False))
        self.assertTrue(pr.asks_for_change("me", "@change use a map", "me", False, False))

    def test_commented_review_bodies_ask_for_a_change(self):
        def review(who, state, body):
            return {"id": f"{who}-{state}", "state": state, "body": body, "author": {"login": who}}
        raw = {"reviews": [
            review("qa", "COMMENTED", "v2 returns 200, expected 400"),
            review("bob", "COMMENTED", ""),
            review("carol", "APPROVED", "🚀"),
            review("dave", "CHANGES_REQUESTED", "rename"),
            review("me", "COMMENTED", "done"),
            review("me", "COMMENTED", "@change drop the flag"),
        ]}
        self.assertEqual(pr.review_comments(raw, "me"), ["review-qa-COMMENTED", "review-me-COMMENTED"])
        self.assertEqual(pr.review_comments({}, "me"), [])


class EntryTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.path = os.path.join(self.dir.name, "workitem.md")
        with open(self.path, "w") as f:
            f.write("---\ntitle: t\nstage: in_review\nagent_status: ok\n---\n\n# body\nstage: not frontmatter\n")

    def tearDown(self):
        self.dir.cleanup()

    def test_a_new_item_has_nothing_handled_and_writes_nothing_until_something_changes(self):
        before = pr.read_entry(self.path)
        self.assertEqual(before, {"attempts": 0, "conflict_head": "", "handled_review": "", "seen_comments": None})
        text = open(self.path).read()
        pr.write_entry(self.path, before, dict(before))
        self.assertEqual(open(self.path).read(), text)

    def test_what_was_handled_is_kept_in_the_frontmatter_and_read_back(self):
        before = pr.read_entry(self.path)
        after = {"attempts": 2, "conflict_head": "abc", "handled_review": "r1", "seen_comments": ["inline-5", "c9"]}
        pr.write_entry(self.path, before, after)
        text = open(self.path).read()
        self.assertIn("pr_seen_comments: inline-5,c9\n", text)
        self.assertTrue(text.endswith("# body\nstage: not frontmatter\n"), "the body is untouched")
        self.assertEqual(pr.read_entry(self.path), after)
        pr.write_entry(self.path, after, {**after, "attempts": 0})
        self.assertEqual(pr.read_entry(self.path)["attempts"], 0)
        self.assertEqual(open(self.path).read().count("pr_fix_attempts:"), 1, "a key is replaced, not repeated")

    def test_seen_comments_can_be_empty_yet_known(self):
        before = pr.read_entry(self.path)
        pr.write_entry(self.path, before, {**before, "seen_comments": []})
        self.assertEqual(pr.read_entry(self.path)["seen_comments"], [])

    def test_set_status_edits_the_frontmatter_line_only(self):
        pr.set_status(self.path, "blocked: x")
        self.assertIn("agent_status: blocked: x\n", open(self.path).read())


if __name__ == "__main__":
    unittest.main()
