import json, os, stat, subprocess, sys, tempfile, unittest
from unittest import mock

sys.dont_write_bytecode = True

HERE = os.path.dirname(os.path.abspath(__file__))
BIN = os.path.join(HERE, "skills/workitem-pipeline/bin")
sys.path.insert(0, BIN)
import workitem_pr as prs  # noqa: E402

CLI = os.path.join(BIN, "workitem-pr")

# Answers what the fixture maps its argv to (first match of `" ".join(argv)` starting with a key,
# longest key first), logs every call with its stdin, and fails on "fail: <message>".
STUB = """#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
stdin = "" if sys.stdin.isatty() else sys.stdin.read()
name = os.path.basename(sys.argv[0])
with open(os.environ["STUB_DIR"] + "/calls", "a") as f:
    f.write(json.dumps({"bin": name, "args": args, "stdin": stdin}) + "\\n")
answers = json.load(open(os.environ["STUB_DIR"] + "/" + name + ".json"))
line = " ".join(args)
for key in sorted(answers, key=len, reverse=True):
    if line.startswith(key):
        answer = answers[key]
        if isinstance(answer, str) and answer.startswith("fail:"):
            print(answer[5:].strip(), file=sys.stderr)
            sys.exit(1)
        print(answer if isinstance(answer, str) else json.dumps(answer))
        break
"""


class Stubs:
    """Stub gh and bkt in a temp dir, named by WORKITEM_GH and WORKITEM_BKT."""

    def __init__(self, test):
        self.tmp = tempfile.TemporaryDirectory()
        test.addCleanup(self.tmp.cleanup)
        self.dir = self.tmp.name
        for name in ("gh", "bkt"):
            path = os.path.join(self.dir, name)
            with open(path, "w") as f:
                f.write(STUB)
            os.chmod(path, os.stat(path).st_mode | stat.S_IEXEC)
            self.answer(name, {})
        self.env = {"WORKITEM_GH": os.path.join(self.dir, "gh"), "WORKITEM_BKT": os.path.join(self.dir, "bkt"),
                    "STUB_DIR": self.dir}
        patch = mock.patch.dict(os.environ, self.env)
        patch.start()
        test.addCleanup(patch.stop)
        os.environ.pop("WORKITEM_BKT_CONTEXT", None)
        os.environ.pop("GH_TOKEN", None)

    def answer(self, name, answers):
        with open(os.path.join(self.dir, f"{name}.json"), "w") as f:
            json.dump(answers, f)

    def calls(self, name=None):
        path = os.path.join(self.dir, "calls")
        if not os.path.exists(path):
            return []
        with open(path) as f:
            calls = [json.loads(line) for line in f]
        return [c for c in calls if name is None or c["bin"] == name]


GH = prs.parse("https://github.com/o/r/pull/7")
BB = prs.parse("https://bitbucket.org/ws/slug/pull-requests/1")


class ParseTest(unittest.TestCase):
    def test_one_reader_for_every_link(self):
        bb = ("bitbucket.org", "ws", "slug", "42")
        for link in ["https://bitbucket.org/ws/slug/pull-requests/42",
                     "https://bitbucket.org/ws/slug/pull-requests/42/diff",
                     "https://bitbucket.org/ws/slug/pull-requests/42?email=x@y",
                     "https://bitbucket.org/ws/slug/pull-requests/42#comment-123",
                     "https://BITBUCKET.ORG:443/ws/slug/pull-requests/42",
                     "https://user@bitbucket.org/ws/slug/pull-requests/42"]:
            self.assertEqual(prs.parse(link)[:4], bb, link)
        self.assertEqual(prs.parse("https://github.com/o/r/pull/7/files#diff")[:4], ("github.com", "o", "r", "7"))
        self.assertEqual(prs.parse("https://GitHub.com/o/r/pull/7")[:4], ("github.com", "o", "r", "7"))
        self.assertEqual(prs.parse("https://ghe.example.com/o/r/pull/7")[:4], ("ghe.example.com", "o", "r", "7"),
                         "any other host is GitHub (Enterprise)")
        self.assertEqual(prs.parse("https://github.com/acme/pull/pull/12")[:4], ("github.com", "acme", "pull", "12"),
                         "a repo named pull is no marker")
        self.assertEqual(prs.parse("https://bitbucket.org/pull-requests/slug/pull-requests/3")[:4],
                         ("bitbucket.org", "pull-requests", "slug", "3"))

    def test_links_that_name_no_pr(self):
        for link in ["", "not a url", "ftp://github.com/o/r/pull/7", "https://bitbucket.org/ws/slug/branch/nope",
                     "https://bitbucket.org/ws/slug/pull-requests/x", "https://github.com/o/r/pull-requests/42",
                     "https://bitbucket.org/ws/slug/pull/42", "https://github.com/app/pull/7",
                     "https://example.com?@bitbucket.org/ws/slug/pull-requests/1",
                     "https://github.com/o/r/pull/²"]:
            self.assertIsNone(prs.parse(link), link)

    def test_the_host_picks_the_provider(self):
        self.assertIsInstance(prs.provider(GH, None), prs.GitHub)
        self.assertIsInstance(prs.provider(BB, None), prs.Bitbucket)


