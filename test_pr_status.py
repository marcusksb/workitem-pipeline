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

    def test_only_the_newest_run_of_a_check_counts(self):
        old = {**run("RSpec", "COMPLETED", "CANCELLED"), "startedAt": "2026-11-25T11:47:00Z"}
        new = {**run("RSpec", "COMPLETED", "SUCCESS"), "startedAt": "2026-11-25T11:53:00Z"}
        old_result = {**run("RSpec result", "COMPLETED", "FAILURE"), "startedAt": "2026-11-25T11:47:30Z"}
        new_result = {**run("RSpec result", "COMPLETED", "SUCCESS"), "startedAt": "2026-11-25T11:58:00Z"}
        self.assertEqual(pr.latest_checks([old, new, old_result, new_result]), [new, new_result])
        self.assertEqual(pr.latest_checks([new_result, old_result, new, old]), [new_result, new], "order does not matter")
        status_old = {"context": "ci/build", "state": "FAILURE", "createdAt": "2026-11-25T11:00:00Z"}
        status_new = {"context": "ci/build", "state": "SUCCESS", "createdAt": "2026-11-25T12:00:00Z"}
        self.assertEqual(pr.latest_checks([status_old, status_new]), [status_new])

    def test_a_queued_rerun_without_a_time_is_the_newest(self):
        old = {**run("RSpec", "COMPLETED", "CANCELLED"), "startedAt": "2026-11-25T11:47:00Z"}
        queued = {**run("RSpec", "QUEUED", None), "startedAt": None, "completedAt": None}
        self.assertEqual(pr.latest_checks([old, queued]), [queued])
        self.assertEqual(pr.ci_of(pr.latest_checks([old, queued])), "pending")

    def test_the_same_job_in_two_workflows_is_two_checks(self):
        def in_workflow(check, workflow):
            return {**check, "checkSuite": {"workflowRun": {"workflow": {"name": workflow}}}}
        ci = in_workflow({**run("build", "COMPLETED", "SUCCESS"), "startedAt": "2026-11-25T12:00:00Z"}, "CI")
        release = in_workflow({**run("build", "COMPLETED", "FAILURE"), "startedAt": "2026-11-25T11:00:00Z"}, "Release")
        self.assertEqual(pr.latest_checks([ci, release]), [ci, release])
        self.assertEqual(pr.ci_of(pr.latest_checks([ci, release])), "failing")


def pull(comments=(), reviews=(), committed="2026-11-02T10:00:00Z", created="2026-11-01T10:00:00Z"):
    return {"createdAt": created, "commits": {"nodes": [{"commit": {"committedDate": committed}}]},
            "comments": {"nodes": list(comments)}, "reviews": {"nodes": list(reviews)}}


def note(at, login, body="hi", kind="User", key="createdAt"):
    return {"id": f"{login}-{at}", key: at, "body": body, "author": {"__typename": kind, "login": login}}


class ActivityTest(unittest.TestCase):
    def test_latest_of_opening_last_commit_and_human_reviews_and_comments(self):
        self.assertEqual(pr.activity_of(pull()), "2026-11-02T10:00:00Z")
        self.assertEqual(pr.activity_of(pull(comments=[note("2026-11-03T09:00:00Z", "bob")])), "2026-11-03T09:00:00Z")
        review = note("2026-11-04T09:00:00Z", "carol", key="submittedAt")
        self.assertEqual(pr.activity_of(pull(comments=[note("2026-11-03T09:00:00Z", "bob")], reviews=[review])), "2026-11-04T09:00:00Z")

    def test_bots_reminders_and_deleted_authors_do_not_count(self):
        ignored = [note("2026-11-05T00:00:00Z", "vercel", kind="Bot"),
                   note("2026-11-05T00:00:00Z", "renovate[bot]"),
                   note("2026-11-05T00:00:00Z", "me", body="<!-- pr-remind -->\n@bob nudge"),
                   {"createdAt": "2026-11-05T00:00:00Z", "body": "x", "author": None}]
        self.assertEqual(pr.activity_of(pull(comments=ignored)), "2026-11-02T10:00:00Z")


