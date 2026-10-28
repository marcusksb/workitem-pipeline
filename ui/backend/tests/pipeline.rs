//! Dispatcher and actions against real tmux, real `workitem-move`, and a stub `claude`.
use std::{
    fs,
    future::IntoFuture,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{body::Body, http::{header, Request, StatusCode}};
use http_body_util::BodyExt;
use tower::ServiceExt;
use workitem_ui::{config::Store, dispatch, router, AppState, Shared};

mod common;
use common::{eventually, Tmux};

/// Behaviour per stage comes from `<repo>/.stub-<stage>`: `advance`, `block`, `done`, `crash`, or idle.
const STUB: &str = r#"#!/bin/bash
repo="$WORKITEM_REPO"
file="$repo/.claude/workitems/$WORKITEM/workitem.md"
{ echo "argc=$#"; printf 'last=%s\n' "${@: -1}"; } > "$LOGDIR/args-$(date +%s%N)"
if [ -n "$WORKITEM" ]; then
  stage=$(perl -ne 'print $1 if /^stage:\s*(\S+)/' "$file")
  echo "$WORKITEM $stage" >> "$repo/.stub-log"
  mode=$(cat "$repo/.stub-$stage" 2>/dev/null || echo idle)
  case "$mode" in
    advance) python3 "$SKILLS/workitem-pipeline/bin/workitem-move" "$file" analyze ;;
    block) perl -pi -e 's/^agent_status:.*/agent_status: blocked: need input/' "$file" ;;
    done) perl -pi -e 's/^agent_status:.*/agent_status: ok/' "$file" ;;
    crash) exit 0 ;;
  esac
fi
sleep 60
"#;

struct World {
    dir: tempfile::TempDir,
    repo: PathBuf,
    tmux: Tmux,
    state: Shared,
    token: String,
}

fn skills() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills").canonicalize().unwrap()
}

fn write_item(repo: &Path, dir: &str, stage: &str, extra: &str) {
    let item = repo.join(".claude/workitems").join(dir);
    fs::create_dir_all(&item).unwrap();
    fs::write(
        item.join("workitem.md"),
        format!("---\ntitle: {dir}\ntype: bug\nstage: {stage}\napproved: false\nagent_status: ok\nissue: T-1\n{extra}---\n\n# {dir}\n"),
    )
    .unwrap();
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo").canonicalize_or_create();
    let logs = dir.path().join("logs");
    fs::create_dir_all(&logs).unwrap();
    let stub = dir.path().join("claude");
    fs::write(&stub, STUB.replace("$SKILLS", &skills().display().to_string()).replace("$LOGDIR", &logs.display().to_string())).unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();

    let gh = dir.path().join("gh");
    // `gh api user` names the reader, `gh api <path>` lists inline comments, anything else is `gh pr view`.
    let d = dir.path().display();
    let script = format!(
        "#!/bin/bash\nif [ \"$1\" = api ]; then\n  if [ \"$2\" = user ]; then echo '{{\"login\":\"me\"}}'; else cat {d}/inline.json 2>/dev/null || echo '[]'; fi\nelif [ -f {d}/pr-once.json ]; then cat {d}/pr-once.json; rm {d}/pr-once.json\nelse cat {d}/pr.json; fi\n"
    );
    fs::write(&gh, script).unwrap();
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();

    let store = Store::open(dir.path().join("config.toml")).unwrap();
    store.add_project("app", repo.to_str().unwrap()).unwrap();
    let token = store.token();
    let tmux = Tmux::new();
    // A real listener: watcher scripts call the backend over HTTP (`workitem-run-item`).
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut state = AppState::new(store, port);
    state.tmux_socket = Some(tmux.0.clone());
    state.skills_dir = skills();
    state.claude = stub.display().to_string();
    state.gh = gh.display().to_string();
    let state = Arc::new(state);
    tokio::spawn(axum::serve(tokio::net::TcpListener::from_std(listener).unwrap(), router(state.clone())).into_future());
    World { dir, repo, tmux, state, token }
}

trait CanonicalizeOrCreate {
    fn canonicalize_or_create(self) -> PathBuf;
}
impl CanonicalizeOrCreate for PathBuf {
    fn canonicalize_or_create(self) -> PathBuf {
        fs::create_dir_all(&self).unwrap();
        self.canonicalize().unwrap()
    }
}

/// One dispatcher pass, with its watchers forgotten (so each pass runs them, as a tick would after
/// their interval) and awaited (they run in tasks of their own).
async fn settle(w: &World) {
    let _ = fs::remove_file(w.state.store.sibling("watch.json"));
    dispatch::pass(&w.state).await;
    for _ in 0..3000 {
        if w.state.watching.lock().unwrap().is_empty() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("a watcher is still running");
}

fn set_pr(w: &World, state: &str, decision: &str, mergeable: &str, conclusion: &str) {
    let json = serde_json::json!({
        "state": state, "reviewDecision": decision, "mergeable": mergeable,
        "statusCheckRollup": [{ "name": "rspec", "status": "COMPLETED", "conclusion": conclusion }],
    });
    fs::write(w.dir.path().join("pr.json"), json.to_string()).unwrap();
}

fn field(repo: &Path, dir: &str, key: &str) -> String {
    let text = fs::read_to_string(repo.join(".claude/workitems").join(dir).join("workitem.md")).unwrap();
    text.lines().find_map(|l| l.strip_prefix(&format!("{key}:"))).unwrap_or("").trim().to_string()
}

fn stub_log(repo: &Path) -> Vec<String> {
    fs::read_to_string(repo.join(".stub-log")).unwrap_or_default().lines().map(String::from).collect()
}

async fn post(w: &World, uri: &str, json: &str) -> (StatusCode, String) {
    let request = Request::post(uri)
        .header(header::HOST, format!("127.0.0.1:{}", w.state.port))
        .header(header::COOKIE, format!("wi_session={}", w.token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json.to_string()))
        .unwrap();
    let response = router(w.state.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    (status, String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes()).into_owned())
}

async fn get(w: &World, uri: &str) -> String {
    let request = Request::get(uri)
        .header(header::HOST, format!("127.0.0.1:{}", w.state.port))
        .header(header::COOKIE, format!("wi_session={}", w.token))
        .body(Body::empty())
        .unwrap();
    let response = router(w.state.clone()).oneshot(request).await.unwrap();
    String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes()).into_owned()
}

#[tokio::test]
async fn dispatcher_runs_handlers_reaps_finished_sessions_and_leaves_blocked_alone() {
    let w = world();
    write_item(&w.repo, "T-1-flow", "backlog", "");
    fs::write(w.repo.join(".stub-backlog"), "advance").unwrap();
    fs::write(w.repo.join(".stub-analyze"), "block").unwrap();

    settle(&w).await;
    assert!(w.tmux.has("wi-T-1-flow"), "backlog item gets a handler session");
    eventually("triage moved the item to analyze", || field(&w.repo, "T-1-flow", "stage") == "analyze").await;

    // Status is `ok` after the move, so the finished session is reaped and analyze starts.
    settle(&w).await;
    eventually("analyze handler blocked the item", || field(&w.repo, "T-1-flow", "agent_status").starts_with("blocked")).await;

    settle(&w).await;
    settle(&w).await;
    assert!(w.tmux.has("wi-T-1-flow"), "a blocked session waits for the human");
    assert_eq!(stub_log(&w.repo), vec!["T-1-flow backlog", "T-1-flow analyze"]);
}

#[tokio::test]
async fn dispatcher_retries_crashed_handlers_then_blocks_at_the_attempt_cap() {
    let w = world();
    write_item(&w.repo, "T-2-crash", "analyze", "");
    fs::write(w.repo.join(".stub-analyze"), "crash").unwrap();

    for _ in 0..10 {
        settle(&w).await;
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        if field(&w.repo, "T-2-crash", "agent_status").starts_with("blocked") {
            break;
        }
    }
    assert_eq!(field(&w.repo, "T-2-crash", "agent_status"), "blocked: max attempts reached in stage analyze");
    assert_eq!(stub_log(&w.repo).len(), 3);
    settle(&w).await;
    assert_eq!(stub_log(&w.repo).len(), 3, "no more attempts once blocked");
}

#[tokio::test]
async fn dispatcher_starts_development_with_the_start_prompt() {
    let w = world();
    write_item(&w.repo, "T-10-dev", "in_development", "");

    settle(&w).await;
    eventually("the start handler logged its invocation", || {
        stub_log(&w.repo) == vec!["T-10-dev in_development"]
    })
    .await;
    let args = fs::read_dir(w.dir.path().join("logs")).unwrap().map(|e| fs::read_to_string(e.unwrap().path()).unwrap()).collect::<String>();
    assert!(args.contains("Current stage: in_development"), "{args}");
    assert!(args.contains("resume it as is"), "{args}");
    assert!(!args.contains("Do not change code"), "{args}");
}