def gh_pull(requested=(), reviews=(), author="me"):
    return {"author": {"login": author},
            "reviewRequests": {"nodes": [{"requestedReviewer": r} for r in requested]},
            "latestReviews": {"nodes": [{"state": state, "author": a} for a, state in reviews]}}


def user(login):
    return {"__typename": "User", "login": login}


class MentionsTest(unittest.TestCase):
    def test_pending_people_and_reviewers_who_did_not_approve(self):
        mentions = prs.mentions_of(gh_pull(
            requested=[user("alice"), {"__typename": "Team", "combinedSlug": "org/fe"}],
            reviews=[(user("bob"), "COMMENTED"), (user("carol"), "CHANGES_REQUESTED"), (user("dave"), "APPROVED"),
                     ({"__typename": "Bot", "login": "cursor"}, "COMMENTED"), (user("me"), "COMMENTED")]))
        self.assertEqual(mentions, ["@alice", "@bob", "@carol (re-review after your feedback)"])

    def test_a_re_requested_reviewer_who_asked_for_changes_is_named_once(self):
        mentions = prs.mentions_of(gh_pull(requested=[user("carol")], reviews=[(user("carol"), "CHANGES_REQUESTED")]))
        self.assertEqual(mentions, ["@carol (re-review after your feedback)"])

    def test_the_team_only_when_no_person_is_pending(self):
        team = {"__typename": "Team", "combinedSlug": "org/fe"}
        self.assertEqual(prs.mentions_of(gh_pull(requested=[team], reviews=[(user("dave"), "APPROVED")])), ["@org/fe"])
        self.assertEqual(prs.mentions_of(gh_pull()), [])

    def test_bitbucket_mentions_the_reviewers_who_did_not_approve_by_account_id(self):
        stubs = Stubs(self)
        stubs.answer("bkt", {"api /repositories/ws/slug/pullrequests/1": {
            "reviewers": [{"uuid": "{u1}", "account_id": "aid-alice"}, {"uuid": "{u2}", "account_id": "aid-bob"}, {"uuid": "{u3}"}],
            "participants": [{"user": {"uuid": "{u2}"}, "approved": True}]}})
        self.assertEqual(prs.Bitbucket(None).mentions(BB), ["@{aid-alice}", "@{u3}"], "the uuid without braces as fallback")


def bb_pull(**kw):
    return {"id": 1, "state": "OPEN", "merge_state": "open", "draft": False, "description": "The description",
            "author": {"user": {"uuid": "{me}", "nickname": "me"}}, "source": {"commit": {"hash": "h1"}, "branch": {"name": "feature"}},
            "destination": {"branch": {"name": "main"}}, "created_on": "2026-01-01", "updated_on": "2026-01-05",
            "participants": [], **kw}