class FactsTest(unittest.TestCase):
    def test_approval_merge_and_both_ci_values_map_from_github(self):
        raw = {"state": "OPEN", "mergeable": "CONFLICTING", "reviewDecision": "CHANGES_REQUESTED", "headRefOid": "h1",
               "statusCheckRollup": [run("e2e", "COMPLETED", "FAILURE"), run("lint", "COMPLETED", "SUCCESS")]}
        self.assertEqual(pr.facts_of({**raw, "isDraft": True}, "me", IGNORE), {
            "pr_state": "open", "pr_ci": "failing", "pr_ci_fix": "passing", "pr_ci_failed": "e2e", "pr_merge": "conflicting",
            "pr_approval": "changes_requested", "pr_approved_by_me": "false", "pr_draft": "true", "pr_head": "h1",
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
import json, os, re, sys
query = sys.argv[4].split("=", 1)[1]
data = json.load(open(os.environ["STUB_PR"]))
kind = "threads" if "reviewThreads" in query else "pull"
open(os.environ["STUB_PR"] + ".calls", "a").write(kind + "\\n")
pulls = {alias: data[kind] for alias in re.findall(r"(p\\d+): pullRequest", query)}
print(json.dumps({"data": {"viewer": {"login": "me"}, "repository": pulls}}))
"""


def node(view, inline=(), activity=None):
    """A PR as the GraphQL query returns it, from the shape of `gh pr view --json`."""
    act = activity or pull()
    checks = view.get("statusCheckRollup")
    commit = {"committedDate": act["commits"]["nodes"][0]["commit"]["committedDate"] if act["commits"]["nodes"] else "",
              "statusCheckRollup": {"contexts": {"nodes": checks}} if checks else None}
    return {**{k: v for k, v in view.items() if k not in ("statusCheckRollup", "comments", "reviews")},
            "updatedAt": view.get("updatedAt", json.dumps([view, list(inline)])),
            "createdAt": act["createdAt"], "commits": {"nodes": [{"commit": commit}]},
            "comments": {"nodes": view.get("comments", []) + act["comments"]["nodes"]},
            "reviews": {"nodes": view.get("reviews", []) + act["reviews"]["nodes"]}}


def threads(inline):
    return {"reviewThreads": {"nodes": [{"comments": {"nodes": [
        {"databaseId": c["id"], "body": c["body"], "author": {"login": c["user"]["login"]},
         "replyTo": {"databaseId": c["in_reply_to_id"]} if c.get("in_reply_to_id") else None}]}} for c in inline]}}


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

    def pr(self, view, inline=(), activity=None):
        with open(self.stub, "w") as f:
            json.dump({"pull": node(view, inline, activity), "threads": threads(inline)}, f)

    def run_script(self, pr_url="https://github.com/o/r/pull/1"):
        self.run_items([{"dir": "item", "pr": pr_url}, {"dir": "other", "pr": ""}])

    def run_items(self, items):
        env = {**os.environ, "WORKITEM_GH": self.gh, "WORKITEM_DIR_ROOT": self.tmp.name, "STUB_PR": self.stub}
        env.pop("WORKITEM_MOVE", None)
        subprocess.run([sys.executable, PATH], input=json.dumps(items), text=True, env=env, check=True)

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

    def test_writes_required_ci_over_the_required_checks_and_activity(self):
        checks = [{**run("lint", "COMPLETED", "SUCCESS"), "isRequired": True}, {**run("rspec", "COMPLETED", "FAILURE"), "isRequired": True},
                  {**run("deploy", "IN_PROGRESS", None), "isRequired": False}]
        view = {"state": "OPEN", "mergeable": "MERGEABLE", "reviewDecision": "", "headRefOid": "h1", "comments": [],
                "statusCheckRollup": checks}
        self.pr(view, activity=pull(comments=[note("2026-11-03T09:00:00Z", "bob")]))
        self.run_script()
        f = self.fields()
        self.assertEqual((f["pr_ci_required"], f["pr_activity_at"]), ("failing", "2026-11-03T09:00:00Z"))

    def test_a_cancelled_older_run_does_not_fail_a_green_pr(self):
        def check(name, conclusion, started):
            return {**run(name, "COMPLETED", conclusion), "startedAt": started, "isRequired": True}
        checks = [check("RSpec", "CANCELLED", "2026-11-25T11:47:00Z"), check("RSpec result", "FAILURE", "2026-11-25T11:47:30Z"),
                  check("RSpec", "SUCCESS", "2026-11-25T11:53:00Z"), check("RSpec result", "SUCCESS", "2026-11-25T11:58:00Z")]
        self.pr({"state": "OPEN", "mergeable": "MERGEABLE", "reviewDecision": "", "headRefOid": "h1", "comments": [],
                 "statusCheckRollup": checks})
        self.run_script()
        f = self.fields()
        self.assertEqual((f["pr_ci"], f["pr_ci_fix"], f["pr_ci_required"], f.get("pr_ci_failed", "")),
                         ("passing", "passing", "passing", ""))

    def test_pr_head_at_is_when_the_current_head_was_first_seen(self):
        view = {"state": "OPEN", "mergeable": "MERGEABLE", "reviewDecision": "", "headRefOid": "h1", "comments": []}
        self.pr(view)
        self.run_script()
        first = self.fields()["pr_head_at"]
        self.run_script()
        self.assertEqual(self.fields()["pr_head_at"], first, "the same head keeps its time")
        self.pr({**view, "headRefOid": "h2"})
        self.run_script()
        self.assertGreater(self.fields()["pr_head_at"], first, "a new head gets a new time")
        pr.set_fields(self.path, {"pr_head_at": None})
        self.run_script()
        self.assertIn("pr_head_at", self.fields(), "an item read before pr_head_at existed gets one")

    def test_a_merged_pr_is_not_read_again_and_a_new_link_starts_over(self):
        self.pr({"state": "MERGED"})
        self.run_script()
        self.assertEqual(self.fields()["pr_state"], "merged")
        self.run_script()
        self.assertEqual(self.text(self.stub + ".calls").count("pull"), 1, "merged is final")
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


    def test_every_pr_of_a_repo_is_one_query_and_threads_are_read_only_when_the_pr_moved(self):
        view = {"state": "OPEN", "mergeable": "MERGEABLE", "reviewDecision": "", "headRefOid": "h1", "comments": [], "updatedAt": "t1"}
        mine = {"id": 7, "user": {"login": "me"}, "body": "x", "in_reply_to_id": None}
        self.pr(view, inline=[mine])
        second = os.path.join(self.tmp.name, "second", "workitem.md")
        os.makedirs(os.path.dirname(second))
        with open(second, "w") as f:
            f.write("---\ntitle: t\nstage: in_review\npr: https://github.com/o/r/pull/2\n---\n")
        items = [{"dir": "item", "pr": "https://github.com/o/r/pull/1"}, {"dir": "second", "pr": "https://github.com/o/r/pull/2"}]
        self.run_items(items)
        self.assertEqual(self.text(self.stub + ".calls").split(), ["pull", "threads"], "both PRs in each query")
        self.assertEqual(pr.read_fields(second)["pr_updated_at"], "t1")

        pr.set_fields(self.path, {"pr_seen_comments": "", "pr_unseen_comments": "inline-7"})
        self.run_items(items)
        self.assertEqual(self.text(self.stub + ".calls").split(), ["pull", "threads", "pull"], "nothing moved, no threads")
        self.assertEqual(self.fields()["pr_unseen_comments"], "inline-7", "an unseen inline comment stays unseen")

        self.pr({**view, "updatedAt": "t2"}, inline=[mine, {**mine, "id": 8, "in_reply_to_id": 7}, {**mine, "id": 9, "user": {"login": "bob"}}])
        self.run_items(items)
        self.assertEqual(self.text(self.stub + ".calls").split()[-1], "threads")
        self.assertEqual(self.fields()["pr_unseen_comments"], "inline-7,inline-9", "my own reply is conversation")


BKT_STUB = """#!/usr/bin/env python3
import json, os, sys
path = sys.argv[2]
with open(os.environ["STUB_BKT_CALLS"], "a") as f:
    f.write(path + "\\n")
data = json.load(open(os.environ["STUB_BKT"]))
print(json.dumps(data.get(path, {})))
"""


def bb_pull(n, **kw):
    """A Cloud pull request as the REST list returns it."""
    return {"id": n, "state": "OPEN", "merge_state": "open", "draft": False,
            "author": {"user": {"uuid": "{me-uuid}", "nickname": "me"}},
            "source": {"commit": {"hash": "h1"}},
            "created_on": "2026-11-01T10:00:00Z", "updated_on": "2026-11-02T10:00:00Z",
            "participants": [{"user": {"uuid": "{bob-uuid}", "nickname": "bob"}, "approved": True,
                              "participated_on": "2026-11-03T09:00:00Z"}], **kw}


def bb_status(name, state, created="2026-11-02T10:00:00Z", updated="2026-11-02T10:00:00Z"):
    return {"name": name, "state": state, "created_on": created, "updated_on": updated}


def bb_comment(id, login, body, created="2026-11-03T09:00:00Z", **kw):
    return {"id": id, "user": {"nickname": login}, "content": {"raw": body}, "created_on": created, **kw}


class BitbucketReaderTest(unittest.TestCase):
    """The whole script against a stub bkt, with a stub gh alongside for the mixed board."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = self.tmp.name
        self.bkt = os.path.join(root, "bkt")
        with open(self.bkt, "w") as f:
            f.write(BKT_STUB)
        os.chmod(self.bkt, os.stat(self.bkt).st_mode | stat.S_IEXEC)
        self.stub = os.path.join(root, "bb.json")
        self.calls = os.path.join(root, "bkt.calls")
        self.gh = os.path.join(root, "gh")
        with open(self.gh, "w") as f:
            f.write(GH_STUB)
        os.chmod(self.gh, os.stat(self.gh).st_mode | stat.S_IEXEC)
        self.gh_stub = os.path.join(root, "pr.json")
        self.gh_fixture = {"pull": node({"state": "OPEN", "mergeable": "MERGEABLE", "reviewDecision": "",
                                         "headRefOid": "h1", "comments": []}), "threads": threads([])}
        self.path = os.path.join(root, "item", "workitem.md")
        os.makedirs(os.path.dirname(self.path))
        self.write_item("https://bitbucket.org/ws/slug/pull-requests/1")
        self.fixture = {}

    def tearDown(self):
        self.tmp.cleanup()

    def write_item(self, pr_url, extra=""):
        with open(self.path, "w") as f:
            f.write(f"---\ntitle: t\nstage: in_review\npr: {pr_url}\nagent_status: ok\n{extra}---\n\n# body\n")

    def run_script(self, pr_url="https://bitbucket.org/ws/slug/pull-requests/1"):
        self.run_items([{"dir": "item", "pr": pr_url}, {"dir": "other", "pr": ""}])

    def run_items(self, items):
        with open(self.stub, "w") as f:
            json.dump(self.fixture, f)
        with open(self.gh_stub, "w") as f:
            json.dump(self.gh_fixture, f)
        env = {**os.environ, "WORKITEM_BKT": self.bkt, "WORKITEM_GH": self.gh,
               "WORKITEM_DIR_ROOT": self.tmp.name, "STUB_BKT": self.stub, "STUB_BKT_CALLS": self.calls,
               "STUB_PR": self.gh_stub}
        env.pop("WORKITEM_MOVE", None)
        subprocess.run([sys.executable, PATH], input=json.dumps(items), text=True, env=env, check=True)

    def fields(self):
        return pr.read_fields(self.path)

    def text(self, path):
        with open(path) as f:
            return f.read()

    def calls_text(self):
        with open(self.calls) as f:
            return f.read()

    def test_writes_the_facts_of_a_bitbucket_pr(self):
        self.fixture = {
            "/user": {"nickname": "me"},
            "/repositories/ws/slug/pullrequests": {"values": [bb_pull(1)]},
            "/repositories/ws/slug/commit/h1": {"hash": "h1", "date": "2026-11-02T10:00:00Z"},
            "/repositories/ws/slug/commit/h1/statuses": {"values": [bb_status("lint", "FAILED")]},
        }
        self.run_script()
        f = self.fields()
        self.assertEqual((f["pr_state"], f["pr_ci"], f["pr_ci_fix"], f["pr_ci_failed"], f["pr_head"]),
                         ("open", "failing", "failing", "lint", "h1"))
        self.assertEqual((f["pr_merge"], f["pr_approval"], f["pr_approved_by_me"], f["pr_draft"]),
                         ("mergeable", "approved", "false", "false"))
        self.assertEqual(f["pr_ci_required"], "failing", "every Cloud status counts as required")
        self.assertEqual(f["pr_activity_at"], "2026-11-03T09:00:00Z", "the approval counts as activity")
        self.assertEqual(f["pr_updated_at"], "2026-11-02T10:00:00Z")
        self.assertIn("pr_head_at", f)

    def test_the_ci_fixer_ignores_end_to_end_suites(self):
        self.fixture = {
            "/user": {"nickname": "me"},
            "/repositories/ws/slug/pullrequests": {"values": [bb_pull(1)]},
            "/repositories/ws/slug/commit/h1": {"hash": "h1", "date": "2026-11-02T10:00:00Z"},
            "/repositories/ws/slug/commit/h1/statuses": {"values": [bb_status("e2e", "FAILED")]},
        }
        self.run_script()
        self.assertEqual((self.fields()["pr_ci"], self.fields()["pr_ci_fix"]), ("failing", "none"))

    def test_a_conflicting_merge_state_is_a_conflict(self):
        self.fixture = {
            "/user": {"nickname": "me"},
            "/repositories/ws/slug/pullrequests": {"values": [bb_pull(1, merge_state="conflicted")]},
            "/repositories/ws/slug/commit/h1": {"hash": "h1", "date": "2026-11-02T10:00:00Z"},
            "/repositories/ws/slug/commit/h1/statuses": {"values": []},
        }
        self.run_script()
        self.assertEqual(self.fields()["pr_merge"], "conflicting")

    def test_comments_are_read_only_when_the_pr_moved_and_replies_are_conversation(self):
        self.fixture = {
            "/user": {"nickname": "me"},
            "/repositories/ws/slug/pullrequests": {"values": [bb_pull(1)]},
            "/repositories/ws/slug/commit/h1": {"hash": "h1", "date": "2026-11-02T10:00:00Z"},
            "/repositories/ws/slug/commit/h1/statuses": {"values": []},
            "/repositories/ws/slug/pullrequests/1/comments": {"values": [
                bb_comment(10, "bob", "rename this", created="2026-11-03T09:00:00Z"),
                bb_comment(11, "me", "will do", created="2026-11-03T09:10:00Z", parent={"id": 10}),
                bb_comment(12, "bob", "nit", created="2026-11-03T09:20:00Z", inline={"from": 1, "to": 2})]},
        }
        self.run_script()
        f = self.fields()
        self.assertEqual((f["pr_seen_comments"], f["pr_unseen_comments"]), ("inline-12,10", ""),
                         "what is there on the first reading is not new")
        self.assertEqual(f["pr_activity_at"], "2026-11-03T09:10:00Z", "the newest comment counts as activity")
        self.run_script()
        self.assertEqual(self.fields()["pr_unseen_comments"], "")

    def test_a_deleted_comment_is_not_activity_and_not_unseen(self):
        self.fixture = {
            "/user": {"nickname": "me"},
            "/repositories/ws/slug/pullrequests": {"values": [bb_pull(1)]},
            "/repositories/ws/slug/commit/h1": {"hash": "h1", "date": "2026-11-02T10:00:00Z"},
            "/repositories/ws/slug/commit/h1/statuses": {"values": []},
            "/repositories/ws/slug/pullrequests/1/comments": {"values": [
                bb_comment(10, "bob", "rename this", created="2026-11-03T09:00:00Z"),
                bb_comment(11, "bob", "actually never mind", created="2026-11-03T09:30:00Z", deleted=True),
                bb_comment(12, "bob", "another nit", created="2026-11-03T09:20:00Z", inline={"from": 1, "to": 2}, deleted=True)]},
        }
        self.run_script()
        f = self.fields()
        self.assertEqual((f["pr_seen_comments"], f["pr_unseen_comments"]), ("10", ""),
                         "a deleted comment is not there, inline or not")
        self.assertEqual(f["pr_activity_at"], "2026-11-03T09:00:00Z", "a deleted comment is not activity")

    def test_unseen_comments_survive_a_pass_that_does_not_move_the_pr(self):
        self.fixture = {
            "/user": {"nickname": "me"},
            "/repositories/ws/slug/pullrequests": {"values": [bb_pull(1)]},
            "/repositories/ws/slug/commit/h1": {"hash": "h1", "date": "2026-11-02T10:00:00Z"},
            "/repositories/ws/slug/commit/h1/statuses": {"values": []},
        }
        self.run_script()
        self.assertEqual(self.fields()["pr_unseen_comments"], "")
        # A comment arrives and the PR moves with it: the reading reports it as unseen.
        self.fixture = {
            **self.fixture,
            "/repositories/ws/slug/pullrequests": {"values": [bb_pull(1, updated_on="2026-11-02T11:00:00Z")]},
            "/repositories/ws/slug/pullrequests/1/comments": {"values": [bb_comment(10, "bob", "rename this")]},
        }
        self.run_script()
        self.assertEqual(self.fields()["pr_unseen_comments"], "10")
        self.assertEqual(self.fields()["pr_activity_at"], "2026-11-03T09:00:00Z")
        # The next pass sees the same updated_on, so the comments are not read again; the unseen
        # comment and the activity time it counted must not be lost.
        self.run_script()
        f = self.fields()
        self.assertEqual(f["pr_unseen_comments"], "10")
        self.assertEqual(f["pr_activity_at"], "2026-11-03T09:00:00Z")

    def test_a_link_with_a_suffix_still_reads_and_a_bad_one_does_not_stop_the_run(self):
        other = os.path.join(self.tmp.name, "other", "workitem.md")
        os.makedirs(os.path.dirname(other))
        with open(other, "w") as f:
            f.write("---\ntitle: t\nstage: in_review\npr: https://bitbucket.org/ws/slug/pull-requests/1\n---\n")
        self.fixture = {
            "/user": {"nickname": "me"},
            "/repositories/ws/slug/pullrequests": {"values": [bb_pull(1)]},
            "/repositories/ws/slug/commit/h1": {"hash": "h1", "date": "2026-11-02T10:00:00Z"},
            "/repositories/ws/slug/commit/h1/statuses": {"values": [bb_status("lint", "SUCCESSFUL")]},
        }
        self.run_items([{"dir": "item", "pr": "https://bitbucket.org/ws/slug/pull-requests/1?email=test@example.com"},
                        {"dir": "other", "pr": "https://bitbucket.org/ws/slug/pull-requests/1/diff"}])
        f = self.fields()
        self.assertEqual((f["pr_state"], f["pr_ci"]), ("open", "passing"), "suffixes after the number are ignored")
        self.assertEqual(pr.read_fields(other)["pr_ci"], "passing")
        self.run_items([{"dir": "item", "pr": "https://bitbucket.org/ws/slug/branch/nope"},
                        {"dir": "other", "pr": "https://bitbucket.org/ws/slug/pull-requests/1"}])
        self.assertEqual(pr.read_fields(other)["pr_ci"], "passing", "a bad link errors alone")

    def test_pagination_follows_the_next_link(self):
        other = os.path.join(self.tmp.name, "other", "workitem.md")
        os.makedirs(os.path.dirname(other))
        with open(other, "w") as f:
            f.write("---\ntitle: t\nstage: in_review\npr: https://bitbucket.org/ws/slug/pull-requests/2\n---\n")
        self.fixture = {
            "/user": {"nickname": "me"},
            "/repositories/ws/slug/pullrequests": {"values": [bb_pull(1)],
                                                   "next": "/2.0/repositories/ws/slug/pullrequests?page=2"},
            "/2.0/repositories/ws/slug/pullrequests": {"values": [bb_pull(2, source={"commit": {"hash": "h2"}})]},
            "/repositories/ws/slug/commit/h1": {"hash": "h1", "date": "2026-11-02T10:00:00Z"},
            "/repositories/ws/slug/commit/h2": {"hash": "h2", "date": "2026-11-02T11:00:00Z"},
            "/repositories/ws/slug/commit/h1/statuses": {"values": []},
            "/repositories/ws/slug/commit/h2/statuses": {"values": []},
        }
        self.run_items([{"dir": "item", "pr": "https://bitbucket.org/ws/slug/pull-requests/1"},
                        {"dir": "other", "pr": "https://bitbucket.org/ws/slug/pull-requests/2"}])
        self.assertEqual((self.fields()["pr_head"], pr.read_fields(other)["pr_head"]), ("h1", "h2"),
                         "the PR on page 2 was read too")

    def test_a_merged_pr_is_not_read_again_and_a_new_link_starts_over(self):
        self.fixture = {
            "/user": {"nickname": "me"},
            "/repositories/ws/slug/pullrequests": {"values": []},
            "/repositories/ws/slug/pullrequests/1": {"id": 1, "state": "MERGED", "merge_state": "open", "draft": False,
                "author": {"user": {"uuid": "{me-uuid}", "nickname": "me"}},
                "source": {"commit": {"hash": "h1"}}, "created_on": "2026-11-01T10:00:00Z",
                "updated_on": "2026-11-02T10:00:00Z", "participants": []},
        }
        self.run_script()
        self.assertEqual(self.fields()["pr_state"], "merged")
        calls_before = self.calls_text()
        self.run_script()
        self.assertEqual(self.calls_text(), calls_before, "merged is final")
        self.write_item("https://bitbucket.org/ws/slug/pull-requests/2",
                        extra="pr_url: https://bitbucket.org/ws/slug/pull-requests/1\npr_state: merged\npr_seen_comments: old\n")
        self.fixture["/repositories/ws/slug/pullrequests/2"] = {**bb_pull(2), "source": {"commit": {"hash": "h2"}}}
        self.run_script("https://bitbucket.org/ws/slug/pull-requests/2")
        f = self.fields()
        self.assertEqual((f["pr_url"], f["pr_state"], f["pr_seen_comments"]),
                         ("https://bitbucket.org/ws/slug/pull-requests/2", "open", ""))

    def test_github_and_bitbucket_prs_are_read_by_their_own_host(self):
        other = os.path.join(self.tmp.name, "other", "workitem.md")
        os.makedirs(os.path.dirname(other))
        with open(other, "w") as f:
            f.write("---\ntitle: t\nstage: in_review\npr: https://github.com/o/r/pull/1\n---\n")
        self.fixture = {
            "/user": {"nickname": "me"},
            "/repositories/ws/slug/pullrequests": {"values": [bb_pull(1)]},
            "/repositories/ws/slug/commit/h1": {"hash": "h1", "date": "2026-11-02T10:00:00Z"},
            "/repositories/ws/slug/commit/h1/statuses": {"values": [bb_status("lint", "SUCCESSFUL")]},
        }
        self.run_items([{"dir": "item", "pr": "https://bitbucket.org/ws/slug/pull-requests/1"},
                        {"dir": "other", "pr": "https://github.com/o/r/pull/1"}])
        self.assertIn("/repositories/ws/slug/pullrequests", self.calls_text(), "bkt reads the Bitbucket PR")
        self.assertEqual(self.text(self.gh_stub + ".calls").split(), ["pull", "threads"], "gh reads the GitHub PR")
        self.assertEqual(self.fields()["pr_ci"], "passing")
        self.assertEqual(pr.read_fields(other)["pr_ci"], "none")


if __name__ == "__main__":
    unittest.main()