#[tokio::test]
async fn dispatcher_starts_development_once_a_ready_item_is_approved() {
    let w = world();
    write_item(&w.repo, "T-11-ready", "ready_for_development", "");

    settle(&w).await;
    assert!(stub_log(&w.repo).is_empty(), "an unapproved item waits for the human");

    let (status, _) = post(&w, "/api/workitems/app/T-11-ready/approve", "").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    settle(&w).await;
    assert_eq!(field(&w.repo, "T-11-ready", "stage"), "in_development", "approving moves the item on");
    eventually("the start handler picked up the approved item", || {
        stub_log(&w.repo) == vec!["T-11-ready in_development"]
    })
    .await;
}

#[tokio::test]
async fn dispatcher_ignores_manual_stages_and_a_same_named_session_from_another_repo() {
    let w = world();
    write_item(&w.repo, "T-3-manual", "ready_for_development", "");
    write_item(&w.repo, "T-4-other", "backlog", "");
    let elsewhere = w.dir.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    w.tmux.run(&["new-session", "-d", "-s", "wi-T-4-other", "-c", elsewhere.to_str().unwrap(), "sleep 60"]);

    settle(&w).await;
    // The other repo's session is not this item's session, and the name is taken, so no launch.
    assert!(stub_log(&w.repo).is_empty());
    assert_eq!(field(&w.repo, "T-3-manual", "agent_status"), "ok");
    assert_eq!(field(&w.repo, "T-4-other", "agent_status"), "ok", "failed launch is rolled back");
}

#[tokio::test]
async fn approve_and_reject_edit_the_workitem_through_the_pipeline_rules() {
    let w = world();
    write_item(&w.repo, "T-5-gate", "backlog", "");
    let (status, _) = post(&w, "/api/workitems/app/T-5-gate/approve", "").await;
    assert_eq!(status, StatusCode::CONFLICT, "only ready_for_development can be approved");

    write_item(&w.repo, "T-6-gate", "ready_for_development", "");
    let (status, _) = post(&w, "/api/workitems/app/T-6-gate/approve", "").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(field(&w.repo, "T-6-gate", "approved"), "true");
    assert_eq!(field(&w.repo, "T-6-gate", "approved_at").len(), 10);

    write_item(&w.repo, "T-7-gate", "ready_for_development", "");
    let (status, _) = post(&w, "/api/workitems/app/T-7-gate/reject", r#"{"comment":"  "}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = post(&w, "/api/workitems/app/T-7-gate/reject", r#"{"comment":"needs a rollback plan"}"#).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(field(&w.repo, "T-7-gate", "stage"), "analyze");
    assert_eq!(field(&w.repo, "T-7-gate", "approved"), "false");
    let text = fs::read_to_string(w.repo.join(".claude/workitems/T-7-gate/workitem.md")).unwrap();
    assert!(text.contains("## Rejection\n\nneeds a rollback plan"));
}

#[tokio::test]
async fn edit_brief_replaces_the_body_only_while_it_is_unchanged() {
    let w = world();
    write_item(&w.repo, "T-30-edit", "analyze", "");
    let file = w.repo.join(".claude/workitems/T-30-edit/workitem.md");
    let (status, _) = post(&w, "/api/workitems/app/T-30-edit/brief", r#"{"base":"\n# stale\n","body":"x"}"#).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) =
        post(&w, "/api/workitems/app/T-30-edit/brief", r#"{"base":"\n# T-30-edit\n","body":"\n# Edited\n\nstage: done\n"}"#).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.ends_with("---\n\n# Edited\n\nstage: done\n"));
    assert_eq!(field(&w.repo, "T-30-edit", "stage"), "analyze", "the body cannot change the frontmatter");
}

#[tokio::test]
async fn edit_frontmatter_sets_existing_fields_but_never_stage() {
    let w = world();
    write_item(&w.repo, "T-31-info", "analyze", "");
    let url = "/api/workitems/app/T-31-info/frontmatter";
    let (status, _) = post(&w, url, r#"{"fields":{"stage":"done"}}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = post(&w, url, r#"{"fields":{"nope":"x"}}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = post(&w, url, r#"{"fields":{"title":"a\nstage: done"}}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = post(&w, url, r#"{"fields":{"title":"New title"}}"#).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(field(&w.repo, "T-31-info", "title"), "New title");
    assert_eq!(field(&w.repo, "T-31-info", "stage"), "analyze");
}

#[tokio::test]
async fn clear_blocked_sets_working_when_an_agent_is_still_waiting() {
    let w = world();
    write_item(&w.repo, "T-8-idle", "analyze", "");
    let (status, _) = post(&w, "/api/workitems/app/T-8-idle/clear-blocked", "").await;
    assert_eq!(status, StatusCode::CONFLICT, "not blocked");

    let blocked = fs::read_to_string(w.repo.join(".claude/workitems/T-8-idle/workitem.md")).unwrap().replace("agent_status: ok", "agent_status: blocked: x");
    fs::write(w.repo.join(".claude/workitems/T-8-idle/workitem.md"), &blocked).unwrap();
    let (status, _) = post(&w, "/api/workitems/app/T-8-idle/clear-blocked", "").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(field(&w.repo, "T-8-idle", "agent_status"), "ok");

    fs::write(w.repo.join(".claude/workitems/T-8-idle/workitem.md"), &blocked).unwrap();
    w.tmux.run(&["new-session", "-d", "-s", "wi-T-8-idle", "-c", w.repo.to_str().unwrap(), "sleep 60"]);
    post(&w, "/api/workitems/app/T-8-idle/clear-blocked", "").await;
    assert_eq!(field(&w.repo, "T-8-idle", "agent_status"), "working");

    let (status, _) = post(&w, "/api/workitems/app/T-8-idle/kill-session", "").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(!w.tmux.has("wi-T-8-idle"));
    assert_eq!(field(&w.repo, "T-8-idle", "agent_status"), "ok");
}

#[tokio::test]
async fn agent_sessions_start_from_the_ui_and_pass_text_as_one_argument() {
    let w = world();
    write_item(&w.repo, "T-9-start", "ready_for_development", "");

    let (status, _) = post(&w, "/api/workitems/app/T-9-start/start", "").await;
    assert_eq!(status, StatusCode::CONFLICT, "not approved");
    post(&w, "/api/workitems/app/T-9-start/approve", "").await;
    let (status, _) = post(&w, "/api/workitems/app/T-9-start/start", "").await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(w.tmux.has("wi-T-9-start"));
    assert_eq!(field(&w.repo, "T-9-start", "agent_status"), "working");
    let (status, body) = post(&w, "/api/workitems/app/T-9-start/start", "").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body.contains("already exists"));

    let pwn = w.dir.path().join("pwned");
    let input = format!("it's \"quoted\" $(touch {}) `touch {}`", pwn.display(), pwn.display());
    let (status, body) = post(&w, "/api/projects/app/create", &serde_json::json!({ "input": input }).to_string()).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let session = serde_json::from_str::<serde_json::Value>(&body).unwrap()["session"].as_str().unwrap().to_string();
    assert!(session.starts_with("wi-new-"));

    let sessions = get(&w, "/api/sessions").await;
    assert!(sessions.contains(&session), "{sessions}");

    let logs = w.dir.path().join("logs");
    eventually("stub recorded the create prompt", || {
        fs::read_dir(&logs).unwrap().filter_map(Result::ok).any(|e| fs::read_to_string(e.path()).is_ok_and(|t| t.contains("Create a workitem with the arguments")))
    })
    .await;
    let create_log = fs::read_dir(&logs).unwrap().filter_map(Result::ok).map(|e| fs::read_to_string(e.path()).unwrap()).find(|t| t.contains("Create a workitem with the arguments")).unwrap();
    assert!(create_log.starts_with("argc=3\n"), "settings flag, path and prompt: {create_log}");
    assert!(create_log.contains(&input), "prompt keeps the text verbatim");
    assert!(!pwn.exists(), "text must never reach a shell");
}

#[tokio::test]
async fn a_manual_session_starts_in_any_stage_and_survives_the_dispatcher() {
    let w = world();
    write_item(&w.repo, "T-12-chat", "backlog", "");
    let (status, _) = post(&w, "/api/workitems/app/T-12-chat/session", "").await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(w.tmux.has("wi-T-12-chat"));
    assert_eq!(field(&w.repo, "T-12-chat", "agent_status"), "working");
    settle(&w).await;
    assert!(w.tmux.has("wi-T-12-chat"), "a working session is not reaped");

    let (status, body) = post(&w, "/api/workitems/app/T-12-chat/session", "").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body.contains("already exists"));
    assert_eq!(field(&w.repo, "T-12-chat", "agent_status"), "working", "a refused launch leaves the status alone");

    write_item(&w.repo, "T-13-stuck", "analyze", "");
    let stuck = fs::read_to_string(w.repo.join(".claude/workitems/T-13-stuck/workitem.md")).unwrap().replace("agent_status: ok", "agent_status: blocked: x");
    fs::write(w.repo.join(".claude/workitems/T-13-stuck/workitem.md"), stuck).unwrap();
    let (status, _) = post(&w, "/api/workitems/app/T-13-stuck/session", "").await;
    assert_eq!(status, StatusCode::CONFLICT, "clear blocked first");
}

#[tokio::test]
async fn discuss_brief_opens_a_session_on_the_brief() {
    let w = world();
    write_item(&w.repo, "T-14-talk", "ready_for_development", "");
    let (status, _) = post(&w, "/api/workitems/app/T-14-talk/discuss", "").await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(w.tmux.has("wi-T-14-talk"));
    assert_eq!(field(&w.repo, "T-14-talk", "agent_status"), "working");

    let logs = w.dir.path().join("logs");
    eventually("stub recorded the brief prompt", || {
        fs::read_dir(&logs).unwrap().filter_map(Result::ok).any(|e| fs::read_to_string(e.path()).is_ok_and(|t| t.contains("discussing the brief")))
    })
    .await;

    let (status, body) = post(&w, "/api/workitems/app/T-14-talk/discuss", "").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body.contains("already exists"));
}

#[tokio::test]
async fn a_session_starts_in_the_worktree_and_still_belongs_to_the_project() {
    let w = world();
    write_item(&w.repo, "T-15-wt", "analyze", "");
    let git = |args: &[&str], cwd: &Path| {
        let ok = std::process::Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).current_dir(cwd).output().unwrap().status.success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q"], &w.repo);
    git(&["commit", "-q", "--allow-empty", "-m", "i"], &w.repo);
    let worktree = w.repo.parent().unwrap().join("repo-T-15-wt");
    git(&["worktree", "add", "-q", "-b", "wt", worktree.to_str().unwrap()], &w.repo);

    let (status, _) = post(&w, "/api/workitems/app/T-15-wt/session", "").await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let out = std::process::Command::new("tmux").args(["-L", &w.tmux.0, "list-sessions", "-F", "#{session_path}"]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), worktree.to_str().unwrap());

    let logs = w.dir.path().join("logs");
    eventually("stub ran with the repo added", || {
        fs::read_dir(&logs).unwrap().filter_map(Result::ok).any(|e| fs::read_to_string(e.path()).is_ok_and(|t| t.starts_with("argc=5\n")))
    })
    .await;

    let items: serde_json::Value = serde_json::from_str(&get(&w, "/api/workitems").await).unwrap();
    assert_eq!(items[0]["session"], true, "the session is the item's although it runs in the worktree");
    settle(&w).await;
    assert_eq!(field(&w.repo, "T-15-wt", "agent_status"), "working", "not taken for a working item without a session");
    assert_eq!(w.tmux.session_names(), vec!["wi-T-15-wt"]);
}