class BitbucketMappingTest(unittest.TestCase):
    def test_states_approvals_and_build_statuses_in_github_words(self):
        value = bb_pull(state="DECLINED", merge_state="conflicted", participants=[
            {"user": {"uuid": "{me}", "nickname": "me"}, "approved": True, "participated_on": "2026-01-02"},
            {"user": {"uuid": "{b}", "nickname": "bob"}, "approved": True, "participated_on": None},
            {"user": {"uuid": "{c}", "nickname": "carol"}, "approved": False, "participated_on": "2026-01-03"}])
        checks = prs.bb_checks([{"name": "lint", "state": "FAILED"}, {"key": "build", "state": "INPROGRESS"}, {"name": "odd", "state": "NEW"}])
        node = prs.bb_node(value, "2026-01-04", checks)
        self.assertEqual((node["state"], node["mergeable"], node["reviewDecision"]), ("CLOSED", "CONFLICTING", "APPROVED"))
        self.assertEqual([(r["author"]["login"], r["submittedAt"]) for r in node["reviews"]["nodes"]], [("bob", "")],
                         "the author's own approval is no review; a null participated_on is no time")
        self.assertEqual([(c["name"], c["status"], c["conclusion"], c["isRequired"]) for c in checks],
                         [("lint", "COMPLETED", "FAILURE", True), ("build", "IN_PROGRESS", "IN_PROGRESS", True),
                          ("odd", "COMPLETED", "FAILURE", True)], "an unknown build state is red, never green")
        self.assertIsNone(node["comments"], "the pull read carries no comments")
        self.assertEqual(prs.bb_node(bb_pull(merge_state=None), None, [])["mergeable"], "UNKNOWN")

    def test_pagination_follows_the_next_link_and_deleted_comments_are_gone(self):
        stubs = Stubs(self)
        stubs.answer("bkt", {
            "api /repositories/ws/slug/pullrequests/1/comments": {
                "values": [{"id": 10, "user": {"nickname": "bob"}, "content": {"raw": "rename"}, "created_on": "2026-01-03"},
                           {"id": 11, "user": {"nickname": "bob"}, "content": {"raw": "gone"}, "deleted": True}],
                "next": "https://api.bitbucket.org/2.0/repositories/ws/slug/pullrequests/1/comments?page=2&pagelen=100"},
            "api /2.0/repositories/ws/slug/pullrequests/1/comments": {
                "values": [{"id": 12, "user": {"nickname": "me"}, "content": {"raw": "will do"}, "created_on": "2026-01-04",
                            "inline": {"path": "a.rs", "to": 3}, "parent": {"id": 10}}]}})
        threads, comments = prs.Bitbucket(None).read_threads([BB.url])
        self.assertEqual([c["id"] for c in comments[BB.url]], ["10"])
        inline = threads[BB.url]["reviewThreads"]["nodes"][0]["comments"]["nodes"][0]
        self.assertEqual((inline["databaseId"], inline["replyTo"]), (12, {"databaseId": 10}))
        second = stubs.calls("bkt")[1]["args"]
        self.assertEqual(second, ["api", "/2.0/repositories/ws/slug/pullrequests/1/comments", "--json",
                                  "--param", "page=2", "--param", "pagelen=100"])

    def test_the_conversation_of_a_bitbucket_pr(self):
        stubs = Stubs(self)
        stubs.answer("bkt", {
            "api /repositories/ws/slug/pullrequests/1/comments": {"values": [
                {"id": 10, "user": {"display_name": "You"}, "content": {"raw": "A comment"}, "created_on": "2026-01-03"},
                {"id": 11, "user": {"display_name": "You"}, "content": {"raw": "inline"}, "created_on": "2026-01-01T12",
                 "inline": {"path": "a.rs"}},
                {"id": 12, "content": {"raw": "deleted"}, "created_on": "2026-01-09", "deleted": True}]},
            "api /repositories/ws/slug/pullrequests/1": bb_pull(participants=[
                {"user": {"uuid": "{b}", "display_name": "Me"}, "approved": True, "participated_on": "2026-01-02"},
                {"user": {"uuid": "{c}", "display_name": "Null"}, "approved": True, "participated_on": None}])})
        found = prs.Bitbucket(None).conversation(BB)
        self.assertEqual(found["body"], "The description")
        self.assertEqual([(n["kind"], n["id"], n["author"], n["at"], n["path"]) for n in found["notes"]], [
            ("review", "approval-{c}", "Null", "2026-01-05", None),
            ("comment", "10", "You", "2026-01-03", None),
            ("review", "approval-{b}", "Me", "2026-01-02", None),
            ("inline", "inline-11", "You", "2026-01-01T12", "a.rs")],
            "a null participated_on shows at the PR's update time; deleted comments are gone")


