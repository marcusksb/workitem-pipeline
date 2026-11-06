import importlib.machinery, importlib.util, io, json, os, stat, sys, tempfile, unittest
from contextlib import redirect_stdout
from datetime import datetime, timedelta, timezone
from unittest import mock

sys.dont_write_bytecode = True

HERE = os.path.dirname(os.path.abspath(__file__))
PATH = os.path.join(HERE, "skills/workitem-pipeline/watch/pr-remind")
loader = importlib.machinery.SourceFileLoader("pr_remind", PATH)
spec = importlib.util.spec_from_loader("pr_remind", loader)
pr = importlib.util.module_from_spec(spec)
loader.exec_module(pr)

CET = timezone(timedelta(hours=1))


def at(text):
    """A local time in CET, e.g. 'Mon 2026-11-09 10:00'."""
    return datetime.strptime(text.split(" ", 1)[1], "%Y-%m-%d %H:%M").replace(tzinfo=CET)


READY = {"pr_state": "open", "pr_draft": "false", "pr_merge": "mergeable", "pr_ci_required": "passing",
         "pr_approval": "review_required", "pr_activity_at": "2026-11-04T09:00:00Z"}  # Wed 10:00 CET


class DueTest(unittest.TestCase):
    def test_two_business_days_of_quiet(self):
        self.assertIsNone(pr.not_due(READY, at("Fri 2026-11-06 10:00")))
        self.assertIn("active", pr.not_due(READY, at("Fri 2026-11-06 09:59")))

    def test_the_weekend_does_not_count(self):
        friday = {**READY, "pr_activity_at": "2026-11-06T15:00:00Z"}  # Fri 16:00 CET
        self.assertIn("active", pr.not_due(friday, at("Mon 2026-11-09 16:59")))
        self.assertIn("active", pr.not_due(friday, at("Tue 2026-11-10 15:59")))
        self.assertIsNone(pr.not_due(friday, at("Tue 2026-11-10 16:00")))
        saturday = {**READY, "pr_activity_at": "2026-11-07T10:00:00Z"}
        self.assertIn("active", pr.not_due(saturday, at("Tue 2026-11-10 16:00")))
        self.assertIsNone(pr.not_due(saturday, at("Wed 2026-11-11 09:00")))

    def test_only_on_weekdays_during_office_hours(self):
        self.assertIn("outside", pr.not_due(READY, at("Fri 2026-11-06 17:00")))
        self.assertIn("outside", pr.not_due(READY, at("Mon 2026-11-09 08:59")))
        self.assertIn("outside", pr.not_due(READY, at("Sat 2026-11-07 12:00")))
        self.assertIsNone(pr.not_due(READY, at("Mon 2026-11-09 09:00")))

    def test_once_per_quiet_period(self):
        now = at("Fri 2026-11-06 12:00")
        self.assertIn("already", pr.not_due({**READY, "pr_reminded_at": "2026-11-06T09:00:00.000Z"}, now))
        self.assertIsNone(pr.not_due({**READY, "pr_reminded_at": "2026-11-02T09:00:00.000Z"}, now), "new activity since")

    def test_the_pr_must_only_wait_for_a_review(self):
        now = at("Fri 2026-11-06 12:00")
        for key, value in (("pr_state", "merged"), ("pr_draft", "true"), ("pr_merge", "conflicting"),
                           ("pr_ci_required", "pending"), ("pr_approval", "approved"), ("pr_activity_at", None)):
            fields = {k: v for k, v in {**READY, key: value}.items() if v is not None}
            self.assertIsNotNone(pr.not_due(fields, now), key)


def user(login):
    return {"__typename": "User", "login": login}


def pull(requested=(), reviews=()):
    return {"author": {"login": "me"},
            "reviewRequests": {"nodes": [{"requestedReviewer": r} for r in requested]},
            "latestReviews": {"nodes": [{"state": s, "author": a} for a, s in reviews]}}


class MentionsTest(unittest.TestCase):
    def test_pending_people_and_reviewers_who_did_not_approve(self):
        mentions = pr.mentions_of(pull(
            requested=[user("alice"), {"__typename": "Team", "combinedSlug": "org/fe"}],
            reviews=[(user("bob"), "COMMENTED"), (user("carol"), "CHANGES_REQUESTED"), (user("dave"), "APPROVED"),
                     ({"__typename": "Bot", "login": "cursor"}, "COMMENTED"), (user("me"), "COMMENTED")]))
        self.assertEqual(mentions, ["@alice", "@bob", "@carol (re-review after your feedback)"])

    def test_a_re_requested_reviewer_who_asked_for_changes_is_named_once(self):
        mentions = pr.mentions_of(pull(requested=[user("carol")], reviews=[(user("carol"), "CHANGES_REQUESTED")]))
        self.assertEqual(mentions, ["@carol (re-review after your feedback)"])

    def test_the_team_only_when_no_person_is_pending(self):
        team = {"__typename": "Team", "combinedSlug": "org/fe"}
        self.assertEqual(pr.mentions_of(pull(requested=[team], reviews=[(user("dave"), "APPROVED")])), ["@org/fe"])
        self.assertEqual(pr.mentions_of(pull()), [])