#[tokio::test]
async fn items_report_unpushed_commits_of_their_worktree() {
    let w = world();
    write_item(&w.repo, "T-12-wt", "in_development", "");
    write_item(&w.repo, "T-13-none", "in_development", "");
    let git = |args: &[&str], cwd: &Path| {
        let ok = std::process::Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).current_dir(cwd).output().unwrap().status.success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q"], &w.repo);
    git(&["commit", "-q", "--allow-empty", "-m", "i"], &w.repo);
    let worktree = w.repo.parent().unwrap().join("repo-T-12-wt");
    git(&["worktree", "add", "-q", "-b", "wt", worktree.to_str().unwrap()], &w.repo);
    git(&["commit", "-q", "--allow-empty", "-m", "a"], &worktree);

    async fn unpushed(w: &World, dir: &str) -> serde_json::Value {
        let items: serde_json::Value = serde_json::from_str(&get(w, "/api/workitems").await).unwrap();
        items.as_array().unwrap().iter().find(|i| i["dir"] == dir).unwrap()["unpushed"].clone()
    }
    assert_eq!(unpushed(&w, "T-12-wt").await, 2, "never pushed: every commit counts");
    assert!(unpushed(&w, "T-13-none").await.is_null(), "no worktree, nothing to report");

    // A remote that has the commits clears the count.
    let remote = w.repo.parent().unwrap().join("remote.git");
    git(&["init", "-q", "--bare", remote.to_str().unwrap()], &w.repo);
    git(&["remote", "add", "origin", remote.to_str().unwrap()], &worktree);
    git(&["push", "-q", "origin", "wt"], &worktree);
    assert_eq!(unpushed(&w, "T-12-wt").await, 0);
    git(&["commit", "-q", "--allow-empty", "-m", "b"], &worktree);
    assert_eq!(unpushed(&w, "T-12-wt").await, 1);

    // The remote branch rewritten with the same patches under new hashes (a rebase): nothing is unpushed.
    git(&["push", "-q", "origin", "wt"], &worktree);
    git(&["commit", "-q", "--amend", "--allow-empty", "--no-edit", "--date=2001-01-01T00:00:00"], &worktree);
    git(&["push", "-q", "--force", "origin", "wt"], &worktree);
    git(&["fetch", "-q", "origin"], &worktree);
    git(&["reset", "-q", "--hard", "HEAD~1"], &worktree);
    git(&["commit", "-q", "--allow-empty", "-m", "b"], &worktree);
    assert_eq!(unpushed(&w, "T-12-wt").await, 0, "same patch on the remote under another hash");
}

#[tokio::test]
async fn remove_is_finished_when_the_request_returns() {
    let w = world();
    write_item(&w.repo, "T-11-busy", "analyze", "");
    w.tmux.run(&["new-session", "-d", "-s", "wi-T-11-busy", "-c", w.repo.to_str().unwrap(), "sleep 60"]);
    let git = |args: &[&str], cwd: &Path| {
        let ok = std::process::Command::new("git").args(args).current_dir(cwd).output().unwrap().status.success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q"], &w.repo);
    git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "i"], &w.repo);
    let worktree = w.repo.parent().unwrap().join("repo-T-11-busy");
    git(&["worktree", "add", "-q", "-b", "wt", worktree.to_str().unwrap()], &w.repo);

    let (status, _) = post(&w, "/api/workitems/app/T-11-busy/remove", "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!w.tmux.has("wi-T-11-busy"), "session stopped");
    assert!(!worktree.exists(), "worktree removed");
    assert!(!w.repo.join(".claude/workitems/T-11-busy").exists(), "folder removed");

    // Nothing is left for the dispatcher to pick up.
    settle(&w).await;
    assert!(stub_log(&w.repo).is_empty());
    assert!(!w.tmux.has("wi-T-11-busy"));
}

const PR: &str = "pr: https://github.com/app/pull/7\n";