class CallsTest(unittest.TestCase):
    """The exact gh and bkt calls of the actions."""

    def setUp(self):
        self.stubs = Stubs(self)

    def args(self, name):
        return [c["args"] for c in self.stubs.calls(name)]

    def test_github_actions(self):
        gh = prs.GitHub(None)
        gh.merge(GH)
        gh.rebase(GH, "/nowhere")
        gh.ready(GH)
        gh.add_reviewers(GH, ["org/team"])
        gh.comment(GH, "hello")
        gh.reply(GH, "inline-5", "fixed")
        gh.reply(GH, "review-PRR_1", "noted")
        self.assertEqual(self.args("gh"), [
            ["pr", "merge", GH.url, "--squash"],
            ["pr", "update-branch", GH.url, "--rebase"],
            ["pr", "ready", GH.url],
            ["pr", "edit", GH.url, "--add-reviewer", "org/team"],
            ["pr", "comment", GH.url, "--body-file", "-"],
            ["api", "repos/o/r/pulls/7/comments/5/replies", "--hostname", "github.com", "-F", "body=@-"],
            ["pr", "comment", GH.url, "--body-file", "-"]])
        self.assertEqual([c["stdin"] for c in self.stubs.calls("gh")][4:], ["hello", "fixed", "noted"])

    def test_bitbucket_actions_carry_the_context(self):
        os.environ["WORKITEM_BKT_CONTEXT"] = "work"
        bb = prs.Bitbucket(None)
        bb.merge(BB)
        bb.ready(BB)
        bb.add_reviewers(BB, ["alice", "bob"])
        bb.comment(BB, "hello")
        bb.reply(BB, "inline-5", "fixed")
        bb.reply(BB, "6", "noted")
        where = ["1", "--workspace", "ws", "--repo", "slug"]
        self.assertEqual(self.args("bkt"), [
            ["pr", "merge", *where, "--strategy", "squash", "--context", "work"],
            ["pr", "publish", *where, "--context", "work"],
            ["pr", "edit", *where, "--reviewer", "alice", "--reviewer", "bob", "--context", "work"],
            ["pr", "comment", *where, "--text", "hello", "--context", "work"],
            ["pr", "comment", *where, "--text", "fixed", "--parent", "5", "--context", "work"],
            ["pr", "comment", *where, "--text", "noted", "--parent", "6", "--context", "work"]])

    def test_the_context_comes_from_the_workspace_without_the_env(self):
        with tempfile.TemporaryDirectory() as ws:
            with open(os.path.join(ws, "workspace.toml"), "w") as f:
                f.write('bkt_context = "team"\n')
            prs.Bitbucket(ws).merge(BB)
            prs.Bitbucket(None).merge(BB)
        self.assertEqual([a[-2:] for a in self.args("bkt")], [["--context", "team"], ["--strategy", "squash"]])

    def test_a_failure_carries_a_message_even_without_stderr(self):
        self.stubs.answer("gh", {"pr merge": "fail:"})
        with self.assertRaises(RuntimeError) as e:
            prs.GitHub(None).merge(GH)
        self.assertEqual(str(e.exception), "gh pr exited with 1")
        self.stubs.answer("bkt", {"pr merge": "fail: HTTP 409 conflict"})
        with self.assertRaises(RuntimeError) as e:
            prs.Bitbucket(None).merge(BB)
        self.assertEqual(str(e.exception), "HTTP 409 conflict")

    def test_failed_logs_of_red_workflow_runs(self):
        self.stubs.answer("gh", {
            "pr checks": [{"name": "rspec", "bucket": "fail", "link": "https://github.com/o/r/actions/runs/11/job/1"},
                          {"name": "lint", "bucket": "fail", "link": "https://github.com/o/r/actions/runs/11/job/2"},
                          {"name": "ci/ext", "bucket": "fail", "link": "https://ci.example.com/9"},
                          {"name": "ok", "bucket": "pass", "link": "https://github.com/o/r/actions/runs/12/job/3"}],
            "run view 11": "rspec failed here"})
        log = prs.GitHub(None).failed_log(GH)
        self.assertIn("ci/ext", log)
        self.assertIn("https://ci.example.com/9", log)
        self.assertIn("rspec failed here", log)
        self.assertEqual([a for a in self.args("gh") if a[0] == "run"], [["run", "view", "11", "-R", "github.com/o/r", "--log-failed"]],
                         "each red run once")

    def test_checks_fail_on_a_gh_error_and_are_empty_without_checks(self):
        self.stubs.answer("gh", {"pr checks": "fail: HTTP 401: Bad credentials"})
        with self.assertRaises(RuntimeError) as e:
            prs.GitHub(None).failed_log(GH)
        self.assertEqual(str(e.exception), "HTTP 401: Bad credentials", "exit 1 without JSON is an error, not green")
        self.stubs.answer("gh", {"pr checks": "fail: no checks reported on the 'feature' branch"})
        self.assertEqual(prs.GitHub(None).checks(GH), [])

    def test_an_enterprise_pr_is_read_on_its_own_host(self):
        ghe = prs.parse("https://ghe.example.com/o/r/pull/7")
        self.stubs.answer("gh", {"api graphql": {"data": {"viewer": {"login": "me"}, "repository": {"p7": {"number": 7, "state": "MERGED", "mergeable": "UNKNOWN"}}}},
                                 "pr view": {"body": "", "comments": [], "reviews": []},
                                 "api repos/o/r/pulls/7/comments": [[]]})
        nodes, me = prs.read_pulls([GH.url, ghe.url], None)
        prs.GitHub(None).conversation(ghe)
        hosts = [a[a.index("--hostname") + 1] for a in self.args("gh") if a[0] == "api"]
        self.assertEqual(sorted(hosts[:2]), ["ghe.example.com", "github.com"], "one query per host")
        self.assertEqual(hosts[2], "ghe.example.com")
        self.assertEqual(me, {GH.url: "me", ghe.url: "me"})

    def test_a_hanging_token_read_is_a_problem_not_a_crash(self):
        with tempfile.TemporaryDirectory() as ws:
            with open(os.path.join(ws, "workspace.toml"), "w") as f:
                f.write('github_account = "me"\n')
            with mock.patch.object(prs.tracker.subprocess, "run", side_effect=subprocess.TimeoutExpired("gh", 30)):
                with self.assertRaises(prs.ERRORS):
                    prs.GitHub(ws).merge(GH)


