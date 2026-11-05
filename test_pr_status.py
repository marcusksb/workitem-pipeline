import importlib.machinery, importlib.util, json, os, stat, subprocess, sys, tempfile, unittest

sys.dont_write_bytecode = True

HERE = os.path.dirname(os.path.abspath(__file__))
PATH = os.path.join(HERE, "skills/workitem-pipeline/watch/pr-status")
loader = importlib.machinery.SourceFileLoader("pr_status", PATH)
spec = importlib.util.spec_from_loader("pr_status", loader)
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

    def test_failed_checks_lists_each_red_check_once(self):
        failed = pr.failed_checks([run("lint", "COMPLETED", "SUCCESS"), run("RSpec", "COMPLETED", "FAILURE"), run("RSpec", "COMPLETED", "FAILURE")])
        self.assertEqual(failed, ["RSpec"])


class FactsTest(unittest.TestCase):
    def test_approval_merge_and_both_ci_values_map_from_github(self):
        raw = {"state": "OPEN", "mergeable": "CONFLICTING", "reviewDecision": "CHANGES_REQUESTED", "headRefOid": "h1",
               "statusCheckRollup": [run("e2e", "COMPLETED", "FAILURE"), run("lint", "COMPLETED", "SUCCESS")]}
        self.assertEqual(pr.facts_of(raw, "me", IGNORE), {
            "pr_state": "open", "pr_ci": "failing", "pr_ci_fix": "passing", "pr_ci_failed": "e2e", "pr_merge": "conflicting",
            "pr_approval": "changes_requested", "pr_approved_by_me": "false", "pr_head": "h1",
        })
        facts = pr.facts_of({"state": "OPEN", "mergeable": "UNKNOWN", "reviewDecision": ""}, "me", IGNORE)
        self.assertEqual((facts["pr_approval"], facts["pr_merge"], facts["pr_ci"]), ("none", "unknown", "none"))

    def test_approved_by_me_follows_my_latest_verdict(self):
        def reviews(*pairs):
            return {"reviews": [{"id": "x", "state": state, "author": {"login": who}} for who, state in pairs]}
        self.assertTrue(pr.approved_by(reviews(("me", "APPROVED")), "me"))
        self.assertTrue(pr.approved_by(reviews(("me", "APPROVED"), ("me", "COMMENTED")), "me"), "a comment keeps the approval")
        self.assertFalse(pr.approved_by(reviews(("me", "APPROVED"), ("me", "CHANGES_REQUESTED")), "me"))
        self.assertFalse(pr.approved_by(reviews(("me", "APPROVED"), ("me", "DISMISSED")), "me"))
        self.assertFalse(pr.approved_by(reviews(("me", "COMMENTED")), "me"))
        self.assertFalse(pr.approved_by(reviews(("other", "APPROVED")), "me"), "others do not count")


class CommentsTest(unittest.TestCase):
    def test_only_feedback_asks_for_a_change(self):
        self.assertTrue(pr.asks_for_change("alice", "rename", "me", True, False))
        self.assertTrue(pr.asks_for_change("alice", "looks good", "me", False, False))
        self.assertTrue(pr.asks_for_change("github-actions[bot]", "Verdict: Requires Additional Work", "me", False, False))
        self.assertTrue(pr.asks_for_change("me", "why not remove this?", "me", True, False))
        self.assertFalse(pr.asks_for_change("me", "will do", "me", True, True))
        self.assertFalse(pr.asks_for_change("me", "will do", "me", False, False))
        self.assertTrue(pr.asks_for_change("me", "@change use a map", "me", False, False))

    def test_reviews_that_ask_for_a_change_include_every_changes_request(self):
        def review(id, who, state, body):
            return {"id": id, "state": state, "body": body, "author": {"login": who}}
        raw = {"reviews": [
            review("r1", "qa", "COMMENTED", "v2 returns 200, expected 400"),
            review("r2", "bob", "COMMENTED", ""),
            review("r3", "carol", "APPROVED", "🚀"),
            review("r4", "dave", "CHANGES_REQUESTED", ""),
            review("r5", "me", "COMMENTED", "done"),
            review("r6", "me", "COMMENTED", "@change drop the flag"),
        ]}
        self.assertEqual(pr.review_comments(raw, "me"), ["review-r1", "review-r4", "review-r6"])
        self.assertEqual(pr.review_comments({}, "me"), [])

    def test_the_first_reading_sees_everything_and_later_ones_report_the_new(self):
        self.assertEqual(pr.comment_updates({}, ["a", "b"]), {"pr_seen_comments": "a,b", "pr_unseen_comments": ""})
        self.assertEqual(pr.comment_updates({"pr_seen_comments": ""}, ["a"]), {"pr_seen_comments": "", "pr_unseen_comments": "a"})
        self.assertEqual(pr.comment_updates({"pr_seen_comments": "a"}, ["a", "b", "c"]), {"pr_seen_comments": "a", "pr_unseen_comments": "b,c"})

    def test_an_old_handled_review_counts_as_seen(self):
        self.assertEqual(pr.comment_updates({"pr_seen_comments": "a", "pr_handled_review": "r1"}, ["a", "review-r1", "b"]),
                         {"pr_handled_review": None, "pr_seen_comments": "a,review-r1", "pr_unseen_comments": "b"})