GH_STUB = """#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
data = json.load(open(os.environ["STUB"]))
open(os.environ["STUB"] + ".calls", "a").write(json.dumps(args) + "\\n")
if args[:2] == ["api", "graphql"]:
    print(json.dumps({"data": {"repository": {"pullRequest": data["pull"]}}}))
elif args[:2] == ["pr", "comment"]:
    if data.get("post_fails"):
        print("HTTP 502", file=sys.stderr)
        sys.exit(1)
    print("https://github.com/o/r/pull/1#issuecomment-1")
"""


class ReminderTest(unittest.TestCase):
    """The whole script against a stub gh, with an injected clock."""

    NOW = at("Fri 2026-11-06 12:00")

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = self.tmp.name
        self.gh = os.path.join(root, "gh")
        with open(self.gh, "w") as f:
            f.write(GH_STUB)
        os.chmod(self.gh, os.stat(self.gh).st_mode | stat.S_IEXEC)
        self.stub = os.path.join(root, "stub.json")
        self.path = os.path.join(root, "item", "workitem.md")
        os.makedirs(os.path.dirname(self.path))
        facts = "".join(f"{k}: {v}\n" for k, v in READY.items())
        with open(self.path, "w") as f:
            f.write(f"---\ntitle: t\nstage: in_review\npr: https://github.com/o/r/pull/1\nagent_status: ok\n{facts}---\n\n# body\n")

    def tearDown(self):
        self.tmp.cleanup()

    def stub_gh(self, pull, post_fails=False):
        with open(self.stub, "w") as f:
            json.dump({"pull": pull, "post_fails": post_fails}, f)

    def run_script(self, *argv, now=NOW):
        env = {"WORKITEM_GH": self.gh, "WORKITEM_DIR_ROOT": self.tmp.name, "STUB": self.stub}
        items = json.dumps([{"dir": "item", "pr": "https://github.com/o/r/pull/1"}, {"dir": "other", "pr": ""}])
        out = io.StringIO()
        with mock.patch.dict(os.environ, env), mock.patch("sys.stdin", io.StringIO(items)), redirect_stdout(out):
            pr.main(list(argv), now=now)
        return out.getvalue()

    def calls(self):
        if not os.path.exists(self.stub + ".calls"):
            return []
        with open(self.stub + ".calls") as f:
            return [json.loads(line) for line in f]

    def fields(self):
        return pr.read_fields(self.path)

    def text(self):
        with open(self.path) as f:
            return f.read()

    def test_posts_once_and_records_it(self):
        self.stub_gh(pull(requested=[user("alice")]))
        self.run_script()
        posts = [c for c in self.calls() if c[:2] == ["pr", "comment"]]
        self.assertEqual(len(posts), 1)
        self.assertEqual(posts[0][-1], pr.comment_of(["@alice"]))
        self.assertTrue(posts[0][-1].startswith("<!-- pr-remind -->\n@alice friendly nudge"))
        self.assertEqual(self.fields()["pr_reminded_at"], "2026-11-06T11:00:00.000Z")
        self.run_script(now=at("Mon 2026-11-09 12:00"))
        self.assertEqual(len([c for c in self.calls() if c[:2] == ["pr", "comment"]]), 1, "no second reminder without new activity")

    def test_bot_activity_alone_allows_no_new_reminder(self):
        # pr-status ignores bots, so pr_activity_at stays put and the dedupe holds.
        pr.set_fields(self.path, {"pr_reminded_at": "2026-11-06T09:00:00.000Z"})
        self.stub_gh(pull(requested=[user("alice")]))
        self.run_script(now=at("Wed 2026-11-11 12:00"))
        self.assertEqual(self.calls(), [])

    def test_a_failed_post_is_retried_on_the_next_run(self):
        self.stub_gh(pull(requested=[user("alice")]), post_fails=True)
        self.run_script()
        self.assertNotIn("pr_reminded_at", self.fields())
        self.stub_gh(pull(requested=[user("alice")]))
        self.run_script()
        self.assertIn("pr_reminded_at", self.fields())

    def test_nobody_to_mention_blocks_the_item_once(self):
        self.stub_gh(pull())
        self.run_script()
        f = self.fields()
        self.assertEqual(f["agent_status"], "blocked: PR #1 is ready but has no reviewer to remind")
        self.assertIn("pr_reminded_at", f)
        self.assertFalse(any(c[:2] == ["pr", "comment"] for c in self.calls()))

    def test_dry_run_prints_and_writes_nothing(self):
        self.stub_gh(pull(requested=[user("alice")]))
        before = self.text()
        out = self.run_script("--dry-run")
        self.assertIn("would post on PR #1", out)
        self.assertIn("@alice friendly nudge", out)
        self.assertEqual(self.text(), before)
        self.assertFalse(any(c[:2] == ["pr", "comment"] for c in self.calls()))
        self.assertIn("skip, active", self.run_script("--dry-run", now=at("Thu 2026-11-05 12:00")))


if __name__ == "__main__":
    unittest.main()