#[tokio::test]
async fn review_shows_pr_status_and_leaves_green_ci_alone() {
    let w = world();
    write_item(&w.repo, "T-20-green", "in_review", PR);
    set_pr(&w, "OPEN", "APPROVED", "MERGEABLE", "SUCCESS");

    settle(&w).await;
    let items = get(&w, "/api/workitems").await;
    assert!(items.contains(r#""review":{"approval":"approved","approved_by_me":false,"merge":"mergeable","ci":"passing","failed":[]}"#), "{items}");
    assert!(stub_log(&w.repo).is_empty() && !w.tmux.has("wi-T-20-green"), "nothing to fix");
}

#[tokio::test]
async fn merge_button_merges_in_review_items_and_the_next_pass_moves_them_to_done() {
    let w = world();
    write_item(&w.repo, "T-23-early", "in_development", PR);
    let (status, _) = post(&w, "/api/workitems/app/T-23-early/merge", "").await;
    assert_eq!(status, StatusCode::CONFLICT, "only in_review can be merged");

    write_item(&w.repo, "T-24-merge", "in_review", PR);
    set_pr(&w, "OPEN", "APPROVED", "MERGEABLE", "SUCCESS");
    settle(&w).await;
    set_pr(&w, "MERGED", "APPROVED", "MERGEABLE", "SUCCESS");
    let (status, _) = post(&w, "/api/workitems/app/T-24-merge/merge", "").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(field(&w.repo, "T-24-merge", "stage"), "in_review", "the dispatcher moves it, not the button");
    settle(&w).await;
    assert_eq!(field(&w.repo, "T-24-merge", "stage"), "done");
}

#[tokio::test]
async fn rebase_button_only_works_on_in_review_items_with_a_pr() {
    let w = world();
    write_item(&w.repo, "T-25-early", "in_development", PR);
    let (status, _) = post(&w, "/api/workitems/app/T-25-early/rebase", "").await;
    assert_eq!(status, StatusCode::CONFLICT);

    write_item(&w.repo, "T-26-rebase", "in_review", PR);
    set_pr(&w, "OPEN", "APPROVED", "MERGEABLE", "SUCCESS");
    let (status, _) = post(&w, "/api/workitems/app/T-26-rebase/rebase", "").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn review_starts_a_fixer_on_red_ci_and_leaves_a_blocked_one_waiting() {
    let w = world();
    write_item(&w.repo, "T-21-red", "in_review", PR);
    set_pr(&w, "OPEN", "REVIEW_REQUIRED", "MERGEABLE", "FAILURE");
    fs::write(w.repo.join(".stub-in_review"), "block").unwrap();

    settle(&w).await;
    eventually("fixer could not fix it and blocked", || field(&w.repo, "T-21-red", "agent_status").starts_with("blocked")).await;
    let items = get(&w, "/api/workitems").await;
    assert!(items.contains(r#""approval":"review_required","approved_by_me":false,"merge":"mergeable","ci":"failing""#), "{items}");
    let prompts = fs::read_dir(w.dir.path().join("logs")).unwrap().map(|e| fs::read_to_string(e.unwrap().path()).unwrap()).collect::<String>();
    assert!(prompts.contains("CI is failing on https://github.com/app/pull/7"), "{prompts}");

    settle(&w).await;
    settle(&w).await;
    assert!(w.tmux.has("wi-T-21-red"), "a blocked fixer waits for the human");
    assert_eq!(stub_log(&w.repo).len(), 1, "no second fixer while blocked");
}

#[tokio::test]
async fn review_starts_one_fixer_per_changes_requested_review() {
    let w = world();
    write_item(&w.repo, "T-25-changes", "in_review", PR);
    let set_review = |id: &str| {
        let json = serde_json::json!({
            "state": "OPEN", "reviewDecision": "CHANGES_REQUESTED", "mergeable": "MERGEABLE",
            "statusCheckRollup": [{ "name": "rspec", "status": "COMPLETED", "conclusion": "SUCCESS" }],
            "reviews": [{ "id": id, "state": "CHANGES_REQUESTED" }],
        });
        fs::write(w.dir.path().join("pr.json"), json.to_string()).unwrap();
    };
    set_review("r1");
    fs::write(w.repo.join(".stub-in_review"), "done").unwrap();

    settle(&w).await;
    eventually("fixer ran", || stub_log(&w.repo).len() == 1).await;
    let prompts = fs::read_dir(w.dir.path().join("logs")).unwrap().map(|e| fs::read_to_string(e.unwrap().path()).unwrap()).collect::<String>();
    assert!(prompts.contains("A reviewer requested changes on https://github.com/app/pull/7"), "{prompts}");

    for _ in 0..3 {
        settle(&w).await;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
    assert_eq!(stub_log(&w.repo).len(), 1, "the same review starts no second fixer");

    set_review("r2");
    settle(&w).await;
    eventually("a new review starts a new fixer", || stub_log(&w.repo).len() == 2).await;
}

#[tokio::test]
async fn review_starts_one_fixer_per_batch_of_new_comments() {
    let w = world();
    write_item(&w.repo, "T-27-comments", "in_review", PR);
    fs::write(w.repo.join(".stub-in_review"), "done").unwrap();
    // `pr_level` are PR comments, `inline` are review comments (`gh api --paginate --slurp` wraps pages in an array).
    let set_comments = |pr_level: serde_json::Value, inline: serde_json::Value| {
        let json = serde_json::json!({
            "state": "OPEN", "reviewDecision": "", "mergeable": "MERGEABLE",
            "statusCheckRollup": [{ "name": "rspec", "status": "COMPLETED", "conclusion": "SUCCESS" }],
            "comments": pr_level,
        });
        fs::write(w.dir.path().join("pr.json"), json.to_string()).unwrap();
        fs::write(w.dir.path().join("inline.json"), serde_json::json!([inline]).to_string()).unwrap();
    };
    let inline = |login: &str, body: &str, at: &str| serde_json::json!({ "id": at.bytes().map(u64::from).sum::<u64>(), "user": { "login": login }, "body": body, "created_at": at });
    let plain = |login: &str, body: &str, at: &str| serde_json::json!({ "id": at, "author": { "login": login }, "body": body, "createdAt": at });
    let passes = || async {
        for _ in 0..3 {
            settle(&w).await;
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    };

    // Comments that were already on the PR when the item was first seen are not new.
    let old = inline("alice", "rename this", "2026-01-01T10:00:00Z");
    set_comments(serde_json::json!([]), serde_json::json!([old.clone()]));
    passes().await;
    assert_eq!(stub_log(&w.repo).len(), 0, "existing comments start no fixer");

    set_comments(serde_json::json!([]), serde_json::json!([old.clone(), inline("alice", "extract a method", "2026-02-01T10:00:00Z")]));
    settle(&w).await;
    eventually("a new inline comment starts a fixer", || stub_log(&w.repo).len() == 1).await;
    let prompts = fs::read_dir(w.dir.path().join("logs")).unwrap().map(|e| fs::read_to_string(e.unwrap().path()).unwrap()).collect::<String>();
    assert!(prompts.contains("New review comments on https://github.com/app/pull/7"), "{prompts}");
    assert!(prompts.contains("Unsure whether a comment or suggestion is right"), "the fixer asks instead of guessing: {prompts}");
    passes().await;
    assert_eq!(stub_log(&w.repo).len(), 1, "the same comment starts no second fixer");

    // The reader's own replies and plain comments, and other people's plain PR comments, ask for nothing.
    let mut reply = inline("me", "will do", "2026-02-02T10:00:00Z");
    reply["in_reply_to_id"] = serde_json::json!(1);
    let seen = [old.clone(), inline("alice", "extract a method", "2026-02-01T10:00:00Z"), reply];
    set_comments(serde_json::json!([plain("bob", "looks good", "2026-02-03T10:00:00Z")]), serde_json::json!(seen));
    passes().await;
    assert_eq!(stub_log(&w.repo).len(), 1, "chatter starts no fixer");

    set_comments(serde_json::json!([plain("bob", "```suggestion\nfoo\n```", "2026-03-01T10:00:00Z")]), serde_json::json!(seen));
    settle(&w).await;
    eventually("a suggestion starts a fixer", || stub_log(&w.repo).len() == 2).await;

    // The reader's own new inline thread asks for a change, with no marker.
    let mut own = seen.to_vec();
    own.push(inline("me", "why not remove this method?", "2026-04-01T10:00:00Z"));
    set_comments(serde_json::json!([plain("bob", "```suggestion\nfoo\n```", "2026-03-01T10:00:00Z")]), serde_json::json!(own));
    settle(&w).await;
    eventually("the reader's own inline thread starts a fixer", || stub_log(&w.repo).len() == 3).await;
}

#[tokio::test]
async fn review_starts_one_conflict_fixer_per_pr_head() {
    let w = world();
    write_item(&w.repo, "T-26-conflict", "in_review", PR);
    let set_head = |sha: &str| {
        let json = serde_json::json!({
            "state": "OPEN", "reviewDecision": "", "mergeable": "CONFLICTING", "headRefOid": sha,
            "statusCheckRollup": [{ "name": "rspec", "status": "COMPLETED", "conclusion": "SUCCESS" }],
        });
        fs::write(w.dir.path().join("pr.json"), json.to_string()).unwrap();
    };
    set_head("aaa");
    fs::write(w.repo.join(".stub-in_review"), "done").unwrap();

    settle(&w).await;
    eventually("conflict fixer ran", || stub_log(&w.repo).len() == 1).await;
    let prompts = fs::read_dir(w.dir.path().join("logs")).unwrap().map(|e| fs::read_to_string(e.unwrap().path()).unwrap()).collect::<String>();
    assert!(prompts.contains("conflicts with its base branch"), "{prompts}");

    for _ in 0..3 {
        settle(&w).await;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
    assert_eq!(stub_log(&w.repo).len(), 1, "the same head starts no second fixer");
    // The fixer said ok without a push: the item must not sit on ok with the conflict unseen.
    assert!(field(&w.repo, "T-26-conflict", "agent_status").starts_with("blocked: merge conflict still open"));
    assert_eq!(field(&w.repo, "T-26-conflict", "pr_conflict_head"), "", "the head is forgotten while blocked");

    // Clearing the block tries again on the same head; a new head does too.
    set_ok(&w.repo, "T-26-conflict");
    settle(&w).await;
    eventually("clearing the block retries the fixer", || stub_log(&w.repo).len() == 2).await;

    for _ in 0..3 {
        settle(&w).await;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
    set_ok(&w.repo, "T-26-conflict");
    set_head("bbb");
    settle(&w).await;
    eventually("a new head starts a new fixer", || stub_log(&w.repo).len() == 3).await;
}

#[tokio::test]
async fn review_forgets_the_conflict_head_while_the_fixer_is_blocked() {
    let w = world();
    write_item(&w.repo, "T-33-conflict-blocked", "in_review", PR);
    let json = serde_json::json!({ "state": "OPEN", "reviewDecision": "", "mergeable": "CONFLICTING", "headRefOid": "aaa", "statusCheckRollup": [] });
    fs::write(w.dir.path().join("pr.json"), json.to_string()).unwrap();
    fs::write(w.repo.join(".stub-in_review"), "block").unwrap();

    settle(&w).await;
    eventually("the fixer blocked", || field(&w.repo, "T-33-conflict-blocked", "agent_status").starts_with("blocked")).await;
    settle(&w).await;
    eventually("the head is forgotten", || field(&w.repo, "T-33-conflict-blocked", "pr_conflict_head").is_empty()).await;
    assert_eq!(stub_log(&w.repo).len(), 1, "a blocked item starts nothing");
}

/// What clearing a block in the UI leaves behind once the session is gone.
fn set_ok(repo: &Path, dir: &str) {
    let file = repo.join(".claude/workitems").join(dir).join("workitem.md");
    let text = fs::read_to_string(&file).unwrap();
    let text = text.lines().map(|l| if l.starts_with("agent_status:") { "agent_status: ok" } else { l }).collect::<Vec<_>>().join("\n");
    fs::write(&file, text + "\n").unwrap();
}

#[tokio::test]
async fn review_fixes_the_conflict_before_cancelled_ci() {
    let w = world();
    // The CI attempts of an earlier red streak, as the watcher keeps them in the item.
    write_item(&w.repo, "T-31-conflict-ci", "in_review", &format!("{PR}pr_fix_attempts: 3\n"));
    let json = serde_json::json!({
        "state": "OPEN", "reviewDecision": "", "mergeable": "CONFLICTING", "headRefOid": "aaa",
        "statusCheckRollup": [{ "name": "rspec", "status": "COMPLETED", "conclusion": "CANCELLED" }],
    });
    fs::write(w.dir.path().join("pr.json"), json.to_string()).unwrap();
    fs::write(w.repo.join(".stub-in_review"), "done").unwrap();

    settle(&w).await;
    eventually("a fixer ran", || stub_log(&w.repo).len() == 1).await;
    let prompts = fs::read_dir(w.dir.path().join("logs")).unwrap().map(|e| fs::read_to_string(e.unwrap().path()).unwrap()).collect::<String>();
    assert!(prompts.contains("conflicts with its base branch"), "the conflict fixer goes first: {prompts}");
    assert_eq!(field(&w.repo, "T-31-conflict-ci", "pr_fix_attempts"), "0", "the conflict fixer resets the CI attempts");
    assert_eq!(field(&w.repo, "T-31-conflict-ci", "pr_conflict_head"), "aaa", "the head it handled is kept in the item");
}

#[tokio::test]
async fn review_asks_again_while_github_computes_mergeability() {
    let w = world();
    write_item(&w.repo, "T-32-unknown", "in_review", PR);
    let settled = serde_json::json!({ "state": "OPEN", "reviewDecision": "", "mergeable": "CONFLICTING", "headRefOid": "aaa", "statusCheckRollup": [] });
    fs::write(w.dir.path().join("pr.json"), settled.to_string()).unwrap();
    let computing = serde_json::json!({ "state": "OPEN", "reviewDecision": "", "mergeable": "UNKNOWN", "headRefOid": "aaa", "statusCheckRollup": [] });
    fs::write(w.dir.path().join("pr-once.json"), computing.to_string()).unwrap();
    fs::write(w.repo.join(".stub-in_review"), "done").unwrap();

    settle(&w).await;
    eventually("the conflict is seen on the first pass", || stub_log(&w.repo).len() == 1).await;
}

#[tokio::test]
async fn review_blocks_after_the_fix_attempt_cap() {
    let w = world();
    write_item(&w.repo, "T-22-cap", "in_review", PR);
    set_pr(&w, "OPEN", "", "MERGEABLE", "FAILURE");
    fs::write(w.repo.join(".stub-in_review"), "crash").unwrap();

    for _ in 0..12 {
        settle(&w).await;
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        if field(&w.repo, "T-22-cap", "agent_status").starts_with("blocked") {
            break;
        }
    }
    assert_eq!(field(&w.repo, "T-22-cap", "agent_status"), "blocked: CI still failing after 3 fix attempts");
    assert_eq!(stub_log(&w.repo).len(), 3);
}

#[tokio::test]
async fn review_moves_a_merged_pr_to_done_and_blocks_a_closed_one() {
    let w = world();
    write_item(&w.repo, "T-23-merged", "in_review", PR);
    set_pr(&w, "MERGED", "APPROVED", "UNKNOWN", "SUCCESS");
    settle(&w).await;
    assert_eq!(field(&w.repo, "T-23-merged", "stage"), "done");

    write_item(&w.repo, "T-24-closed", "in_review", PR);
    set_pr(&w, "CLOSED", "", "UNKNOWN", "SUCCESS");
    settle(&w).await;
    assert_eq!(field(&w.repo, "T-24-closed", "stage"), "in_review");
    assert_eq!(field(&w.repo, "T-24-closed", "agent_status"), "blocked: PR was closed without merge");
}

#[tokio::test]
async fn pr_tab_gets_description_and_every_kind_of_comment_newest_first() {
    let w = world();
    write_item(&w.repo, "T-30-conversation", "in_review", PR);
    let json = serde_json::json!({
        "body": "## Why\nBecause.",
        "comments": [{ "id": "c1", "author": { "login": "bob" }, "body": "looks good", "createdAt": "2026-02-03T10:00:00Z" }],
        "reviews": [
            { "id": "r1", "state": "COMMENTED", "author": { "login": "carol" }, "body": "", "submittedAt": "2026-02-02T10:00:00Z" },
            { "id": "r2", "state": "APPROVED", "author": { "login": "dave" }, "body": "", "submittedAt": "2026-02-05T10:00:00Z" },
        ],
    });
    fs::write(w.dir.path().join("pr.json"), json.to_string()).unwrap();
    let inline = serde_json::json!([[{ "id": 1, "user": { "login": "alice" }, "body": "rename", "created_at": "2026-02-04T10:00:00Z", "path": "src/a.rs" }]]);
    fs::write(w.dir.path().join("inline.json"), inline.to_string()).unwrap();

    let body: serde_json::Value = serde_json::from_str(&get(&w, "/api/workitems/app/T-30-conversation/pr").await).unwrap();
    assert_eq!(body["body"], "## Why\nBecause.");
    let notes: Vec<(&str, &str)> = body["notes"].as_array().unwrap().iter().map(|n| (n["kind"].as_str().unwrap(), n["author"].as_str().unwrap())).collect();
    // The empty COMMENTED review is noise; an approval is not.
    assert_eq!(notes, [("review", "dave"), ("inline", "alice"), ("comment", "bob")]);
    assert_eq!(body["notes"][1]["path"], "src/a.rs");
}

fn board(stages: &str) -> String {
    format!(r#"{{ "brief_sections": [], "entry": "qa", "stages": {stages} }}"#)
}

#[tokio::test]
async fn project_board_decides_which_stages_the_dispatcher_runs() {
    let w = world();
    write_item(&w.repo, "T-20-qa", "qa", "");
    write_item(&w.repo, "T-21-off", "backlog", "");
    let file = w.repo.join(".claude/workitems/pipeline.json");
    fs::write(&file, board(r#"[{"name":"qa","handler":"my-qa","auto":true,"next":["shipped"]},{"name":"shipped"}]"#)).unwrap();

    write_skill(&w.repo.join(".claude/workitems/skills"), "my-qa", "QA STEPS\n");

    settle(&w).await;
    eventually("the project's own handler ran", || stub_log(&w.repo) == vec!["T-20-qa qa"]).await;
    assert!(!w.tmux.has("wi-T-21-off"), "a stage that is not on the board has no handler");
    let args = args_log(&w);
    assert!(args.contains("QA STEPS"), "{args}");
    assert!(args.contains("Current stage: qa"), "{args}");
    assert!(args.contains("Allowed next stages: shipped (default)"), "{args}");
}

#[tokio::test]
async fn invalid_board_file_keeps_the_last_valid_board_and_is_reported() {
    let w = world();
    let stages = |body: &str| -> serde_json::Value {
        let boards: serde_json::Value = serde_json::from_str(body).unwrap();
        boards["app"][0].clone()
    };
    let file = w.repo.join(".claude/workitems/pipeline.json");
    fs::create_dir_all(file.parent().unwrap()).unwrap();

    let bundled = stages(&get(&w, "/api/pipelines").await);
    assert_eq!(bundled["error"], serde_json::Value::Null);
    assert_eq!(bundled["pipeline"]["stages"][0]["name"], "backlog");

    fs::write(&file, board(r#"[{"name":"qa","next":["shipped"]},{"name":"shipped"}]"#)).unwrap();
    let own = stages(&get(&w, "/api/pipelines").await);
    assert_eq!(own["pipeline"]["stages"][0]["name"], "qa");
    assert_eq!(own["pipeline"]["entry"], "qa");

    fs::write(&file, board(r#"[{"name":"qa","next":["gone"]}]"#)).unwrap();
    let broken = stages(&get(&w, "/api/pipelines").await);
    assert!(broken["error"].as_str().unwrap().contains("gone"), "{broken}");
    assert_eq!(broken["pipeline"]["stages"][0]["name"], "qa", "the last valid board stays");

    fs::write(&file, board(r#"[{"name":"qa"}]"#)).unwrap();
    assert_eq!(stages(&get(&w, "/api/pipelines").await)["error"], serde_json::Value::Null);
}

/// A board with its own names: a gate, a development stage and a PR stage under other names.
const OWN_BOARD: &str = r#"{
  "brief_sections": [], "entry": "idea",
  "stages": [
    { "name": "idea", "next": ["verify"] },
    { "name": "verify", "next": ["build", "idea"], "on_enter": { "approved": "false" },
      "gate": { "label": "Implementation approved", "approve_to": "build", "reject_to": "idea" } },
    { "name": "build", "handler": "workitem-start", "auto": true, "next": ["waiting"], "require": { "approved": "true" } },
    { "name": "waiting", "handler": "builtin:pr-review", "auto": true, "next": ["shipped"], "on_merge": "shipped" },
    { "name": "shipped" }
  ]
}"#;

#[tokio::test]
async fn gates_and_the_pr_stage_follow_the_project_board_not_stage_names() {
    let w = world();
    fs::create_dir_all(w.repo.join(".claude/workitems")).unwrap();
    fs::write(w.repo.join(".claude/workitems/pipeline.json"), OWN_BOARD).unwrap();
    write_item(&w.repo, "T-40-reject", "verify", "");
    write_item(&w.repo, "T-41-approve", "verify", "");
    write_item(&w.repo, "T-42-not-a-gate", "idea", "");
    write_item(&w.repo, "T-43-merged", "waiting", PR);

    let items: serde_json::Value = serde_json::from_str(&get(&w, "/api/workitems").await).unwrap();
    let role = |dir: &str, key: &str| items.as_array().unwrap().iter().find(|i| i["dir"] == dir).unwrap()[key].clone();
    assert_eq!(role("T-41-approve", "gate")["label"], "Implementation approved");
    assert_eq!(role("T-42-not-a-gate", "gate"), serde_json::Value::Null);
    assert_eq!(role("T-43-merged", "pr_watch"), true);

    let (status, _) = post(&w, "/api/workitems/app/T-42-not-a-gate/approve", "").await;
    assert_eq!(status, StatusCode::CONFLICT, "only a gate stage can be approved");

    let (status, _) = post(&w, "/api/workitems/app/T-40-reject/reject", r#"{"comment":"no"}"#).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(field(&w.repo, "T-40-reject", "stage"), "idea");

    let (status, _) = post(&w, "/api/workitems/app/T-41-approve/approve", "").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(field(&w.repo, "T-41-approve", "stage"), "build");
    assert_eq!(field(&w.repo, "T-41-approve", "approved"), "true");

    set_pr(&w, "MERGED", "APPROVED", "UNKNOWN", "SUCCESS");
    settle(&w).await;
    assert_eq!(field(&w.repo, "T-43-merged", "stage"), "shipped", "on_merge decides where a merged PR goes");
}

#[tokio::test]
async fn a_refused_approval_move_leaves_the_item_unapproved() {
    let w = world();
    fs::create_dir_all(w.repo.join(".claude/workitems")).unwrap();
    fs::write(
        w.repo.join(".claude/workitems/pipeline.json"),
        r#"{ "brief_sections": [], "entry": "a", "stages": [
            { "name": "a", "next": ["b"], "gate": { "label": "Go", "approve_to": "b", "reject_to": "b" } },
            { "name": "b", "require": { "issue": "linked" } } ] }"#,
    )
    .unwrap();
    write_item(&w.repo, "T-44-no-issue", "a", "");
    let text = fs::read_to_string(w.repo.join(".claude/workitems/T-44-no-issue/workitem.md")).unwrap();
    fs::write(w.repo.join(".claude/workitems/T-44-no-issue/workitem.md"), text.replace("issue: T-1", "issue: none")).unwrap();

    let (status, body) = post(&w, "/api/workitems/app/T-44-no-issue/approve", "").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(field(&w.repo, "T-44-no-issue", "stage"), "a");
    assert_eq!(field(&w.repo, "T-44-no-issue", "approved"), "false");
}

fn write_skill(skills: &Path, name: &str, body: &str) {
    fs::create_dir_all(skills.join(name)).unwrap();
    fs::write(skills.join(name).join("SKILL.md"), format!("---\nname: {name}\ndescription: test\n---\n\n{body}")).unwrap();
}

fn args_log(w: &World) -> String {
    fs::read_dir(w.dir.path().join("logs")).unwrap().map(|e| fs::read_to_string(e.unwrap().path()).unwrap()).collect()
}

#[tokio::test]
async fn items_follow_the_board_they_name_and_boards_are_listed_default_first() {
    let w = world();
    let boards = w.repo.join(".claude/workitems/boards");
    fs::create_dir_all(&boards).unwrap();
    fs::write(boards.join("research.json"), board(r#"[{"name":"qa","handler":"my-qa","auto":true,"next":["shipped"]},{"name":"shipped"}]"#)).unwrap();
    write_skill(&boards.join("research/skills"), "my-qa", "RESEARCH QA\n");
    fs::write(boards.join("a-first.json"), board(r#"[{"name":"qa"}]"#)).unwrap();
    fs::write(boards.join("not-a-board.txt"), "x").unwrap();
    write_item(&w.repo, "T-50-research", "qa", "board: research\n");
    write_item(&w.repo, "T-51-lost", "qa", "board: gone\n");
    write_item(&w.repo, "T-52-default", "backlog", "");

    let listed: serde_json::Value = serde_json::from_str(&get(&w, "/api/pipelines").await).unwrap();
    let names: Vec<&str> = listed["app"].as_array().unwrap().iter().map(|b| b["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["default", "a-first", "research"]);

    let items: serde_json::Value = serde_json::from_str(&get(&w, "/api/workitems").await).unwrap();
    let board_of = |dir: &str| items.as_array().unwrap().iter().find(|i| i["dir"] == dir).unwrap()["board"].clone();
    assert_eq!(board_of("T-50-research"), "research");
    assert_eq!(board_of("T-51-lost"), "gone", "an item on a board without a file is still listed");
    assert_eq!(board_of("T-52-default"), "default");

    settle(&w).await;
    eventually("the research board's handler ran", || stub_log(&w.repo).contains(&"T-50-research qa".to_string())).await;
    assert!(!w.tmux.has("wi-T-51-lost"), "no board file, no handler");
    assert!(args_log(&w).contains("RESEARCH QA"));
}

#[tokio::test]
async fn new_workitems_start_on_a_board_of_the_project() {
    let w = world();
    fs::create_dir_all(w.repo.join(".claude/workitems/boards")).unwrap();
    fs::write(w.repo.join(".claude/workitems/boards/research.json"), board(r#"[{"name":"qa"}]"#)).unwrap();

    let (status, _) = post(&w, "/api/projects/app/create", r#"{"input":"look into x","board":"nope"}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = post(&w, "/api/projects/app/create", r#"{"input":"look into x","board":"../x"}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = post(&w, "/api/projects/app/create", r#"{"input":"look into x","board":"research"}"#).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    eventually("the create session got the board", || args_log(&w).contains("--board research")).await;
    let (status, _) = post(&w, "/api/projects/app/create", r#"{"input":"and y"}"#).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    eventually("a default create has no --board", || args_log(&w).contains("and y")).await;
    let repo = w.repo.display();
    assert_eq!(args_log(&w).matches(&format!("arguments --repo {repo} --board research.")).count(), 1);
    assert_eq!(args_log(&w).matches(&format!("arguments --repo {repo}. Follow")).count(), 1);
}

#[tokio::test]
async fn handler_instructions_come_from_the_board_then_the_project_then_the_bundled_skills() {
    let w = world();
    let project = w.state.store.project("app").unwrap();
    let workitems = w.repo.join(".claude/workitems");
    fs::create_dir_all(&workitems).unwrap();
    let find = |board: &str, name: &str| workitem_ui::launch::instructions(&w.state, &project, board, name);

    write_skill(&workitems.join("skills"), "my-qa", "PROJECT QA\n");
    assert_eq!(find("research", "my-qa").unwrap(), "PROJECT QA\n", "front matter is stripped");
    write_skill(&workitems.join("boards/research/skills"), "my-qa", "BOARD QA\n");
    assert_eq!(find("research", "my-qa").unwrap(), "BOARD QA\n");
    assert_eq!(find("default", "my-qa").unwrap(), "PROJECT QA\n", "another board still gets the project's");

    let bundled = find("default", "workitem-triage").unwrap();
    assert!(!bundled.is_empty() && !bundled.starts_with("---"), "{bundled}");
    write_skill(&workitems.join("skills"), "workitem-start", "MY START\n");
    assert_eq!(find("default", "workitem-start").unwrap(), "MY START\n", "a project overrides a bundled skill");

    assert!(find("default", "nope").unwrap_err().contains("not found"));
    for bad in ["../x", "a/b", "", "Upper"] {
        assert!(find("default", bad).unwrap_err().contains("not a handler name"), "{bad}");
    }
}

#[tokio::test]
async fn a_missing_handler_blocks_the_item_and_an_inline_prompt_needs_no_file() {
    let w = world();
    fs::create_dir_all(w.repo.join(".claude/workitems")).unwrap();
    fs::write(
        w.repo.join(".claude/workitems/pipeline.json"),
        board(r#"[{"name":"qa","handler":"no-such-skill","auto":true,"next":["think"]},{"name":"think","prompt":"THINK HARD about it.","auto":true,"next":["qa"]}]"#),
    )
    .unwrap();
    write_item(&w.repo, "T-60-missing", "qa", "");
    write_item(&w.repo, "T-61-inline", "think", "");

    settle(&w).await;
    let status = field(&w.repo, "T-60-missing", "agent_status");
    assert!(status.starts_with("blocked: handler 'no-such-skill' not found"), "{status}");
    assert!(!w.tmux.has("wi-T-60-missing"));
    eventually("the inline prompt ran", || stub_log(&w.repo).contains(&"T-61-inline think".to_string())).await;
    let args = args_log(&w);
    assert!(args.contains("THINK HARD about it."), "{args}");
    assert!(args.contains("Allowed next stages: qa (default)"), "{args}");
}

fn queue_board(repo: &Path) {
    let board = r#"{ "brief_sections": [], "entry": "queue", "stages": [
        { "name": "queue", "auto": true, "next": ["work"] },
        { "name": "work", "handler": "h", "auto": true, "limit": 1, "next": ["post"] },
        { "name": "post", "handler": "h", "auto": true, "once": true, "when": { "approved": "true" }, "next": [] }
    ] }"#;
    let dir = repo.join(".claude/workitems");
    fs::create_dir_all(dir.join("skills/h")).unwrap();
    fs::write(dir.join("skills/h/SKILL.md"), "Do the work.\n").unwrap();
    fs::write(dir.join("pipeline.json"), board).unwrap();
}

#[tokio::test]
async fn a_queue_stage_feeds_the_next_one_oldest_first_up_to_its_limit() {
    let w = world();
    queue_board(&w.repo);
    write_item(&w.repo, "T-20-new", "queue", "created: 2026-01-02T10:00:00Z\n");
    write_item(&w.repo, "T-21-old", "queue", "created: 2026-01-01T10:00:00Z\n");
    write_item(&w.repo, "T-22-busy", "queue", "created: 2026-01-03T10:00:00Z\nagent_status: working\n");

    settle(&w).await;
    eventually("the oldest item is pulled first", || field(&w.repo, "T-21-old", "stage") == "work").await;
    assert_eq!(field(&w.repo, "T-20-new", "stage"), "queue", "the limit of 1 keeps the next one waiting");
    assert_eq!(field(&w.repo, "T-22-busy", "stage"), "queue");

    settle(&w).await;
    eventually("the work handler started for the pulled item", || stub_log(&w.repo) == vec!["T-21-old work"]).await;
    assert_eq!(field(&w.repo, "T-20-new", "stage"), "queue", "still full while the first item is in work");
}

#[tokio::test]
async fn once_and_when_keep_a_final_handler_from_starting_again_or_for_the_wrong_item() {
    let w = world();
    queue_board(&w.repo);
    write_item(&w.repo, "T-23-rejected", "post", "");
    write_item(&w.repo, "T-24-approved", "post", "approved: true\n");
    fs::write(w.repo.join(".stub-post"), "done").unwrap();

    settle(&w).await;
    eventually("only the approved item got a handler", || stub_log(&w.repo) == vec!["T-24-approved post"]).await;

    settle(&w).await;
    settle(&w).await;
    settle(&w).await;
    assert_eq!(stub_log(&w.repo), vec!["T-24-approved post"], "the finished handler is not started again");
}

#[tokio::test]
async fn a_pr_status_stage_shows_the_pr_chips_and_starts_nothing() {
    let w = world();
    queue_board(&w.repo);
    let board = fs::read_to_string(w.repo.join(".claude/workitems/pipeline.json")).unwrap();
    fs::write(w.repo.join(".claude/workitems/pipeline.json"), board.replace(r#""name": "post", "handler""#, r#""name": "post", "pr_status": true, "handler""#)).unwrap();
    write_item(&w.repo, "T-30-waiting", "post", PR);
    write_item(&w.repo, "T-31-no-chips", "queue", PR);
    set_pr(&w, "OPEN", "CHANGES_REQUESTED", "CONFLICTING", "FAILURE");

    settle(&w).await;
    let items: serde_json::Value = serde_json::from_str(&get(&w, "/api/workitems").await).unwrap();
    let review = |dir: &str| items.as_array().unwrap().iter().find(|i| i["dir"] == dir).unwrap()["review"].clone();
    assert_eq!(review("T-30-waiting")["approval"], "changes_requested");
    assert!(review("T-31-no-chips").is_null(), "only stages with pr_status poll");
    assert!(stub_log(&w.repo).is_empty() && !w.tmux.has("wi-T-30-waiting"), "red CI and conflicts start no fixer");
    assert_eq!(review("T-30-waiting")["approved_by_me"], false);

    // The stub's `gh api user` is "me": an approval by that login lights the extra icon.
    let mut pr: serde_json::Value = serde_json::from_str(&fs::read_to_string(w.dir.path().join("pr.json")).unwrap()).unwrap();
    pr["reviews"] = serde_json::json!([{ "id": "r1", "state": "APPROVED", "author": { "login": "me" } }]);
    fs::write(w.dir.path().join("pr.json"), pr.to_string()).unwrap();
    w.state.reviews.lock().unwrap().clear();
    settle(&w).await;
    let items: serde_json::Value = serde_json::from_str(&get(&w, "/api/workitems").await).unwrap();
    let mine = items.as_array().unwrap().iter().find(|i| i["dir"] == "T-30-waiting").unwrap()["review"]["approved_by_me"].clone();
    assert_eq!(mine, true);
}

#[tokio::test]
async fn an_item_whose_pr_head_moved_past_its_head_sha_is_flagged() {
    let w = world();
    queue_board(&w.repo);
    let board = fs::read_to_string(w.repo.join(".claude/workitems/pipeline.json")).unwrap();
    fs::write(w.repo.join(".claude/workitems/pipeline.json"), board.replace(r#""name": "post", "handler""#, r#""name": "post", "pr_status": true, "handler""#)).unwrap();
    write_item(&w.repo, "T-40-seen", "post", &format!("{PR}head_sha: aaa\n"));
    write_item(&w.repo, "T-41-moved", "post", &format!("{PR}head_sha: old\n"));
    write_item(&w.repo, "T-42-no-sha", "post", PR);
    let pr = serde_json::json!({ "state": "OPEN", "reviewDecision": "", "mergeable": "MERGEABLE", "headRefOid": "aaa", "statusCheckRollup": [] });
    fs::write(w.dir.path().join("pr.json"), pr.to_string()).unwrap();

    settle(&w).await;
    let items: serde_json::Value = serde_json::from_str(&get(&w, "/api/workitems").await).unwrap();
    let moved = |dir: &str| items.as_array().unwrap().iter().find(|i| i["dir"] == dir).unwrap()["pr_moved"].clone();
    assert_eq!(moved("T-40-seen"), false);
    assert_eq!(moved("T-41-moved"), true);
    assert_eq!(moved("T-42-no-sha"), false, "an item without head_sha never saw a head");
}

fn write_watch_script(repo: &Path, name: &str, body: &str) {
    let dir = repo.join(".claude/workitems/watch");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/bash\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[tokio::test]
async fn a_column_watcher_runs_its_script_once_per_interval_with_the_column_as_context() {
    let w = world();
    write_item(&w.repo, "T-30-in", "queue", "");
    write_item(&w.repo, "T-31-out", "other", "");
    write_watch_script(&w.repo, "record", "echo \"$WORKITEM_BOARD $WORKITEM_STAGE $WORKITEM_NEXT $1\" >> .watch-log\ncat >> .watch-items");
    fs::write(
        w.repo.join(".claude/workitems/pipeline.json"),
        board(r#"[{"name":"queue","next":["other"],"watch":{"every":"1d","command":"record","args":["hello"]}},{"name":"other"}]"#).replace(r#""entry": "qa""#, r#""entry": "queue""#),
    )
    .unwrap();

    dispatch::pass(&w.state).await;
    eventually("the watcher ran", || w.repo.join(".watch-items").exists()).await;
    dispatch::pass(&w.state).await;
    dispatch::pass(&w.state).await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    assert_eq!(fs::read_to_string(w.repo.join(".watch-log")).unwrap(), "default queue other hello\n", "once, not per pass");
    let items = fs::read_to_string(w.repo.join(".watch-items")).unwrap();
    assert!(items.contains("T-30-in") && !items.contains("T-31-out"), "only the column's items: {items}");
}

#[tokio::test]
async fn a_watcher_with_a_missing_script_does_nothing_and_does_not_stop_the_dispatcher() {
    let w = world();
    write_item(&w.repo, "T-32-qa", "qa", "");
    fs::write(
        w.repo.join(".claude/workitems/pipeline.json"),
        board(r#"[{"name":"qa","handler":"my-qa","auto":true,"next":["x"]},{"name":"x","watch":{"every":"1m","command":"nope"}}]"#),
    )
    .unwrap();
    write_skill(&w.repo.join(".claude/workitems/skills"), "my-qa", "QA\n");
    settle(&w).await;
    eventually("handlers still run", || stub_log(&w.repo) == vec!["T-32-qa qa"]).await;
}

#[tokio::test]
async fn the_bundled_age_out_script_deletes_or_moves_old_items_and_spares_new_ones_and_open_prs() {
    let w = world();
    write_item(&w.repo, "T-40-old", "done", "created: 2020-01-01T10:00:00Z\n");
    write_item(&w.repo, "T-41-new", "done", &format!("created: {}\n", "2999-01-01T10:00:00Z"));
    write_item(&w.repo, "T-42-pr", "done", "created: 2020-01-01T10:00:00Z\npr: https://github.com/o/r/pull/1\n");
    fs::write(
        w.repo.join(".claude/workitems/pipeline.json"),
        board(r#"[{"name":"done","watch":{"every":"1d","command":"age-out","args":["--older-than","30d","--delete"]}}]"#).replace(r#""entry": "qa""#, r#""entry": "done""#),
    )
    .unwrap();
    settle(&w).await;
    let items = w.repo.join(".claude/workitems");
    eventually("the old item is deleted", || !items.join("T-40-old").exists()).await;
    assert!(items.join("T-41-new").exists() && items.join("T-42-pr").exists());
}

async fn post_bearer(w: &World, uri: &str) -> (StatusCode, String) {
    let request = Request::post(uri)
        .header(header::HOST, format!("127.0.0.1:{}", w.state.port))
        .header(header::AUTHORIZATION, format!("Bearer {}", w.token))
        .body(Body::empty())
        .unwrap();
    let response = router(w.state.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    (status, String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes()).into_owned())
}

#[tokio::test]
async fn run_starts_the_stage_handler_of_an_idle_item_for_a_script_with_the_token() {
    let w = world();
    write_item(&w.repo, "T-50-qa", "qa", "");
    write_item(&w.repo, "T-51-blocked", "qa", "agent_status: blocked: need input\n");
    write_item(&w.repo, "T-52-manual", "shipped", "");
    fs::write(
        w.repo.join(".claude/workitems/pipeline.json"),
        board(r#"[{"name":"qa","handler":"my-qa","next":["shipped"]},{"name":"shipped"}]"#),
    )
    .unwrap();
    write_skill(&w.repo.join(".claude/workitems/skills"), "my-qa", "QA STEPS\n");

    let denied = Request::post("/api/workitems/app/T-50-qa/run").header(header::HOST, format!("127.0.0.1:{}", w.state.port)).body(Body::empty()).unwrap();
    assert_eq!(router(w.state.clone()).oneshot(denied).await.unwrap().status(), StatusCode::UNAUTHORIZED);

    let (status, body) = post_bearer(&w, "/api/workitems/app/T-50-qa/run").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    eventually("the handler ran", || stub_log(&w.repo) == vec!["T-50-qa qa"]).await;
    assert_eq!(field(&w.repo, "T-50-qa", "agent_status"), "working");
    assert!(args_log(&w).contains("QA STEPS"));

    assert_eq!(post_bearer(&w, "/api/workitems/app/T-50-qa/run").await.0, StatusCode::CONFLICT, "already running");
    assert_eq!(post_bearer(&w, "/api/workitems/app/T-51-blocked/run").await.0, StatusCode::CONFLICT, "blocked items are left alone");
    let (status, body) = post_bearer(&w, "/api/workitems/app/T-52-manual/run").await;
    assert_eq!(status, StatusCode::CONFLICT, "a stage without a handler has nothing to run");
    assert!(body.contains("no handler"), "{body}");
}

#[tokio::test]
async fn the_default_board_can_live_in_the_boards_folder() {
    let w = world();
    let dir = w.repo.join(".claude/workitems");
    fs::create_dir_all(dir.join("boards")).unwrap();
    let project = w.state.store.project("app").unwrap();
    let entry = |w: &World| w.state.pipeline(&project, "default").pipeline.unwrap().entry;
    assert_eq!(entry(&w), "backlog", "the bundled board without a project file");

    fs::write(dir.join("pipeline.json"), board(r#"[{"name":"old"}]"#).replace(r#""entry": "qa""#, r#""entry": "old""#)).unwrap();
    assert_eq!(entry(&w), "old");
    fs::write(dir.join("boards/default.json"), board(r#"[{"name":"new"}]"#).replace(r#""entry": "qa""#, r#""entry": "new""#)).unwrap();
    assert_eq!(entry(&w), "new", "boards/default.json wins over pipeline.json");
    assert_eq!(w.state.board_names(&project), vec!["default"], "it is not listed as a second board");
}

#[tokio::test]
async fn stage_source_shows_the_files_the_dispatcher_resolves() {
    let w = world();
    let workitems = w.repo.join(".claude/workitems");
    fs::create_dir_all(workitems.join("boards")).unwrap();
    fs::write(
        workitems.join("boards/research.json"),
        board(
            r#"[{"name":"qa","handler":"my-qa","auto":true},
                {"name":"inline","prompt":"Do the thing."},
                {"name":"pr","handler":"builtin:pr-review","auto":true,"next":["gate"],"on_merge":"gate"},
                {"name":"gate"},
                {"name":"lost","handler":"nope","auto":true}]"#,
        ),
    )
    .unwrap();
    write_skill(&workitems.join("skills"), "my-qa", "PROJECT QA\n");
    write_skill(&workitems.join("boards/research/skills"), "my-qa", "BOARD QA\n");
    let source = |stage: &str| {
        let w = &w;
        let uri = format!("/api/projects/app/boards/research/stages/{stage}/source");
        async move { serde_json::from_str::<serde_json::Value>(&get(w, &uri).await).unwrap() }
    };

    let qa = source("qa").await;
    assert_eq!(qa["prose"]["origin"], "board");
    assert!(qa["prose"]["content"].as_str().unwrap().contains("BOARD QA"));
    assert!(qa["prose"]["path"].as_str().unwrap().ends_with("boards/research/skills/my-qa/SKILL.md"));
    assert_eq!(qa["scripts"][0]["name"], "run-items");
    assert_eq!(qa["scripts"][0]["note"], "implicit");
    assert_eq!(qa["scripts"][0]["origin"], "bundled");
    assert!(qa["scripts"][0]["content"].as_str().unwrap().starts_with("#!"));

    let inline = source("inline").await;
    assert_eq!(inline["prose"]["content"], "Do the thing.");
    assert_eq!(inline["prose"]["note"], "inline in pipeline.json");
    assert!(inline["scripts"].as_array().unwrap().is_empty());

    let pr = source("pr").await;
    assert!(pr["prose"].is_null(), "a builtin handler has no skill");
    assert_eq!(pr["scripts"][0]["name"], "pr-review");

    let gate = source("gate").await;
    assert!(gate["prose"].is_null() && gate["scripts"].as_array().unwrap().is_empty());

    let lost = source("lost").await;
    assert!(lost["prose"]["error"].as_str().unwrap().contains("not found"));

    for uri in [
        "/api/projects/app/boards/research/stages/nope/source",
        "/api/projects/app/boards/nope/stages/qa/source",
        "/api/projects/app/boards/..%2Fx/stages/qa/source",
        "/api/projects/other/boards/research/stages/qa/source",
    ] {
        assert!(get(&w, uri).await.contains("not found"), "{uri}");
    }
}

#[tokio::test]
async fn files_tab_lists_and_serves_the_workitem_folder_sandboxed() {
    let w = world();
    write_item(&w.repo, "T-1-files", "backlog", "");
    let dir = w.repo.join(".claude/workitems/T-1-files");
    fs::create_dir_all(dir.join("notes")).unwrap();
    fs::write(dir.join("notes/page.html"), "<script>x</script>").unwrap();
    fs::write(w.repo.join(".claude/workitems/secret.txt"), "no").unwrap();

    let list: serde_json::Value = serde_json::from_str(&get(&w, "/api/workitems/app/T-1-files/files").await).unwrap();
    let paths: Vec<_> = list.as_array().unwrap().iter().map(|e| e["path"].as_str().unwrap()).collect();
    assert_eq!(paths, ["notes/page.html", "workitem.md"]);

    let request = Request::get("/api/workitems/app/T-1-files/files/notes/page.html")
        .header(header::HOST, format!("127.0.0.1:{}", w.state.port))
        .header(header::COOKIE, format!("wi_session={}", w.token))
        .body(Body::empty())
        .unwrap();
    let response = router(w.state.clone()).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "text/html");
    assert_eq!(response.headers()[header::CONTENT_SECURITY_POLICY], "sandbox");

    for uri in ["/api/workitems/app/T-1-files/files/..%2Fsecret.txt", "/api/workitems/app/T-1-files/files/notes"] {
        assert!(get(&w, uri).await.contains("file not found"), "{uri}");
    }
    for path in ["../secret.txt", "notes", "/etc/hosts"] {
        let (status, _) = post(&w, "/api/workitems/app/T-1-files/reveal", &serde_json::json!({ "path": path }).to_string()).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
}