GH_STUB = """#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
data = json.load(open(os.environ["STUB_PR"]))
if args[:2] == ["api", "user"]:
    print(json.dumps({"login": "me"}))
elif args[0] == "api":
    print(json.dumps([data.get("inline", [])]))
else:
    open(os.environ["STUB_PR"] + ".calls", "a").write("view\\n")
    print(json.dumps(data["view"]))
"""


class ReaderTest(unittest.TestCase):
    """The whole script against a stub gh."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = self.tmp.name
        self.gh = os.path.join(root, "gh")
        with open(self.gh, "w") as f:
            f.write(GH_STUB)
        os.chmod(self.gh, os.stat(self.gh).st_mode | stat.S_IEXEC)
        self.stub = os.path.join(root, "pr.json")
        self.path = os.path.join(root, "item", "workitem.md")
        os.makedirs(os.path.dirname(self.path))
        self.write_item("https://github.com/o/r/pull/1")

    def tearDown(self):
        self.tmp.cleanup()

    def write_item(self, pr_url, extra=""):
        with open(self.path, "w") as f:
            f.write(f"---\ntitle: t\nstage: in_review\npr: {pr_url}\nagent_status: ok\n{extra}---\n\n# body\n")

    def pr(self, view, inline=()):
        with open(self.stub, "w") as f:
            json.dump({"view": view, "inline": list(inline)}, f)

    def run_script(self, pr_url="https://github.com/o/r/pull/1"):
        env = {**os.environ, "WORKITEM_GH": self.gh, "WORKITEM_DIR_ROOT": self.tmp.name, "STUB_PR": self.stub}
        env.pop("WORKITEM_MOVE", None)
        items = json.dumps([{"dir": "item", "pr": pr_url}, {"dir": "other", "pr": ""}])
        subprocess.run([sys.executable, PATH], input=items, text=True, env=env, check=True)

    def fields(self):
        return pr.read_fields(self.path)

    def text(self, path):
        with open(path) as f:
            return f.read()

    def test_writes_the_facts_and_only_new_comments_are_unseen(self):
        view = {"state": "OPEN", "mergeable": "MERGEABLE", "reviewDecision": "", "headRefOid": "h1",
                "statusCheckRollup": [run("lint", "COMPLETED", "FAILURE")], "comments": [{"id": "c1", "author": {"login": "bob"}}]}
        self.pr(view)
        self.run_script()
        f = self.fields()
        self.assertEqual((f["pr_state"], f["pr_ci"], f["pr_ci_fix"], f["pr_ci_failed"], f["pr_head"]), ("open", "failing", "failing", "lint", "h1"))
        self.assertEqual((f["pr_seen_comments"], f["pr_unseen_comments"]), ("c1", ""), "what is there on the first reading is not new")
        self.assertIn("pr_checked_at", f)
        self.pr({**view, "comments": view["comments"] + [{"id": "c2", "author": {"login": "bob"}}]},
                inline=[{"id": 7, "user": {"login": "me"}, "body": "x", "in_reply_to_id": None}])
        self.run_script()
        self.assertEqual(self.fields()["pr_unseen_comments"], "inline-7,c2")

    def test_a_merged_pr_is_not_read_again_and_a_new_link_starts_over(self):
        self.pr({"state": "MERGED"})
        self.run_script()
        self.assertEqual(self.fields()["pr_state"], "merged")
        self.run_script()
        self.assertEqual(self.text(self.stub + ".calls").count("view"), 1, "merged is final")
        self.write_item("https://github.com/o/r/pull/2", extra="pr_url: https://github.com/o/r/pull/1\npr_state: merged\npr_seen_comments: old\n")
        self.pr({"state": "OPEN", "mergeable": "MERGEABLE", "reviewDecision": "", "headRefOid": "h2", "comments": []})
        self.run_script("https://github.com/o/r/pull/2")
        f = self.fields()
        self.assertEqual((f["pr_url"], f["pr_state"], f["pr_seen_comments"]), ("https://github.com/o/r/pull/2", "open", ""))

    def test_unchanged_facts_leave_the_file_alone_unless_a_reading_is_awaited(self):
        self.pr({"state": "OPEN", "mergeable": "MERGEABLE", "reviewDecision": "", "headRefOid": "h1", "comments": []})
        self.run_script()
        before = self.text(self.path)
        self.assertTrue(before.endswith("---\n\n# body\n"), "the body is untouched")
        self.run_script()
        self.assertEqual(self.text(self.path), before, "agents that read the file see no write")

        pr.set_fields(self.path, {"pr_fix_ended_at": "9999-01-01T00:00:00.000Z"})
        self.run_script()
        self.assertNotEqual(self.fields()["pr_checked_at"], before.split("pr_checked_at: ")[1].split("\n")[0], "pr-review waits for it")


if __name__ == "__main__":
    unittest.main()