def git(cwd, *args):
    return subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", *args], cwd=cwd, check=True,
                          capture_output=True, text=True).stdout


class BitbucketRebaseTest(unittest.TestCase):
    """The rebase against a local bare remote: a temporary worktree, never the repo's checkout."""

    def setUp(self):
        self.stubs = Stubs(self)
        self.stubs.answer("bkt", {"api /repositories/ws/slug/pullrequests/1": bb_pull()})
        root = self.stubs.dir
        self.repo, remote, self.feature = os.path.join(root, "repo"), os.path.join(root, "remote.git"), os.path.join(root, "feature")
        os.makedirs(self.repo)
        git(self.repo, "init", "-q", "-b", "main")
        git(self.repo, "commit", "-q", "--allow-empty", "-m", "base")
        git(root, "init", "-q", "--bare", remote)
        git(self.repo, "remote", "add", "origin", remote)
        git(self.repo, "push", "-q", "-u", "origin", "main")
        git(self.repo, "worktree", "add", "-q", "-b", "feature", self.feature)
        with open(os.path.join(self.feature, "file"), "w") as f:
            f.write("feature\n")
        git(self.feature, "add", "file")
        git(self.feature, "commit", "-q", "-m", "feature")
        git(self.feature, "push", "-q", "-u", "origin", "feature")

    def main_commit(self, content=None):
        if content:
            with open(os.path.join(self.repo, "file"), "w") as f:
                f.write(content)
            git(self.repo, "add", "file")
        git(self.repo, "commit", "-q", "--allow-empty", "-m", "later base")
        git(self.repo, "push", "-q", "origin", "main")

    def worktrees(self):
        return git(self.repo, "worktree", "list")

    def test_rebases_and_pushes_with_lease(self):
        self.main_commit()
        prs.Bitbucket(None).rebase(BB, self.repo)
        git(self.repo, "merge-base", "--is-ancestor", "origin/main", "origin/feature")
        self.assertEqual(git(self.repo, "symbolic-ref", "HEAD").strip(), "refs/heads/main", "the checkout kept its branch")
        self.assertNotIn("wi-rebase", self.worktrees())

    def test_a_conflict_aborts_and_leaves_nothing_behind(self):
        self.main_commit("main\n")
        before = git(self.repo, "rev-parse", "origin/feature")
        with self.assertRaises(RuntimeError):
            prs.Bitbucket(None).rebase(BB, self.repo)
        self.assertEqual(git(self.repo, "rev-parse", "origin/feature"), before, "the remote branch is untouched")
        self.assertNotIn("wi-rebase", self.worktrees())
        with open(os.path.join(self.repo, "file")) as f:
            self.assertEqual(f.read(), "main\n", "the checkout was not touched")


class CliTest(unittest.TestCase):
    def setUp(self):
        self.stubs = Stubs(self)
        self.ws = os.path.join(self.stubs.dir, "ws")
        os.makedirs(os.path.join(self.ws, "boards"))
        with open(os.path.join(self.ws, "boards", "dev.json"), "w") as f:
            json.dump({"repo": self.stubs.dir}, f)

    def item(self, pr):
        folder = os.path.join(self.ws, "item")
        os.makedirs(folder, exist_ok=True)
        with open(os.path.join(folder, "workitem.md"), "w") as f:
            f.write(f"---\ntitle: t\nboard: dev\npr: {pr}\n---\n")
        return folder

    def cli(self, *args):
        return subprocess.run([sys.executable, CLI, *args], capture_output=True, text=True, env={**os.environ})

    def test_errors_exit_2_or_1_with_a_message(self):
        result = self.cli("merge", self.item("none"))
        self.assertEqual((result.returncode, result.stderr.strip()), (2, "workitem-pr: item has no PR link"))
        result = self.cli("merge", self.item("https://bitbucket.org/ws/slug/branch/x"))
        self.assertEqual(result.returncode, 2)
        self.assertIn("is not a pull request link", result.stderr)
        self.stubs.answer("gh", {"pr merge": "fail:"})
        result = self.cli("merge", self.item(GH.url))
        self.assertEqual((result.returncode, result.stderr.strip()), (1, "workitem-pr: gh pr exited with 1"))

    def test_conversation_as_json_and_as_text(self):
        self.stubs.answer("gh", {
            "pr view": {"body": "Why", "comments": [{"id": "IC_1", "author": {"login": "bob"}, "body": "hi", "createdAt": "2026-02-03"}],
                        "reviews": [{"id": "PRR_1", "state": "COMMENTED", "author": {"login": "carol"}, "body": "", "submittedAt": "2026-02-02"},
                                    {"id": "PRR_2", "state": "APPROVED", "author": {"login": "dave"}, "body": "", "submittedAt": "2026-02-05"}]},
            "api repos/o/r/pulls/7/comments": [[{"id": 5, "user": {"login": "alice"}, "body": "rename", "created_at": "2026-02-04", "path": "a.rs"}]]})
        result = self.cli("conversation", self.item(GH.url), "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        found = json.loads(result.stdout)
        self.assertEqual([(n["kind"], n["id"], n["author"]) for n in found["notes"]],
                         [("review", "review-PRR_2", "dave"), ("inline", "inline-5", "alice"), ("comment", "IC_1", "bob")],
                         "an empty COMMENTED review is noise")
        self.assertEqual(found["notes"][1]["path"], "a.rs")
        text = self.cli("conversation", self.item(GH.url)).stdout
        self.assertIn("## inline by alice\n\nPosted 2026-02-04 · id inline-5 · a.rs\n\nrename", text)
        self.assertIn("## review by dave (approved)", text)

    def test_reply_reads_the_body_file(self):
        body = os.path.join(self.stubs.dir, "body.md")
        with open(body, "w") as f:
            f.write("Fixed in abc")
        result = self.cli("reply", self.item(BB.url), "inline-5", "--body-file", body)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.stubs.calls("bkt")[-1]["args"][-4:], ["--text", "Fixed in abc", "--parent", "5"])


if __name__ == "__main__":
    unittest.main()
