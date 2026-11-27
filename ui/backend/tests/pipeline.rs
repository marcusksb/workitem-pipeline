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

/// Behaviour per stage comes from `<repo>/.stub-<stage>`: `advance`, `block`, `wait`, `done`, `crash`, or idle.
const STUB: &str = r#"#!/bin/bash
repo="$WORKITEM_REPO"
file="$WORKITEM_FILE"
{ echo "argc=$#"; [ -n "$CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD" ] && echo "claude_md=$CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD"; printf 'last=%s\n' "${@: -1}"; a=("$@"); for i in "${!a[@]}"; do [ "${a[$i]}" = --model ] && echo "model=${a[$((i+1))]}"; [ "${a[$i]}" = --mcp-config ] && echo "mcp=${a[$((i+1))]}"; [ "${a[$i]}" = --settings ] && echo "settings=${a[$((i+1))]}"; done; [ -n "$GH_TOKEN" ] && echo "gh_token=$GH_TOKEN"; } > "$LOGDIR/args-$(date +%s%N)"
[ -n "$WORKITEM" ] && echo "$PWD|$WORKITEM_REPO|$WORKITEM_WORKTREE" > "$LOGDIR/../cwd-$WORKITEM"
if [ -n "$WORKITEM" ]; then
  stage=$(perl -ne 'print $1 if /^stage:\s*(\S+)/' "$file")
  echo "$WORKITEM $stage" >> "$repo/.stub-log"
  mode=$(cat "$repo/.stub-$stage" 2>/dev/null || echo idle)
  case "$mode" in
    advance) printf '\n## Review brief\n\n### Findings\nx\n### Plan\nx\n### Risk\nx\n### Size\nx\n### Test plan\nx\n### Assumptions\nx\n### Links\nx\n' >> "$file"
      python3 "$SKILLS/workitem-pipeline/bin/workitem-move" "$file" ready_for_development ;;
    block) perl -pi -e 's/^agent_status:.*/agent_status: blocked: need input/' "$file" ;;
    wait) perl -pi -e 's/^agent_status:.*/agent_status: waiting: CI/' "$file" ;;
    done) perl -pi -e 's/^agent_status:.*/agent_status: ok/' "$file" ;;
    crash) exit 0 ;;
  esac
fi
sleep 60
"#;

/// `gh api graphql` for `watch/pr-status`: every PR the query asks for is `pr.json` (or `pr-once.json`,
/// read once) in the shape of `gh pr view --json`, with `inline.json` (REST pages) as its review threads.
/// `updatedAt` follows the files, as GitHub's moves with every comment.
const GH_GRAPHQL: &str = r#"import json, os, re, sys
d, query = sys.argv[1], sys.argv[2].split("=", 1)[1]
def load(name, default):
    try:
        return json.load(open(os.path.join(d, name)))
    except OSError:
        return default
inline = [c for page in load("inline.json", []) for c in page]
once = os.path.join(d, "pr-once.json")
threads = "reviewThreads" in query
if not threads and os.path.exists(once):
    view = load("pr-once.json", {})
    os.remove(once)
else:
    view = load("pr.json", {})
checks = view.get("statusCheckRollup") or []
pull = {**view, "createdAt": "2026-01-01T00:00:00Z", "updatedAt": json.dumps([view, inline]),
        "commits": {"nodes": [{"commit": {"committedDate": "2026-01-01T00:00:00Z",
                                          "statusCheckRollup": {"contexts": {"nodes": checks}} if checks else None}}]},
        "comments": {"nodes": view.get("comments") or []}, "reviews": {"nodes": view.get("reviews") or []},
        "reviewThreads": {"nodes": [{"comments": {"nodes": [{"databaseId": c["id"], "body": c.get("body", ""),
            "author": {"login": c["user"]["login"]},
            "replyTo": {"databaseId": c["in_reply_to_id"]} if c.get("in_reply_to_id") else None}]}} for c in inline]}}
pulls = {alias: pull for alias in re.findall(r"(p\d+): pullRequest", query)}
print(json.dumps({"data": {"viewer": {"login": "me"}, "repository": pulls}}))
"#;

struct World {
    dir: tempfile::TempDir,
    repo: PathBuf,
    /// The workspace folder; its board `default` is the bundled one for `repo`.
    ws: PathBuf,
    tmux: Tmux,
    state: Shared,
    token: String,
}

fn skills() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills").canonicalize().unwrap()
}

/// An item in the workspace `ws`, on board `default` unless `extra` names one.
fn write_item(ws: &Path, dir: &str, stage: &str, extra: &str) {
    let item = ws.join(dir);
    fs::create_dir_all(&item).unwrap();
    let board = if extra.contains("board:") { "" } else { "board: default\n" };
    fs::write(
        item.join("workitem.md"),
        format!("---\ntitle: {dir}\ntype: bug\nstage: {stage}\n{board}approved: false\nagent_status: ok\nissue: T-1\n{extra}---\n\n# {dir}\n"),
    )
    .unwrap();
}

/// A board file with `repo` set to the world's repo, the rest of the text as it is.
fn board_file(w: &World, json: &str) -> String {
    let rest = json.trim_start().strip_prefix('{').unwrap();
    format!("{{ \"repo\": {},{rest}", serde_json::to_string(&w.repo.display().to_string()).unwrap())
}

fn world() -> World {
    // The bundled board's `issue-status` watcher must not reach the real Jira: without a jira-cli
    // config it does nothing.
    std::env::set_var("JIRA_CONFIG_FILE", "/nonexistent/jira.yml");
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo").canonicalize_or_create();
    let logs = dir.path().join("logs");
    fs::create_dir_all(&logs).unwrap();
    let stub = dir.path().join("claude");
    fs::write(&stub, STUB.replace("$SKILLS", &skills().display().to_string()).replace("$LOGDIR", &logs.display().to_string())).unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();

    let gh = dir.path().join("gh");
    // `gh api graphql` is pr-status (GH_GRAPHQL); for the PR tab, `gh api user` names the reader,
    // `gh api <path>` lists inline comments, anything else is `gh pr view`.
    let d = dir.path().display();
    fs::write(dir.path().join("gh_graphql.py"), GH_GRAPHQL).unwrap();
    let script = format!(
        "#!/bin/bash\nif [ \"$1 $2\" = 'auth token' ]; then [ \"$4\" = work-bot ] && echo tok-123 && exit 0; echo 'no account' >&2; exit 1\nelif [ \"$1 $2\" = 'api graphql' ]; then exec python3 {d}/gh_graphql.py {d} \"$4\"\nelif [ \"$1\" = api ]; then\n  if [ \"$2\" = user ]; then echo '{{\"login\":\"me\"}}'; else cat {d}/inline.json 2>/dev/null || echo '[]'; fi\nelif [ -f {d}/pr-once.json ]; then cat {d}/pr-once.json; rm {d}/pr-once.json\nelse cat {d}/pr.json; fi\n"
    );
    fs::write(&gh, script).unwrap();
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();

    let store = Store::open(dir.path().join("home"), None).unwrap();
    let ws = store.add_workspace("app").unwrap().dir;
    fs::create_dir_all(ws.join("boards/default")).unwrap();
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
    state.ask_wait = std::time::Duration::from_millis(1500);
    let board = workitem_ui::pipeline::NewBoard { name: "default", repo: repo.to_str().unwrap(), issue_project: "", reviewers: "" };
    state.add_board(&state.store.workspace("app").unwrap(), &board).unwrap();
    let state = Arc::new(state);
    tokio::spawn(axum::serve(tokio::net::TcpListener::from_std(listener).unwrap(), router(state.clone())).into_future());
    World { dir, repo, ws, tmux, state, token }
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

/// Two passes: `pr-status` reads the PR in the first, `pr-review` acts on what it wrote in the
/// second (both start in the same pass, so the first `pr-review` may not see the reading).
async fn settle_pr(w: &World) {
    settle(w).await;
    settle(w).await;
}

fn set_pr(w: &World, state: &str, decision: &str, mergeable: &str, conclusion: &str) {
    let json = serde_json::json!({
        "state": state, "reviewDecision": decision, "mergeable": mergeable,
        "statusCheckRollup": [{ "name": "rspec", "status": "COMPLETED", "conclusion": conclusion }],
    });
    fs::write(w.dir.path().join("pr.json"), json.to_string()).unwrap();
}

fn field(ws: &Path, dir: &str, key: &str) -> String {
    let text = fs::read_to_string(ws.join(dir).join("workitem.md")).unwrap();
    text.lines().find_map(|l| l.strip_prefix(&format!("{key}:"))).unwrap_or("").trim().to_string()
}

/// A session the backend counts as the workspace's, as `launch::start` tags it.
fn own_session(w: &World, name: &str) {
    let tag = w.ws.to_str().unwrap();
    w.tmux.run(&["new-session", "-d", "-s", name, "-c", w.repo.to_str().unwrap(), "sleep 60", ";", "set-option", "@workitem_workspace", tag]);
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

async fn put(w: &World, uri: &str, json: &str) -> (StatusCode, String) {
    let request = Request::put(uri)
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
    write_item(&w.ws, "T-1-flow", "analyze", "");
    fs::write(w.repo.join(".stub-analyze"), "advance").unwrap();
    fs::write(w.ws.join("boards/default/learnings.md"), "# Learnings\n\n- Never mock tmux.\n").unwrap();

    settle(&w).await;
    assert!(w.tmux.has("wi-T-1-flow"), "an analyze item gets a handler session");
    eventually("analyze moved the item on", || field(&w.ws, "T-1-flow", "stage") == "ready_for_development").await;

    // Status is `ok` after the move, so the finished session is reaped; the gate starts nothing.
    settle(&w).await;
    eventually("the finished session is reaped", || !w.tmux.has("wi-T-1-flow")).await;

    // Rejected, it is back in analyze, where the handler starts again and blocks.
    fs::write(w.repo.join(".stub-analyze"), "block").unwrap();
    assert_eq!(post(&w, "/api/workitems/app/T-1-flow/reject", r#"{"comment":"again"}"#).await.0, StatusCode::NO_CONTENT);
    settle(&w).await;
    eventually("analyze handler blocked the item", || field(&w.ws, "T-1-flow", "agent_status").starts_with("blocked")).await;

    settle(&w).await;
    settle(&w).await;
    assert!(w.tmux.has("wi-T-1-flow"), "a blocked session waits for the human");
    assert_eq!(stub_log(&w.repo), vec!["T-1-flow analyze", "T-1-flow analyze"]);
    let args = fs::read_dir(w.dir.path().join("logs")).unwrap().map(|e| fs::read_to_string(e.unwrap().path()).unwrap()).collect::<String>();
    assert!(!args.contains("Never mock tmux"), "stages that do not write code get no learnings: {args}");
}

#[tokio::test]
async fn project_agent_runs_the_handler_instead_of_the_default() {
    let w = world();
    // A wrapper like `yolo`: marks that it ran, then hands over to the stub with the same arguments.
    let agent = w.dir.path().join("my-agent");
    fs::write(&agent, format!("#!/bin/bash\ntouch \"$WORKITEM_REPO/.agent-ran\"\nexec {} \"$@\"\n", w.state.claude)).unwrap();
    fs::set_permissions(&agent, fs::Permissions::from_mode(0o755)).unwrap();

    let (status, _) = put(&w, "/api/workspaces/app", r#"{"agent":"no such agent"}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, body) = put(&w, "/api/workspaces/app", &serde_json::json!({ "agent": agent }).to_string()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(get(&w, "/api/workspaces").await.contains("my-agent"));

    write_item(&w.ws, "T-9-agent", "analyze", "");
    fs::write(w.repo.join(".stub-analyze"), "advance").unwrap();
    settle(&w).await;
    eventually("the handler ran and moved the item", || field(&w.ws, "T-9-agent", "stage") == "ready_for_development").await;
    assert!(w.repo.join(".agent-ran").exists(), "the project's agent ran the handler");

    // Cleared, the next session runs the default again.
    fs::remove_file(w.repo.join(".agent-ran")).unwrap();
    assert_eq!(put(&w, "/api/workspaces/app", r#"{"agent":null}"#).await.0, StatusCode::OK);
    assert_eq!(post(&w, "/api/workitems/app/T-9-agent/reject", r#"{"comment":"again"}"#).await.0, StatusCode::NO_CONTENT);
    settle(&w).await;
    settle(&w).await;
    eventually("analyze ran", || stub_log(&w.repo).len() == 2).await;
    assert!(!w.repo.join(".agent-ran").exists());
}

fn kill_quietly(w: &World, session: &str) {
    let _ = std::process::Command::new("tmux").args(["-L", &w.tmux.0, "kill-session", "-t", &format!("={session}")]).status();
}

#[tokio::test]
async fn the_default_agent_runs_each_kind_with_its_own_arguments_and_glue() {
    let w = world();
    let logs = w.dir.path().join("kind-logs");
    fs::create_dir_all(&logs).unwrap();
    let (status, _) = put(&w, "/api/agent", r#"{"agent":{"kind":"pi","command":"/no/such/pi"}}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let settings: serde_json::Value = serde_json::from_str(&get(&w, "/api/agent").await).unwrap();
    let kinds: Vec<&str> = settings["kinds"].as_array().unwrap().iter().map(|k| k["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["claude", "opencode", "pi", "omp"]);
    assert_eq!(settings["kinds"][0]["installed"], true, "the stub is the default claude");
    fs::create_dir_all(w.ws.join("boards")).unwrap();
    fs::write(w.ws.join("boards/kinds.json"), board(&w, r#"[{"name":"qa","model":"p/m-1"}]"#)).unwrap();
    let token = w.state.store.token();
    for kind in ["opencode", "pi", "omp"] {
        // Records its arguments, one per line, and the env the launcher sets for the kinds.
        let stub = w.dir.path().join(kind);
        let log = logs.join(kind);
        let env = "echo \"config=$OPENCODE_CONFIG_CONTENT\"; echo \"mcp=$WORKITEM_MCP\"; echo \"create=$WORKITEM_CREATE\"";
        fs::write(&stub, format!("#!/bin/bash\n{{ printf '%s\\n' \"$@\"; {env}; }} > {}\n", log.display())).unwrap();
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
        let body = serde_json::json!({ "agent": { "kind": kind, "command": stub } }).to_string();
        let (status, body) = put(&w, "/api/agent", &body).await;
        assert_eq!(status, StatusCode::OK, "{body}");

        let dir = format!("T-9-{kind}");
        write_item(&w.ws, &dir, "qa", "board: kinds\n");
        let (status, body) = post(&w, &format!("/api/workitems/app/{dir}/session"), "").await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        eventually("the agent ran", || log.exists()).await;
        let args = fs::read_to_string(&log).unwrap();
        let lines: Vec<&str> = args.lines().collect();
        let glue = skills().join(format!("workitem-pipeline/agents/{kind}/workitem-notify.ts"));
        assert!(glue.exists(), "{}", glue.display());
        assert!(args.contains(&format!("You are working on workitem {dir}")), "the prompt comes last: {args}");
        assert!(!args.contains("--settings") && !args.contains("--mcp-config"), "{kind} gets no Claude settings: {args}");
        assert!(!args.contains(&token), "the MCP token is never in argv or the env: {args}");
        let ws_dir = format!("--add-dir={}", w.ws.display());
        let value = |args: &str, key: &str| args.lines().find_map(|l| l.strip_prefix(&format!("{key}="))).unwrap().to_string();
        let url = format!("/api/mcp/app/{dir}");
        match kind {
            "opencode" => {
                assert_eq!(lines[..3], ["-m", "p/m-1", "--prompt"], "the stage's model: {args}");
                let config: serde_json::Value = serde_json::from_str(&value(&args, "config")).unwrap();
                assert_eq!(config["plugin"][0], format!("file://{}", glue.display()));
                let rules = config["permission"]["external_directory"].as_object().unwrap();
                assert!(rules.contains_key(&format!("{}/**", w.ws.display())), "the workspace, as Claude's --add-dir: {rules:?}");
                assert!(rules.values().all(|v| v == "allow"), "only allow rules: {rules:?}");
                assert!(config["permission"]["bash"].is_null(), "item sessions keep the user's shell setting");
                let server = &config["mcp"]["workitem"];
                assert!(server["type"] == "remote" && server["url"].as_str().unwrap().ends_with(&url), "{server}");
                let header = server["headers"]["Authorization"].as_str().unwrap();
                let file = header.strip_prefix("Bearer {file:").and_then(|f| f.strip_suffix('}')).unwrap();
                assert_eq!(fs::read_to_string(file).unwrap(), token, "opencode reads the token from its file");
                assert_eq!(fs::metadata(file).unwrap().permissions().mode() & 0o777, 0o600);
            }
            "pi" => assert_eq!(lines[..4], ["-e", glue.to_str().unwrap(), "--model", "p/m-1"], "{args}"),
            _ => assert_eq!(lines[..4], [ws_dir.as_str(), "-e", glue.to_str().unwrap(), "--model=p/m-1"], "{args}"),
        }
        if kind != "opencode" {
            // The glue offers the MCP server's tools from Claude's config file.
            let config = fs::read_to_string(value(&args, "mcp")).unwrap();
            assert!(config.contains(&url) && config.contains(&token), "{config}");
        }

        // A create session gets no stage, so no model and no MCP server, and its glue reports
        // to the session's toast.
        fs::remove_file(&log).unwrap();
        let (status, body) = post(&w, "/api/workspaces/app/create", r#"{"input":"look into x","board":"default"}"#).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        eventually("the agent created", || log.exists()).await;
        let args = fs::read_to_string(&log).unwrap();
        assert!(value(&args, "create").starts_with("wi-new-"), "{args}");
        assert!(value(&args, "mcp").is_empty() && !args.contains("m-1") && !value(&args, "config").contains("\"mcp\""), "{args}");

        // Planning runs the same agent; Resume continues the recorded conversation.
        fs::remove_file(&log).unwrap();
        let (status, body) = post(&w, "/api/workspaces/app/plans", r#"{"input":"plan it"}"#).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"].as_str().unwrap().to_string();
        eventually("the agent planned", || log.exists()).await;
        let args = fs::read_to_string(&log).unwrap();
        assert!(args.contains("This is a planning session"), "{args}");
        if kind == "opencode" {
            // Shell commands ask first, as in Claude's planning sessions; the record's helper does not.
            let config: serde_json::Value = serde_json::from_str(&value(&args, "config")).unwrap();
            let note = skills().join("workitem-pipeline/bin/workitem-plan-note");
            assert_eq!(config["permission"]["bash"], serde_json::json!({ "*": "ask", format!("python3 {}*", note.display()): "allow" }));
        }
        let record = w.ws.join("plans").join(format!("{id}.md"));
        let text = fs::read_to_string(&record).unwrap();
        assert!(text.contains(&format!("agent: {kind}\n")), "{text}");
        let preset = text.lines().find_map(|l| l.strip_prefix("agent_session:")).unwrap().trim().to_string();
        let lines: Vec<&str> = args.lines().collect();
        match kind {
            "opencode" => {
                assert!(preset.is_empty(), "opencode picks its own id");
                assert_eq!(lines[0], "--prompt", "{args}");
            }
            "pi" => assert_eq!(lines[..4], ["-e", glue.to_str().unwrap(), "--session-id", &preset], "{args}"),
            _ => {
                assert!(preset.is_empty(), "omp picks its own id");
                assert!(lines.contains(&"-e") && !lines.iter().any(|l| l.starts_with("--resume")), "{args}");
            }
        }
        // What the glue of opencode and omp records once the CLI made its session.
        let recorded = if preset.is_empty() { format!("{kind}-session") } else { preset.clone() };
        fs::write(&record, text.replace("agent_session: \n", &format!("agent_session: {recorded}\n"))).unwrap();
        kill_quietly(&w, &format!("wi-plan-{id}"));
        fs::remove_file(&log).unwrap();
        let (status, body) = post(&w, &format!("/api/plans/app/{id}/resume"), "").await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        eventually("the agent resumed", || log.exists()).await;
        let args = fs::read_to_string(&log).unwrap();
        let lines: Vec<&str> = args.lines().collect();
        let tail: Vec<String> = match kind {
            "opencode" => vec!["--session".into(), recorded.clone()],
            "pi" => vec!["--session-id".into(), recorded.clone()],
            _ => vec![format!("--resume={recorded}")],
        };
        let body = &lines[..lines.len() - 3]; // without the env lines
        assert_eq!(body[body.len() - tail.len()..], tail, "no prompt on resume: {args}");
        kill_quietly(&w, &format!("wi-plan-{id}"));
    }
}

/// Opt-in, since it runs the real CLIs with the user's own config and costs a model call each:
/// `WORKITEM_LIVE_AGENTS=opencode,pi,omp cargo test --test pipeline live_agents`. An agent session
/// reads the workitem and stops to wait for the human; its glue must then block the item. A
/// planning session must record the agent's session and turn waiting the same way.
#[tokio::test]
async fn live_agents_block_the_item_when_they_stop() {
    let Ok(kinds) = std::env::var("WORKITEM_LIVE_AGENTS") else { return };
    let w = world();
    for kind in kinds.split(',').map(str::trim).filter(|k| !k.is_empty()) {
        let (status, body) = put(&w, "/api/agent", &serde_json::json!({ "agent": { "kind": kind } }).to_string()).await;
        assert_eq!(status, StatusCode::OK, "{kind}: {body}");
        let dir = format!("T-live-{kind}");
        write_item(&w.ws, &dir, "ready_for_development", "");
        let (status, body) = post(&w, &format!("/api/workitems/app/{dir}/session"), "").await;
        assert_eq!(status, StatusCode::ACCEPTED, "{kind}: {body}");
        live_wait(&w, &format!("{kind} never blocked the item"), &format!("wi-{dir}"), || field(&w.ws, &dir, "agent_status").starts_with("blocked")).await;

        let (status, body) = post(&w, "/api/workspaces/app/plans", r#"{"input":"Which folder holds the workitems? Answer in one sentence."}"#).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{kind}: {body}");
        let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"].as_str().unwrap().to_string();
        let record = w.ws.join("plans").join(format!("{id}.md"));
        let value = |key: &str| {
            let text = fs::read_to_string(&record).unwrap_or_default();
            text.lines().find_map(|l| l.strip_prefix(&format!("{key}:"))).unwrap_or("").trim().to_string()
        };
        let what = format!("{kind}'s plan never recorded its session and waited; record {}", record.display());
        live_wait(&w, &what, &format!("wi-plan-{id}"), || value("status") == "waiting" && !value("agent_session").is_empty()).await;
    }
}

/// Waits up to 3 minutes for `done`, then kills `session`; on timeout, fails with its screen.
async fn live_wait(w: &World, what: &str, session: &str, mut done: impl FnMut() -> bool) {
    for _ in 0..180 {
        if done() {
            kill_quietly(w, session);
            return;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    let screen = std::process::Command::new("tmux").args(["-L", &w.tmux.0, "capture-pane", "-p", "-t", &format!("={session}")]).output().unwrap();
    let record = what.rsplit_once("record ").map(|(_, path)| fs::read_to_string(path).unwrap_or_default()).unwrap_or_default();
    panic!("{what}\n{record}\nthe screen:\n{}", String::from_utf8_lossy(&screen.stdout));
}

#[tokio::test]
async fn dispatcher_retries_crashed_handlers_then_blocks_at_the_attempt_cap() {
    let w = world();
    write_item(&w.ws, "T-2-crash", "analyze", "");
    fs::write(w.repo.join(".stub-analyze"), "crash").unwrap();

    for _ in 0..10 {
        settle(&w).await;
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        if field(&w.ws, "T-2-crash", "agent_status").starts_with("blocked") {
            break;
        }
    }
    assert_eq!(field(&w.ws, "T-2-crash", "agent_status"), "blocked: max attempts reached in stage analyze");
    assert_eq!(stub_log(&w.repo).len(), 3);
    settle(&w).await;
    assert_eq!(stub_log(&w.repo).len(), 3, "no more attempts once blocked");
}

#[tokio::test]
async fn dispatcher_starts_development_with_the_start_prompt() {
    let w = world();
    write_item(&w.ws, "T-10-dev", "in_development", "");
    fs::write(w.ws.join("boards/default/learnings.md"), "# Learnings\n\n- Never mock tmux.\n").unwrap();

    settle(&w).await;
    eventually("the start handler logged its invocation", || {
        stub_log(&w.repo) == vec!["T-10-dev in_development"]
    })
    .await;
    let args = fs::read_dir(w.dir.path().join("logs")).unwrap().map(|e| fs::read_to_string(e.unwrap().path()).unwrap()).collect::<String>();
    assert!(args.contains("Current stage: in_development"), "{args}");
    assert!(args.contains("resume it as is"), "{args}");
    assert!(!args.contains("Do not change code"), "{args}");
    assert!(args.contains("- Never mock tmux."), "development gets the learnings: {args}");
}

#[tokio::test]
async fn dispatcher_starts_development_once_a_ready_item_is_approved() {
    let w = world();
    write_item(&w.ws, "T-11-ready", "ready_for_development", "");

    settle(&w).await;
    assert!(stub_log(&w.repo).is_empty(), "an unapproved item waits for the human");

    let (status, _) = post(&w, "/api/workitems/app/T-11-ready/approve", "").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    settle(&w).await;
    assert_eq!(field(&w.ws, "T-11-ready", "stage"), "in_development", "approving moves the item on");
    eventually("the start handler picked up the approved item", || {
        stub_log(&w.repo) == vec!["T-11-ready in_development"]
    })
    .await;
}

#[tokio::test]
async fn dispatcher_ignores_manual_stages_and_a_same_named_session_from_another_workspace() {
    let w = world();
    write_item(&w.ws, "T-3-manual", "ready_for_development", "");
    write_item(&w.ws, "T-4-other", "analyze", "");
    let elsewhere = w.dir.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    w.tmux.run(&["new-session", "-d", "-s", "wi-T-4-other", "sleep 60", ";", "set-option", "@workitem_workspace", elsewhere.to_str().unwrap()]);

    settle(&w).await;
    // The other repo's session is not this item's session, and the name is taken, so no launch.
    assert!(stub_log(&w.repo).is_empty());
    assert_eq!(field(&w.ws, "T-3-manual", "agent_status"), "ok");
    let status = field(&w.ws, "T-4-other", "agent_status");
    assert!(status.starts_with("blocked: cannot start wi-T-4-other: session wi-T-4-other already exists"), "a failed launch says why: {status}");
}

#[tokio::test]
async fn approve_and_reject_edit_the_workitem_through_the_pipeline_rules() {
    let w = world();
    write_item(&w.ws, "T-5-gate", "analyze", "");
    let (status, _) = post(&w, "/api/workitems/app/T-5-gate/approve", "").await;
    assert_eq!(status, StatusCode::CONFLICT, "only ready_for_development can be approved");

    write_item(&w.ws, "T-6-gate", "ready_for_development", "");
    let (status, _) = post(&w, "/api/workitems/app/T-6-gate/approve", "").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(field(&w.ws, "T-6-gate", "approved"), "true");
    assert_eq!(field(&w.ws, "T-6-gate", "approved_at").len(), 10);

    write_item(&w.ws, "T-7-gate", "ready_for_development", "");
    let (status, _) = post(&w, "/api/workitems/app/T-7-gate/reject", r#"{"comment":"  "}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = post(&w, "/api/workitems/app/T-7-gate/reject", r#"{"comment":"needs a rollback plan"}"#).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(field(&w.ws, "T-7-gate", "stage"), "analyze");
    assert_eq!(field(&w.ws, "T-7-gate", "approved"), "false");
    let text = fs::read_to_string(w.ws.join("T-7-gate/workitem.md")).unwrap();
    assert!(text.contains("## Rejection\n\nneeds a rollback plan"));
}

#[tokio::test]
async fn edit_brief_replaces_the_body_only_while_it_is_unchanged() {
    let w = world();
    write_item(&w.ws, "T-30-edit", "analyze", "");
    let file = w.ws.join("T-30-edit/workitem.md");
    let (status, _) = post(&w, "/api/workitems/app/T-30-edit/brief", r#"{"base":"\n# stale\n","body":"x"}"#).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) =
        post(&w, "/api/workitems/app/T-30-edit/brief", r#"{"base":"\n# T-30-edit\n","body":"\n# Edited\n\nstage: done\n"}"#).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.ends_with("---\n\n# Edited\n\nstage: done\n"));
    assert_eq!(field(&w.ws, "T-30-edit", "stage"), "analyze", "the body cannot change the frontmatter");
}

#[tokio::test]
async fn edit_frontmatter_sets_existing_fields_but_never_stage() {
    let w = world();
    write_item(&w.ws, "T-31-info", "analyze", "");
    let url = "/api/workitems/app/T-31-info/frontmatter";
    let (status, _) = post(&w, url, r#"{"fields":{"stage":"done"}}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = post(&w, url, r#"{"fields":{"nope":"x"}}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = post(&w, url, r#"{"fields":{"title":"a\nstage: done"}}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = post(&w, url, r#"{"fields":{"title":"New title"}}"#).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(field(&w.ws, "T-31-info", "title"), "New title");
    assert_eq!(field(&w.ws, "T-31-info", "stage"), "analyze");
}

#[tokio::test]
async fn dispatcher_keeps_a_waiting_session_and_frees_the_item_once_it_is_gone() {
    let w = world();
    write_item(&w.ws, "T-40-wait", "analyze", "");
    fs::write(w.repo.join(".stub-analyze"), "wait").unwrap();
    settle(&w).await;
    eventually("the handler waits", || field(&w.ws, "T-40-wait", "agent_status") == "waiting: CI").await;

    settle(&w).await;
    assert!(w.tmux.has("wi-T-40-wait"), "a waiting session is kept");
    assert_eq!(field(&w.ws, "T-40-wait", "agent_status"), "waiting: CI");

    w.tmux.run(&["kill-session", "-t", "=wi-T-40-wait"]);
    fs::remove_file(w.repo.join(".stub-analyze")).unwrap();
    settle(&w).await;
    eventually("the item is idle and starts again", || stub_log(&w.repo).len() == 2).await;
}

#[tokio::test]
async fn clear_blocked_sets_working_when_an_agent_is_still_waiting() {
    let w = world();
    write_item(&w.ws, "T-8-idle", "analyze", "");
    let (status, _) = post(&w, "/api/workitems/app/T-8-idle/clear-blocked", "").await;
    assert_eq!(status, StatusCode::CONFLICT, "not blocked");

    let blocked = fs::read_to_string(w.ws.join("T-8-idle/workitem.md")).unwrap().replace("agent_status: ok", "agent_status: blocked: x");
    fs::write(w.ws.join("T-8-idle/workitem.md"), &blocked).unwrap();
    let (status, _) = post(&w, "/api/workitems/app/T-8-idle/clear-blocked", "").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(field(&w.ws, "T-8-idle", "agent_status"), "ok");

    fs::write(w.ws.join("T-8-idle/workitem.md"), &blocked).unwrap();
    own_session(&w, "wi-T-8-idle");
    post(&w, "/api/workitems/app/T-8-idle/clear-blocked", "").await;
    assert_eq!(field(&w.ws, "T-8-idle", "agent_status"), "working");

    let (status, _) = post(&w, "/api/workitems/app/T-8-idle/kill-session", "").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(!w.tmux.has("wi-T-8-idle"));
    assert_eq!(field(&w.ws, "T-8-idle", "agent_status"), "ok");
}

#[tokio::test]
async fn agent_sessions_start_from_the_ui_and_pass_text_as_one_argument() {
    let w = world();
    write_item(&w.ws, "T-9-start", "ready_for_development", "");

    let (status, _) = post(&w, "/api/workitems/app/T-9-start/start", "").await;
    assert_eq!(status, StatusCode::CONFLICT, "not approved");
    post(&w, "/api/workitems/app/T-9-start/approve", "").await;
    let (status, _) = post(&w, "/api/workitems/app/T-9-start/start", "").await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(w.tmux.has("wi-T-9-start"));
    assert_eq!(field(&w.ws, "T-9-start", "agent_status"), "working");
    let (status, body) = post(&w, "/api/workitems/app/T-9-start/start", "").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body.contains("already has a session"));

    let pwn = w.dir.path().join("pwned");
    let input = format!("it's \"quoted\" $(touch {}) `touch {}`", pwn.display(), pwn.display());
    let (status, body) = post(&w, "/api/workspaces/app/create", &serde_json::json!({ "input": input }).to_string()).await;
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
    assert!(create_log.starts_with("argc=5\n"), "--add-dir with the repo, settings flag, path and prompt: {create_log}");
    assert!(create_log.contains(&input), "prompt keeps the text verbatim");
    assert!(create_log.contains(&format!("created_by: {session}")), "the agent links what it writes to its session: {create_log}");
    assert!(!pwn.exists(), "text must never reach a shell");
}

#[tokio::test]
async fn create_sessions_report_whether_they_wait_for_input_or_died() {
    let w = world();
    let ws = w.ws.display().to_string();
    for (name, command) in [("wi-new-wait", "sleep 60"), ("wi-new-dead", "true")] {
        w.tmux.run(&["new-session", "-d", "-s", name, command, ";", "set-option", "-t", name, "@workitem_workspace", &ws, ";", "set-option", "-t", name, "remain-on-exit", "on"]);
    }
    w.tmux.run(&["set-option", "-t", "=wi-new-wait:", "@workitem_waiting", "1"]);
    eventually("the dead pane exited", || {
        let out = std::process::Command::new("tmux").args(["-L", &w.tmux.0, "list-panes", "-t", "=wi-new-dead", "-F", "#{pane_dead}"]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim() == "1"
    })
    .await;
    let sessions: serde_json::Value = serde_json::from_str(&get(&w, "/api/sessions").await).unwrap();
    let of = |name: &str| sessions.as_array().unwrap().iter().find(|s| s["name"] == name).cloned().unwrap();
    assert_eq!((of("wi-new-wait")["waiting"].as_bool(), of("wi-new-wait")["dead"].as_bool()), (Some(true), Some(false)));
    assert_eq!((of("wi-new-dead")["waiting"].as_bool(), of("wi-new-dead")["dead"].as_bool()), (Some(false), Some(true)));
}

async fn kill_session(w: &World, uri: &str) -> StatusCode {
    let request = Request::delete(uri)
        .header(header::HOST, format!("127.0.0.1:{}", w.state.port))
        .header(header::COOKIE, format!("wi_session={}", w.token))
        .body(Body::empty())
        .unwrap();
    router(w.state.clone()).oneshot(request).await.unwrap().status()
}

#[tokio::test]
async fn only_a_dead_create_session_of_the_workspace_can_be_killed() {
    let w = world();
    let ws = w.ws.display().to_string();
    let other = w.dir.path().join("elsewhere").display().to_string();
    for (name, command, folder) in [("wi-new-wait", "sleep 60", &ws), ("wi-new-dead", "true", &ws), ("wi-new-other", "true", &other), ("wi-T-1-x", "true", &ws)] {
        w.tmux.run(&["new-session", "-d", "-s", name, command, ";", "set-option", "-t", name, "@workitem_workspace", folder, ";", "set-option", "-t", name, "remain-on-exit", "on"]);
    }
    eventually("the dead panes exited", || {
        let out = std::process::Command::new("tmux").args(["-L", &w.tmux.0, "list-panes", "-a", "-F", "#{pane_dead}"]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).lines().filter(|l| *l == "1").count() == 3
    })
    .await;

    assert_eq!(kill_session(&w, "/api/sessions/app/wi-T-1-x").await, StatusCode::BAD_REQUEST);
    assert_eq!(kill_session(&w, "/api/sessions/app/wi-new-other").await, StatusCode::NOT_FOUND);
    assert_eq!(kill_session(&w, "/api/sessions/app/wi-new-wait").await, StatusCode::CONFLICT);
    assert_eq!(kill_session(&w, "/api/sessions/nope/wi-new-dead").await, StatusCode::NOT_FOUND);
    for name in ["wi-T-1-x", "wi-new-other", "wi-new-wait", "wi-new-dead"] {
        assert!(w.tmux.has(name), "{name} survives a refused kill");
    }

    assert_eq!(kill_session(&w, "/api/sessions/app/wi-new-dead").await, StatusCode::NO_CONTENT);
    assert!(!w.tmux.has("wi-new-dead"));
    assert!(!get(&w, "/api/sessions").await.contains("wi-new-dead"));
}

#[tokio::test]
async fn a_manual_session_starts_in_any_stage_and_survives_the_dispatcher() {
    let w = world();
    write_item(&w.ws, "T-12-chat", "analyze", "");
    let (status, _) = post(&w, "/api/workitems/app/T-12-chat/session", "").await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(w.tmux.has("wi-T-12-chat"));
    assert_eq!(field(&w.ws, "T-12-chat", "agent_status"), "working");
    settle(&w).await;
    assert!(w.tmux.has("wi-T-12-chat"), "a working session is not reaped");

    let (status, body) = post(&w, "/api/workitems/app/T-12-chat/session", "").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body.contains("already has a session"));
    assert_eq!(field(&w.ws, "T-12-chat", "agent_status"), "working", "a refused launch leaves the status alone");

    write_item(&w.ws, "T-13-stuck", "analyze", "");
    let stuck = fs::read_to_string(w.ws.join("T-13-stuck/workitem.md")).unwrap().replace("agent_status: ok", "agent_status: blocked: x");
    fs::write(w.ws.join("T-13-stuck/workitem.md"), stuck).unwrap();
    let (status, _) = post(&w, "/api/workitems/app/T-13-stuck/session", "").await;
    assert_eq!(status, StatusCode::ACCEPTED, "a human may start a session to resolve the block");
    assert_eq!(field(&w.ws, "T-13-stuck", "agent_status"), "working");
    let logs = w.dir.path().join("logs");
    eventually("stub recorded the block in the session prompt", || {
        fs::read_dir(&logs).unwrap().filter_map(Result::ok).any(|e| fs::read_to_string(e.path()).is_ok_and(|t| t.contains("T-13-stuck") && t.contains("was `blocked: x` when the human started")))
    })
    .await;
}

#[tokio::test]
async fn discuss_brief_opens_a_session_on_the_brief() {
    let w = world();
    write_item(&w.ws, "T-14-talk", "ready_for_development", "");
    let (status, _) = post(&w, "/api/workitems/app/T-14-talk/discuss", "").await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(w.tmux.has("wi-T-14-talk"));
    assert_eq!(field(&w.ws, "T-14-talk", "agent_status"), "working");

    let logs = w.dir.path().join("logs");
    eventually("stub recorded the brief prompt", || {
        fs::read_dir(&logs).unwrap().filter_map(Result::ok).any(|e| fs::read_to_string(e.path()).is_ok_and(|t| t.contains("discussing the brief")))
    })
    .await;

    let (status, body) = post(&w, "/api/workitems/app/T-14-talk/discuss", "").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body.contains("already has a session"));
}

#[tokio::test]
async fn a_session_starts_in_the_worktree_and_still_belongs_to_the_workspace() {
    let w = world();
    write_item(&w.ws, "T-15-wt", "analyze", "");
    let git = |args: &[&str], cwd: &Path| {
        let ok = std::process::Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).current_dir(cwd).output().unwrap().status.success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q"], &w.repo);
    git(&["commit", "-q", "--allow-empty", "-m", "i"], &w.repo);
    let worktree = w.ws.join(".worktrees/repo-T-15-wt");
    git(&["worktree", "add", "-q", "-b", "wt", worktree.to_str().unwrap()], &w.repo);
    let worktree = worktree.canonicalize().unwrap();
    let items: serde_json::Value = serde_json::from_str(&get(&w, "/api/workitems").await).unwrap();
    assert_eq!(items.as_array().unwrap().len(), 1, ".worktrees is not an item");

    let (status, _) = post(&w, "/api/workitems/app/T-15-wt/session", "").await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let out = std::process::Command::new("tmux").args(["-L", &w.tmux.0, "list-sessions", "-F", "#{session_path}"]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), worktree.to_str().unwrap());
    let cwd = w.dir.path().join("cwd-T-15-wt");
    eventually("the agent ran", || cwd.exists()).await;
    assert!(fs::read_to_string(&cwd).unwrap().trim_end().ends_with(&format!("|{}", worktree.display())), "WORKITEM_WORKTREE names it");

    let logs = w.dir.path().join("logs");
    eventually("stub ran with the workspace and the repo added", || {
        fs::read_dir(&logs).unwrap().filter_map(Result::ok).any(|e| fs::read_to_string(e.path()).is_ok_and(|t| t.starts_with("argc=8\n")))
    })
    .await;

    let items: serde_json::Value = serde_json::from_str(&get(&w, "/api/workitems").await).unwrap();
    assert_eq!(items[0]["session"], true, "the session is the item's although it runs in the worktree");
    settle(&w).await;
    assert_eq!(field(&w.ws, "T-15-wt", "agent_status"), "working", "not taken for a working item without a session");
    assert_eq!(w.tmux.session_names(), vec!["wi-T-15-wt"]);
}

#[tokio::test]
async fn items_report_branch_and_unpushed_commits_of_their_worktree() {
    let w = world();
    write_item(&w.ws, "T-12-wt", "in_development", "");
    write_item(&w.ws, "T-13-none", "in_development", "");
    let git = |args: &[&str], cwd: &Path| {
        let ok = std::process::Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).current_dir(cwd).output().unwrap().status.success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q"], &w.repo);
    git(&["commit", "-q", "--allow-empty", "-m", "i"], &w.repo);
    let worktree = w.repo.parent().unwrap().join("repo-T-12-wt");
    git(&["worktree", "add", "-q", "-b", "wt", worktree.to_str().unwrap()], &w.repo);
    git(&["commit", "-q", "--allow-empty", "-m", "a"], &worktree);

    async fn field_of(w: &World, dir: &str, name: &str) -> serde_json::Value {
        let items: serde_json::Value = serde_json::from_str(&get(w, "/api/workitems").await).unwrap();
        items.as_array().unwrap().iter().find(|i| i["dir"] == dir).unwrap()[name].clone()
    }
    async fn unpushed(w: &World, dir: &str) -> serde_json::Value {
        field_of(w, dir, "unpushed").await
    }
    assert_eq!(unpushed(&w, "T-12-wt").await, 2, "never pushed: every commit counts");
    assert!(unpushed(&w, "T-13-none").await.is_null(), "no worktree, nothing to report");
    assert_eq!(field_of(&w, "T-12-wt", "branch").await, "wt");
    assert!(field_of(&w, "T-13-none", "branch").await.is_null(), "no worktree, no branch");

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

    // An upstream on another branch (a worktree added from origin/<base>) is not where the branch is pushed.
    git(&["push", "-q", "origin", "HEAD~2:refs/heads/base"], &worktree);
    git(&["fetch", "-q", "origin"], &worktree);
    git(&["branch", "-q", "--set-upstream-to", "origin/base"], &worktree);
    assert_eq!(unpushed(&w, "T-12-wt").await, 0, "upstream is the base branch, origin/wt has every commit");
}

#[tokio::test]
async fn remove_is_finished_when_the_request_returns() {
    let w = world();
    write_item(&w.ws, "T-11-busy", "analyze", "");
    own_session(&w, "wi-T-11-busy");
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
    assert!(!w.ws.join("T-11-busy").exists(), "folder removed");

    // Nothing is left for the dispatcher to pick up.
    settle(&w).await;
    assert!(stub_log(&w.repo).is_empty());
    assert!(!w.tmux.has("wi-T-11-busy"));
}

async fn delete_board(w: &World, board: &str) -> (StatusCode, String) {
    let request = Request::delete(format!("/api/workspaces/app/boards/{board}"))
        .header(header::HOST, format!("127.0.0.1:{}", w.state.port))
        .header(header::COOKIE, format!("wi_session={}", w.token))
        .body(Body::empty())
        .unwrap();
    let response = router(w.state.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    (status, String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes()).into_owned())
}

#[tokio::test]
async fn deleting_a_board_removes_its_items_and_files() {
    let w = world();
    fs::copy(w.ws.join("boards/default.json"), w.ws.join("boards/side.json")).unwrap();
    fs::create_dir_all(w.ws.join("boards/side/skills")).unwrap();
    fs::write(w.ws.join("boards/side/learnings.md"), "# Learnings\n").unwrap();
    write_item(&w.ws, "T-12-keep", "analyze", "");
    write_item(&w.ws, "T-13-side", "analyze", "board: side\n");
    write_item(&w.ws, "T-14-side", "ready_for_development", "board: side\n");
    own_session(&w, "wi-T-13-side");
    let workspace = w.state.store.workspace("app").unwrap();
    workitem_ui::watch::set_paused(&w.state, &workspace, "side", "analyze", true).unwrap();
    workitem_ui::watch::set_paused(&w.state, &workspace, "default", "analyze", true).unwrap();
    fs::write(w.state.store.sibling("watch.json"), r#"{"app/side/analyze/0": 1, "app/default/analyze/0": 1}"#).unwrap();
    assert!(get(&w, "/api/pipelines").await.contains(r#""name":"side""#));

    assert_eq!(delete_board(&w, "nope").await.0, StatusCode::NOT_FOUND);
    assert_eq!(delete_board(&w, "..").await.0, StatusCode::NOT_FOUND);

    let (status, body) = delete_board(&w, "side").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, r#"{"removed":2,"worktrees_left":[]}"#);
    assert!(!w.tmux.has("wi-T-13-side"), "session stopped");
    assert!(!w.ws.join("T-13-side").exists() && !w.ws.join("T-14-side").exists(), "items removed");
    assert!(!w.ws.join("boards/side.json").exists() && !w.ws.join("boards/side").exists(), "board removed");
    assert!(w.ws.join("T-12-keep").exists() && w.ws.join("boards/default.json").exists(), "other board kept");
    assert!(!get(&w, "/api/pipelines").await.contains(r#""name":"side""#), "not served from the cache");
    let paused = fs::read_to_string(w.state.store.sibling("paused.json")).unwrap();
    assert!(!paused.contains("app/side/") && paused.contains("app/default/analyze"), "{paused}");
    let runs = fs::read_to_string(w.state.store.sibling("watch.json")).unwrap();
    assert!(!runs.contains("app/side/") && runs.contains("app/default/analyze/0"), "{runs}");
}

const PR: &str = "pr: https://github.com/app/pull/7\n";

#[tokio::test]
async fn review_shows_pr_status_and_leaves_green_ci_alone() {
    let w = world();
    write_item(&w.ws, "T-20-green", "in_review", PR);
    set_pr(&w, "OPEN", "APPROVED", "MERGEABLE", "SUCCESS");

    settle_pr(&w).await;
    let items = get(&w, "/api/workitems").await;
    assert!(items.contains(r#""review":{"state":"open","approval":"approved","approved_by_me":false,"merge":"mergeable","ci":"passing","failed":[]}"#), "{items}");
    assert!(stub_log(&w.repo).is_empty() && !w.tmux.has("wi-T-20-green"), "nothing to fix");
}

#[tokio::test]
async fn merge_button_merges_in_review_items_and_the_next_pass_moves_them_to_done() {
    let w = world();
    write_item(&w.ws, "T-23-early", "in_development", PR);
    let (status, _) = post(&w, "/api/workitems/app/T-23-early/merge", "").await;
    assert_eq!(status, StatusCode::CONFLICT, "only in_review can be merged");

    write_item(&w.ws, "T-24-merge", "in_review", PR);
    set_pr(&w, "OPEN", "APPROVED", "MERGEABLE", "SUCCESS");
    settle_pr(&w).await;
    set_pr(&w, "MERGED", "APPROVED", "MERGEABLE", "SUCCESS");
    let (status, _) = post(&w, "/api/workitems/app/T-24-merge/merge", "").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(field(&w.ws, "T-24-merge", "stage"), "in_review", "the dispatcher moves it, not the button");
    settle_pr(&w).await;
    assert_eq!(field(&w.ws, "T-24-merge", "stage"), "done");
}

#[tokio::test]
async fn rebase_button_only_works_on_in_review_items_with_a_pr() {
    let w = world();
    write_item(&w.ws, "T-25-early", "in_development", PR);
    let (status, _) = post(&w, "/api/workitems/app/T-25-early/rebase", "").await;
    assert_eq!(status, StatusCode::CONFLICT);

    write_item(&w.ws, "T-26-rebase", "in_review", PR);
    set_pr(&w, "OPEN", "APPROVED", "MERGEABLE", "SUCCESS");
    let (status, _) = post(&w, "/api/workitems/app/T-26-rebase/rebase", "").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn review_starts_a_fixer_on_red_ci_and_leaves_a_blocked_one_waiting() {
    let w = world();
    write_item(&w.ws, "T-21-red", "in_review", PR);
    set_pr(&w, "OPEN", "REVIEW_REQUIRED", "MERGEABLE", "FAILURE");
    fs::write(w.repo.join(".stub-in_review"), "block").unwrap();

    settle_pr(&w).await;
    eventually("fixer could not fix it and blocked", || field(&w.ws, "T-21-red", "agent_status").starts_with("blocked")).await;
    let items = get(&w, "/api/workitems").await;
    assert!(items.contains(r#""approval":"review_required","approved_by_me":false,"merge":"mergeable","ci":"failing""#), "{items}");
    let prompts = fs::read_dir(w.dir.path().join("logs")).unwrap().map(|e| fs::read_to_string(e.unwrap().path()).unwrap()).collect::<String>();
    assert!(prompts.contains("CI is failing on https://github.com/app/pull/7"), "{prompts}");
    assert!(prompts.contains("gh run view <run> --log-failed"), "the bundled workitem-fix-ci steps: {prompts}");

    settle_pr(&w).await;
    settle_pr(&w).await;
    assert!(w.tmux.has("wi-T-21-red"), "a blocked fixer waits for the human");
    assert_eq!(stub_log(&w.repo).len(), 1, "no second fixer while blocked");
}

#[tokio::test]
async fn review_starts_one_fixer_per_changes_requested_review() {
    let w = world();
    write_item(&w.ws, "T-25-changes", "in_review", PR);
    let set_reviews = |ids: &[&str]| {
        let reviews: Vec<_> = ids.iter().map(|id| serde_json::json!({ "id": id, "state": "CHANGES_REQUESTED" })).collect();
        let json = serde_json::json!({
            "state": "OPEN", "reviewDecision": if ids.is_empty() { "" } else { "CHANGES_REQUESTED" }, "mergeable": "MERGEABLE",
            "statusCheckRollup": [{ "name": "rspec", "status": "COMPLETED", "conclusion": "SUCCESS" }],
            "reviews": reviews,
        });
        fs::write(w.dir.path().join("pr.json"), json.to_string()).unwrap();
    };
    set_reviews(&[]);
    fs::write(w.repo.join(".stub-in_review"), "done").unwrap();
    settle_pr(&w).await;

    set_reviews(&["r1"]);
    settle_pr(&w).await;
    eventually("fixer ran", || stub_log(&w.repo).len() == 1).await;
    let prompts = fs::read_dir(w.dir.path().join("logs")).unwrap().map(|e| fs::read_to_string(e.unwrap().path()).unwrap()).collect::<String>();
    assert!(prompts.contains("a review requesting changes on https://github.com/app/pull/7"), "the comments fixer takes it: {prompts}");

    for _ in 0..3 {
        settle_pr(&w).await;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
    assert_eq!(stub_log(&w.repo).len(), 1, "the same review starts no second fixer");

    set_reviews(&["r1", "r2"]);
    settle_pr(&w).await;
    eventually("a new review starts a new fixer", || stub_log(&w.repo).len() == 2).await;
}

#[tokio::test]
async fn review_starts_one_fixer_per_batch_of_new_comments() {
    let w = world();
    write_item(&w.ws, "T-27-comments", "in_review", PR);
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
            settle_pr(&w).await;
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    };

    // Comments that were already on the PR when the item was first seen are not new.
    let old = inline("alice", "rename this", "2026-01-01T10:00:00Z");
    set_comments(serde_json::json!([]), serde_json::json!([old.clone()]));
    passes().await;
    assert_eq!(stub_log(&w.repo).len(), 0, "existing comments start no fixer");

    set_comments(serde_json::json!([]), serde_json::json!([old.clone(), inline("alice", "extract a method", "2026-02-01T10:00:00Z")]));
    settle_pr(&w).await;
    eventually("a new inline comment starts a fixer", || stub_log(&w.repo).len() == 1).await;
    let prompts = fs::read_dir(w.dir.path().join("logs")).unwrap().map(|e| fs::read_to_string(e.unwrap().path()).unwrap()).collect::<String>();
    assert!(prompts.contains("New review comments or a review requesting changes on https://github.com/app/pull/7"), "{prompts}");
    assert!(prompts.contains("Unsure whether a comment or suggestion is right"), "the fixer asks instead of guessing: {prompts}");
    passes().await;
    assert_eq!(stub_log(&w.repo).len(), 1, "the same comment starts no second fixer");

    // The reader's own replies ask for nothing.
    let mut reply = inline("me", "will do", "2026-02-02T10:00:00Z");
    reply["in_reply_to_id"] = serde_json::json!(1);
    let seen = [old.clone(), inline("alice", "extract a method", "2026-02-01T10:00:00Z"), reply];
    set_comments(serde_json::json!([]), serde_json::json!(seen));
    passes().await;
    assert_eq!(stub_log(&w.repo).len(), 1, "the reader's own reply starts no fixer");

    // Any PR comment by someone else does, a bot's included.
    let bot = plain("github-actions[bot]", "Verdict: Requires Additional Work", "2026-03-01T10:00:00Z");
    set_comments(serde_json::json!([bot.clone()]), serde_json::json!(seen));
    settle_pr(&w).await;
    eventually("a bot's PR comment starts a fixer", || stub_log(&w.repo).len() == 2).await;

    // The reader's own new inline thread asks for a change, with no marker.
    let mut own = seen.to_vec();
    own.push(inline("me", "why not remove this method?", "2026-04-01T10:00:00Z"));
    set_comments(serde_json::json!([bot]), serde_json::json!(own));
    settle_pr(&w).await;
    eventually("the reader's own inline thread starts a fixer", || stub_log(&w.repo).len() == 3).await;
}

#[tokio::test]
async fn review_retries_the_conflict_fixer_up_to_the_cap_per_pr_head() {
    let w = world();
    write_item(&w.ws, "T-26-conflict", "in_review", PR);
    let set_head = |sha: &str| {
        let json = serde_json::json!({
            "state": "OPEN", "reviewDecision": "", "mergeable": "CONFLICTING", "headRefOid": sha,
            "statusCheckRollup": [{ "name": "rspec", "status": "COMPLETED", "conclusion": "SUCCESS" }],
        });
        fs::write(w.dir.path().join("pr.json"), json.to_string()).unwrap();
    };
    set_head("aaa");
    fs::write(w.repo.join(".stub-in_review"), "done").unwrap();

    settle_pr(&w).await;
    eventually("conflict fixer ran", || stub_log(&w.repo).len() == 1).await;
    let prompts = fs::read_dir(w.dir.path().join("logs")).unwrap().map(|e| fs::read_to_string(e.unwrap().path()).unwrap()).collect::<String>();
    assert!(prompts.contains("conflicts with its base branch"), "{prompts}");

    // The fixer said ok without a push: it runs again on the same head, up to the cap (3), and
    // then the item is blocked, so it never sits on ok with the conflict unseen.
    for _ in 0..20 {
        if field(&w.ws, "T-26-conflict", "agent_status").starts_with("blocked") {
            break;
        }
        settle_pr(&w).await;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
    assert_eq!(stub_log(&w.repo).len(), 3, "the same head gets at most three fixers");
    assert!(field(&w.ws, "T-26-conflict", "agent_status").starts_with("blocked: merge conflict still open after 3"));
    assert_eq!(field(&w.ws, "T-26-conflict", "pr_conflict_head"), "", "the head is forgotten while blocked");
    assert_eq!(field(&w.ws, "T-26-conflict", "pr_conflict_attempts"), "", "the attempts are forgotten while blocked");

    // Clearing the block tries again on the same head.
    set_ok(&w.ws, "T-26-conflict");
    settle_pr(&w).await;
    eventually("clearing the block retries the fixer", || stub_log(&w.repo).len() == 4).await;
}

#[tokio::test]
async fn review_forgets_the_conflict_head_while_the_fixer_is_blocked() {
    let w = world();
    write_item(&w.ws, "T-33-conflict-blocked", "in_review", PR);
    let json = serde_json::json!({ "state": "OPEN", "reviewDecision": "", "mergeable": "CONFLICTING", "headRefOid": "aaa", "statusCheckRollup": [] });
    fs::write(w.dir.path().join("pr.json"), json.to_string()).unwrap();
    fs::write(w.repo.join(".stub-in_review"), "block").unwrap();

    settle_pr(&w).await;
    eventually("the fixer blocked", || field(&w.ws, "T-33-conflict-blocked", "agent_status").starts_with("blocked")).await;
    settle_pr(&w).await;
    eventually("the head is forgotten", || field(&w.ws, "T-33-conflict-blocked", "pr_conflict_head").is_empty()).await;
    assert_eq!(stub_log(&w.repo).len(), 1, "a blocked item starts nothing");
}

/// What clearing a block in the UI leaves behind once the session is gone.
fn set_ok(ws: &Path, dir: &str) {
    let file = ws.join(dir).join("workitem.md");
    let text = fs::read_to_string(&file).unwrap();
    let text = text.lines().map(|l| if l.starts_with("agent_status:") { "agent_status: ok" } else { l }).collect::<Vec<_>>().join("\n");
    fs::write(&file, text + "\n").unwrap();
}

#[tokio::test]
async fn review_fixes_the_conflict_before_cancelled_ci() {
    let w = world();
    // The CI attempts of an earlier red streak, as the watcher keeps them in the item.
    write_item(&w.ws, "T-31-conflict-ci", "in_review", &format!("{PR}pr_fix_attempts: 3\n"));
    let json = serde_json::json!({
        "state": "OPEN", "reviewDecision": "", "mergeable": "CONFLICTING", "headRefOid": "aaa",
        "statusCheckRollup": [{ "name": "rspec", "status": "COMPLETED", "conclusion": "CANCELLED" }],
    });
    fs::write(w.dir.path().join("pr.json"), json.to_string()).unwrap();
    fs::write(w.repo.join(".stub-in_review"), "done").unwrap();

    settle_pr(&w).await;
    eventually("a fixer ran", || stub_log(&w.repo).len() == 1).await;
    let prompts = fs::read_dir(w.dir.path().join("logs")).unwrap().map(|e| fs::read_to_string(e.unwrap().path()).unwrap()).collect::<String>();
    assert!(prompts.contains("conflicts with its base branch"), "the conflict fixer goes first: {prompts}");
    assert_eq!(field(&w.ws, "T-31-conflict-ci", "pr_fix_attempts"), "", "the conflict fixer resets the CI attempts");
    assert_eq!(field(&w.ws, "T-31-conflict-ci", "pr_conflict_head"), "aaa", "the head it handled is kept in the item");
}

#[tokio::test]
async fn review_asks_again_while_github_computes_mergeability() {
    let w = world();
    write_item(&w.ws, "T-32-unknown", "in_review", PR);
    let settled = serde_json::json!({ "state": "OPEN", "reviewDecision": "", "mergeable": "CONFLICTING", "headRefOid": "aaa", "statusCheckRollup": [] });
    fs::write(w.dir.path().join("pr.json"), settled.to_string()).unwrap();
    let computing = serde_json::json!({ "state": "OPEN", "reviewDecision": "", "mergeable": "UNKNOWN", "headRefOid": "aaa", "statusCheckRollup": [] });
    fs::write(w.dir.path().join("pr-once.json"), computing.to_string()).unwrap();
    fs::write(w.repo.join(".stub-in_review"), "done").unwrap();

    settle_pr(&w).await;
    eventually("the conflict is seen on the first pass", || stub_log(&w.repo).len() == 1).await;
}

#[tokio::test]
async fn review_blocks_after_the_fix_attempt_cap() {
    let w = world();
    write_item(&w.ws, "T-22-cap", "in_review", PR);
    set_pr(&w, "OPEN", "", "MERGEABLE", "FAILURE");
    fs::write(w.repo.join(".stub-in_review"), "crash").unwrap();

    for _ in 0..30 {
        settle_pr(&w).await;
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        if field(&w.ws, "T-22-cap", "agent_status").starts_with("blocked") {
            break;
        }
    }
    assert_eq!(field(&w.ws, "T-22-cap", "agent_status"), "blocked: CI still failing after 3 fix attempts");
    assert_eq!(stub_log(&w.repo).len(), 3);
}

#[tokio::test]
async fn review_waits_for_a_fresh_reading_and_blocks_a_ci_fixer_that_pushed_nothing() {
    let w = world();
    write_item(&w.ws, "T-34-no-push", "in_review", PR);
    let json = serde_json::json!({
        "state": "OPEN", "reviewDecision": "", "mergeable": "MERGEABLE", "headRefOid": "aaa",
        "statusCheckRollup": [{ "name": "rspec", "status": "COMPLETED", "conclusion": "FAILURE" }],
    });
    fs::write(w.dir.path().join("pr.json"), json.to_string()).unwrap();
    // The fixer says ok without pushing, so the reading before and after it look the same.
    fs::write(w.repo.join(".stub-in_review"), "done").unwrap();

    settle_pr(&w).await;
    eventually("the CI fixer ran", || stub_log(&w.repo).len() == 1).await;
    for _ in 0..4 {
        settle_pr(&w).await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(stub_log(&w.repo).len(), 1, "no second fixer on the reading from before the first one ended");
    assert!(field(&w.ws, "T-34-no-push", "agent_status").starts_with("blocked: CI still failing and the CI fixer pushed nothing"));
}

#[tokio::test]
async fn review_moves_a_merged_pr_to_done_and_blocks_a_closed_one() {
    let w = world();
    write_item(&w.ws, "T-23-merged", "in_review", PR);
    set_pr(&w, "MERGED", "APPROVED", "UNKNOWN", "SUCCESS");
    settle_pr(&w).await;
    assert_eq!(field(&w.ws, "T-23-merged", "stage"), "done");

    write_item(&w.ws, "T-24-closed", "in_review", PR);
    set_pr(&w, "CLOSED", "", "UNKNOWN", "SUCCESS");
    settle_pr(&w).await;
    assert_eq!(field(&w.ws, "T-24-closed", "stage"), "in_review");
    assert_eq!(field(&w.ws, "T-24-closed", "agent_status"), "blocked: PR was closed without merge");
}

#[tokio::test]
async fn pr_tab_gets_description_and_every_kind_of_comment_newest_first() {
    let w = world();
    write_item(&w.ws, "T-30-conversation", "in_review", PR);
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

fn board(w: &World, stages: &str) -> String {
    board_file(w, &format!(r#"{{ "brief_sections": [], "entry": "qa", "stages": {stages} }}"#))
}

#[tokio::test]
async fn project_board_decides_which_stages_the_dispatcher_runs() {
    let w = world();
    write_item(&w.ws, "T-20-qa", "qa", "");
    write_item(&w.ws, "T-21-off", "analyze", "");
    let file = w.ws.join("boards/default.json");
    fs::write(&file, board(&w, r#"[{"name":"qa","handler":"my-qa","auto":true,"next":["shipped"]},{"name":"shipped"}]"#)).unwrap();

    write_skill(&w.ws.join("skills"), "my-qa", "QA STEPS\n");

    settle(&w).await;
    eventually("the project's own handler ran", || stub_log(&w.repo) == vec!["T-20-qa qa"]).await;
    assert!(!w.tmux.has("wi-T-21-off"), "a stage that is not on the board has no handler");
    let args = args_log(&w);
    assert!(args.contains("QA STEPS"), "{args}");
    assert!(args.contains("Current stage: qa"), "{args}");
    assert!(args.contains("Allowed next stages: shipped (default)"), "{args}");
}

#[tokio::test]
async fn state_is_the_five_board_endpoints_in_one_response() {
    let w = world();
    write_item(&w.ws, "T-1-live", "analyze", "");
    write_item(&w.ws, "T-2-idle", "ready_for_development", "");
    fs::create_dir_all(w.ws.join("plans")).unwrap();
    fs::write(w.ws.join("plans/abc1.md"), "---\ntopic: Split auth\nstatus: working\ncreated: 1\n---\n").unwrap();
    for name in ["wi-T-1-live", "wi-new-a", "wi-plan-abc1"] {
        own_session(&w, name);
    }

    let json = |body: String| serde_json::from_str::<serde_json::Value>(&body).unwrap();
    let state = json(get(&w, "/api/state").await);
    for (key, uri) in [("workspaces", "/api/workspaces"), ("pipelines", "/api/pipelines"), ("items", "/api/workitems"), ("sessions", "/api/sessions"), ("plans", "/api/plans")] {
        assert_eq!(state[key], json(get(&w, uri).await), "{key}");
    }
    assert_eq!(state["items"].as_array().unwrap().len(), 2);
    assert_eq!(state["items"].as_array().unwrap().iter().filter(|i| i["session"] == true).count(), 1);
    assert_eq!(state["sessions"][0]["name"], "wi-new-a");
    assert_eq!(state["plans"][0]["live"], true);
}

#[tokio::test]
async fn invalid_board_file_keeps_the_last_valid_board_and_is_reported() {
    let w = world();
    let stages = |body: &str| -> serde_json::Value {
        let boards: serde_json::Value = serde_json::from_str(body).unwrap();
        boards["app"][0].clone()
    };
    let file = w.ws.join("boards/default.json");
    fs::create_dir_all(file.parent().unwrap()).unwrap();

    let bundled = stages(&get(&w, "/api/pipelines").await);
    assert_eq!(bundled["error"], serde_json::Value::Null);
    assert_eq!(bundled["pipeline"]["stages"][0]["name"], "analyze");

    fs::write(&file, board(&w, r#"[{"name":"qa","next":["shipped"]},{"name":"shipped"}]"#)).unwrap();
    let own = stages(&get(&w, "/api/pipelines").await);
    assert_eq!(own["pipeline"]["stages"][0]["name"], "qa");
    assert_eq!(own["pipeline"]["entry"], "qa");

    fs::write(&file, board(&w, r#"[{"name":"qa","next":["gone"]}]"#)).unwrap();
    let broken = stages(&get(&w, "/api/pipelines").await);
    assert!(broken["error"].as_str().unwrap().contains("gone"), "{broken}");
    assert_eq!(broken["pipeline"]["stages"][0]["name"], "qa", "the last valid board stays");

    fs::write(&file, board(&w, r#"[{"name":"qa"}]"#)).unwrap();
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
    fs::create_dir_all(w.ws.clone()).unwrap();
    fs::write(w.ws.join("boards/default.json"), board_file(&w, OWN_BOARD)).unwrap();
    write_item(&w.ws, "T-40-reject", "verify", "");
    write_item(&w.ws, "T-41-approve", "verify", "");
    write_item(&w.ws, "T-42-not-a-gate", "idea", "");
    write_item(&w.ws, "T-43-merged", "waiting", PR);

    let items: serde_json::Value = serde_json::from_str(&get(&w, "/api/workitems").await).unwrap();
    let role = |dir: &str, key: &str| items.as_array().unwrap().iter().find(|i| i["dir"] == dir).unwrap()[key].clone();
    assert_eq!(role("T-41-approve", "gate")["label"], "Implementation approved");
    assert_eq!(role("T-42-not-a-gate", "gate"), serde_json::Value::Null);
    assert_eq!(role("T-43-merged", "pr_watch"), true);

    let (status, _) = post(&w, "/api/workitems/app/T-42-not-a-gate/approve", "").await;
    assert_eq!(status, StatusCode::CONFLICT, "only a gate stage can be approved");

    let (status, _) = post(&w, "/api/workitems/app/T-40-reject/reject", r#"{"comment":"no"}"#).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(field(&w.ws, "T-40-reject", "stage"), "idea");

    let (status, _) = post(&w, "/api/workitems/app/T-41-approve/approve", "").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(field(&w.ws, "T-41-approve", "stage"), "build");
    assert_eq!(field(&w.ws, "T-41-approve", "approved"), "true");

    set_pr(&w, "MERGED", "APPROVED", "UNKNOWN", "SUCCESS");
    settle_pr(&w).await;
    assert_eq!(field(&w.ws, "T-43-merged", "stage"), "shipped", "on_merge decides where a merged PR goes");
}

#[tokio::test]
async fn a_refused_approval_move_leaves_the_item_unapproved() {
    let w = world();
    fs::create_dir_all(w.ws.clone()).unwrap();
    fs::write(
        w.ws.join("boards/default.json"),
        r#"{ "brief_sections": [], "entry": "a", "stages": [
            { "name": "a", "next": ["b"], "gate": { "label": "Go", "approve_to": "b", "reject_to": "b" } },
            { "name": "b", "require": { "issue": "linked" } } ] }"#,
    )
    .unwrap();
    write_item(&w.ws, "T-44-no-issue", "a", "");
    let text = fs::read_to_string(w.ws.join("T-44-no-issue/workitem.md")).unwrap();
    fs::write(w.ws.join("T-44-no-issue/workitem.md"), text.replace("issue: T-1", "issue: none")).unwrap();

    let (status, body) = post(&w, "/api/workitems/app/T-44-no-issue/approve", "").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(field(&w.ws, "T-44-no-issue", "stage"), "a");
    assert_eq!(field(&w.ws, "T-44-no-issue", "approved"), "false");
}

fn write_skill(skills: &Path, name: &str, body: &str) {
    fs::create_dir_all(skills.join(name)).unwrap();
    fs::write(skills.join(name).join("SKILL.md"), format!("---\nname: {name}\ndescription: test\n---\n\n{body}")).unwrap();
}

fn args_log(w: &World) -> String {
    fs::read_dir(w.dir.path().join("logs")).unwrap().map(|e| fs::read_to_string(e.unwrap().path()).unwrap()).collect()
}

#[tokio::test]
async fn items_follow_the_board_they_name_and_boards_are_listed_by_name() {
    let w = world();
    let boards = w.ws.join("boards");
    fs::create_dir_all(&boards).unwrap();
    fs::write(boards.join("research.json"), board(&w, r#"[{"name":"qa","handler":"my-qa","auto":true,"next":["shipped"]},{"name":"shipped"}]"#)).unwrap();
    write_skill(&boards.join("research/skills"), "my-qa", "RESEARCH QA\n");
    fs::write(boards.join("a-first.json"), board(&w, r#"[{"name":"qa"}]"#)).unwrap();
    fs::write(boards.join("not-a-board.txt"), "x").unwrap();
    write_item(&w.ws, "T-50-research", "qa", "board: research\n");
    write_item(&w.ws, "T-51-lost", "qa", "board: gone\n");
    write_item(&w.ws, "T-52-default", "analyze", "");

    let listed: serde_json::Value = serde_json::from_str(&get(&w, "/api/pipelines").await).unwrap();
    let names: Vec<&str> = listed["app"].as_array().unwrap().iter().map(|b| b["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["a-first", "default", "research"]);
    assert_eq!(listed["app"][0]["pipeline"]["repo"], w.repo.display().to_string(), "each board names its repo");

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
async fn item_sessions_run_on_the_model_of_their_stage_and_others_on_the_default() {
    let w = world();
    fs::create_dir_all(w.ws.join("boards")).unwrap();
    fs::write(w.ws.join("boards/research.json"), board(&w, r#"[{"name":"qa","prompt":"QA IT","auto":true,"model":"sonnet[1m]","next":["shipped"]},{"name":"shipped"}]"#)).unwrap();
    write_item(&w.ws, "T-60-model", "qa", "board: research\n");

    settle(&w).await;
    eventually("the handler ran on the stage's model", || args_log(&w).contains("model=sonnet[1m]")).await;

    let (status, _) = post(&w, "/api/workspaces/app/create", r#"{"input":"look into x","board":"research"}"#).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    eventually("the create session started", || args_log(&w).contains("--board research")).await;
    assert_eq!(args_log(&w).matches("model=").count(), 1, "a create session has no stage, so no --model");
}

#[tokio::test]
async fn new_workitems_start_on_a_board_of_the_workspace() {
    let w = world();
    fs::create_dir_all(w.ws.join("boards")).unwrap();
    fs::write(w.ws.join("boards/research.json"), board(&w, r#"[{"name":"qa"}]"#)).unwrap();

    let (status, _) = post(&w, "/api/workspaces/app/create", r#"{"input":"look into x","board":"nope"}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = post(&w, "/api/workspaces/app/create", r#"{"input":"look into x","board":"../x"}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = post(&w, "/api/workspaces/app/create", r#"{"input":"look into x","board":"research"}"#).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    eventually("the create session got the board", || args_log(&w).contains("--board research")).await;
    let (status, _) = post(&w, "/api/workspaces/app/create", r#"{"input":"and y"}"#).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    eventually("a create without a board gets the first one", || args_log(&w).contains("and y")).await;
    let ws = w.ws.display();
    assert_eq!(args_log(&w).matches(&format!("arguments --workspace {ws} --board research.")).count(), 1);
    assert_eq!(args_log(&w).matches(&format!("arguments --workspace {ws} --board default.")).count(), 1);
    assert!(args_log(&w).contains(&format!("- research: repo {}", w.repo.display())), "the boards and their repos are listed");
}

#[tokio::test]
async fn handler_instructions_come_from_the_board_then_the_project_then_the_bundled_skills() {
    let w = world();
    let project = w.state.store.workspace("app").unwrap();
    let workitems = w.ws.clone();
    fs::create_dir_all(&workitems).unwrap();
    let find = |board: &str, name: &str| workitem_ui::launch::instructions(&w.state, &project, board, name);

    write_skill(&workitems.join("skills"), "my-qa", "PROJECT QA\n");
    assert_eq!(find("research", "my-qa").unwrap(), "PROJECT QA\n", "front matter is stripped");
    write_skill(&workitems.join("boards/research/skills"), "my-qa", "BOARD QA\n");
    assert_eq!(find("research", "my-qa").unwrap(), "BOARD QA\n");
    assert_eq!(find("default", "my-qa").unwrap(), "PROJECT QA\n", "another board still gets the project's");

    let bundled = find("default", "workitem-analyze").unwrap();
    assert!(!bundled.is_empty() && !bundled.starts_with("---"), "{bundled}");
    write_skill(&workitems.join("skills"), "workitem-start", "MY START\n");
    assert_eq!(find("default", "workitem-start").unwrap(), "MY START\n", "a project overrides a bundled skill");

    assert!(find("default", "nope").unwrap_err().contains("not found"));
    assert!(find("default", "workitem-triage").unwrap_err().contains("not found"), "triage is part of analyze");
    for bad in ["../x", "a/b", "", "Upper"] {
        assert!(find("default", bad).unwrap_err().contains("not a handler name"), "{bad}");
    }
}

#[tokio::test]
async fn a_missing_handler_blocks_the_item_and_an_inline_prompt_needs_no_file() {
    let w = world();
    fs::create_dir_all(w.ws.clone()).unwrap();
    fs::write(
        w.ws.join("boards/default.json"),
        board(&w, r#"[{"name":"qa","handler":"no-such-skill","auto":true,"next":["think"]},{"name":"think","prompt":"THINK HARD about it.","auto":true,"next":["qa"]}]"#),
    )
    .unwrap();
    write_item(&w.ws, "T-60-missing", "qa", "");
    write_item(&w.ws, "T-61-inline", "think", "");

    settle(&w).await;
    let status = field(&w.ws, "T-60-missing", "agent_status");
    assert!(status.starts_with("blocked: handler 'no-such-skill' not found"), "{status}");
    assert!(!w.tmux.has("wi-T-60-missing"));
    eventually("the inline prompt ran", || stub_log(&w.repo).contains(&"T-61-inline think".to_string())).await;
    let args = args_log(&w);
    assert!(args.contains("THINK HARD about it."), "{args}");
    assert!(args.contains("Allowed next stages: qa (default)"), "{args}");
}

fn queue_board(w: &World) {
    let board = r#"{ "brief_sections": [], "entry": "queue", "stages": [
        { "name": "queue", "auto": true, "next": ["work"] },
        { "name": "work", "handler": "h", "auto": true, "limit": 1, "next": ["post"] },
        { "name": "post", "handler": "h", "auto": true, "once": true, "when": { "approved": "true" }, "next": [] }
    ] }"#;
    let dir = &w.ws;
    fs::create_dir_all(dir.join("skills/h")).unwrap();
    fs::write(dir.join("skills/h/SKILL.md"), "Do the work.\n").unwrap();
    fs::write(dir.join("boards/default.json"), board_file(w, board)).unwrap();
}

#[tokio::test]
async fn a_queue_stage_feeds_the_next_one_oldest_first_up_to_its_limit() {
    let w = world();
    queue_board(&w);
    write_item(&w.ws, "T-20-new", "queue", "created: 2026-01-02T10:00:00Z\n");
    write_item(&w.ws, "T-21-old", "queue", "created: 2026-01-01T10:00:00Z\n");
    write_item(&w.ws, "T-22-busy", "queue", "created: 2026-01-03T10:00:00Z\nagent_status: working\n");

    settle(&w).await;
    eventually("the oldest item is pulled first", || field(&w.ws, "T-21-old", "stage") == "work").await;
    assert_eq!(field(&w.ws, "T-20-new", "stage"), "queue", "the limit of 1 keeps the next one waiting");
    assert_eq!(field(&w.ws, "T-22-busy", "stage"), "queue");

    settle(&w).await;
    eventually("the work handler started for the pulled item", || stub_log(&w.repo) == vec!["T-21-old work"]).await;
    assert_eq!(field(&w.ws, "T-20-new", "stage"), "queue", "still full while the first item is in work");
}

#[tokio::test]
async fn once_and_when_keep_a_final_handler_from_starting_again_or_for_the_wrong_item() {
    let w = world();
    queue_board(&w);
    write_item(&w.ws, "T-23-rejected", "post", "");
    write_item(&w.ws, "T-24-approved", "post", "approved: true\n");
    fs::write(w.repo.join(".stub-post"), "done").unwrap();

    settle(&w).await;
    eventually("only the approved item got a handler", || stub_log(&w.repo) == vec!["T-24-approved post"]).await;

    settle(&w).await;
    settle(&w).await;
    settle(&w).await;
    assert_eq!(stub_log(&w.repo), vec!["T-24-approved post"], "the finished handler is not started again");
}

#[tokio::test]
async fn a_pr_status_board_shows_the_pr_chips_and_starts_nothing() {
    let w = world();
    queue_board(&w);
    let board = fs::read_to_string(w.ws.join("boards/default.json")).unwrap();
    fs::write(w.ws.join("boards/default.json"), board.replace(r#""name": "post", "handler""#, r#""name": "post", "pr_status": true, "handler""#)).unwrap();
    write_item(&w.ws, "T-30-waiting", "post", PR);
    write_item(&w.ws, "T-31-queued", "queue", PR);
    set_pr(&w, "OPEN", "CHANGES_REQUESTED", "CONFLICTING", "FAILURE");

    settle_pr(&w).await;
    let items: serde_json::Value = serde_json::from_str(&get(&w, "/api/workitems").await).unwrap();
    let review = |dir: &str| items.as_array().unwrap().iter().find(|i| i["dir"] == dir).unwrap()["review"].clone();
    assert_eq!(review("T-30-waiting")["approval"], "changes_requested");
    assert_eq!(review("T-31-queued")["merge"], "conflicting", "the board's reader reads every item with a PR");
    assert!(stub_log(&w.repo).is_empty() && !w.tmux.has("wi-T-30-waiting"), "red CI and conflicts start no fixer");
    assert_eq!(review("T-30-waiting")["approved_by_me"], false);

    // The stub's `gh api user` is "me": an approval by that login lights the extra icon.
    let mut pr: serde_json::Value = serde_json::from_str(&fs::read_to_string(w.dir.path().join("pr.json")).unwrap()).unwrap();
    pr["reviews"] = serde_json::json!([{ "id": "r1", "state": "APPROVED", "author": { "login": "me" } }]);
    fs::write(w.dir.path().join("pr.json"), pr.to_string()).unwrap();
    settle(&w).await;
    let items: serde_json::Value = serde_json::from_str(&get(&w, "/api/workitems").await).unwrap();
    let mine = items.as_array().unwrap().iter().find(|i| i["dir"] == "T-30-waiting").unwrap()["review"]["approved_by_me"].clone();
    assert_eq!(mine, true);
}

fn write_watch_script(ws: &Path, name: &str, body: &str) {
    let dir = ws.join("watch");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/bash\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[tokio::test]
async fn a_column_watcher_runs_its_script_once_per_interval_with_the_column_as_context() {
    let w = world();
    write_item(&w.ws, "T-30-in", "queue", "");
    write_item(&w.ws, "T-31-out", "other", "");
    write_watch_script(&w.ws, "record", "echo \"$WORKITEM_BOARD $WORKITEM_STAGE $WORKITEM_NEXT $1\" >> .watch-log\ncat >> .watch-items");
    fs::write(
        w.ws.join("boards/default.json"),
        board(&w, r#"[{"name":"queue","next":["other"],"watch":{"every":"1d","command":"record","args":["hello"]}},{"name":"other"}]"#).replace(r#""entry": "qa""#, r#""entry": "queue""#),
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
async fn a_board_watcher_gets_every_item_of_the_board_and_no_stage() {
    let w = world();
    write_item(&w.ws, "T-34-queue", "queue", "");
    write_item(&w.ws, "T-35-done", "other", "");
    write_item(&w.ws, "T-36-elsewhere", "queue", "board: side\n");
    write_watch_script(&w.ws, "record", "echo \"$WORKITEM_BOARD [$WORKITEM_STAGE] [$WORKITEM_NEXT]\" >> .watch-log\ncat >> .watch-items");
    let stages = r#"[{"name":"queue","next":["other"]},{"name":"other"}], "watch": {"every":"1d","command":"record"}"#;
    fs::write(w.ws.join("boards/default.json"), board(&w, stages).replace(r#""entry": "qa""#, r#""entry": "queue""#)).unwrap();

    dispatch::pass(&w.state).await;
    eventually("the watcher ran", || w.repo.join(".watch-items").exists()).await;
    dispatch::pass(&w.state).await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    assert_eq!(fs::read_to_string(w.repo.join(".watch-log")).unwrap(), "default [] []\n", "once, not per pass");
    let items = fs::read_to_string(w.repo.join(".watch-items")).unwrap();
    assert!(items.contains("T-34-queue") && items.contains("T-35-done"), "every column: {items}");
    assert!(!items.contains("T-36-elsewhere"), "only this board: {items}");
}

#[tokio::test]
async fn a_workspace_watcher_runs_once_for_every_board_in_the_workspace_folder() {
    let w = world();
    write_item(&w.ws, "T-37-default", "queue", "");
    write_item(&w.ws, "T-38-side", "queue", "board: side\n");
    write_watch_script(&w.ws, "record", "echo \"[$WORKITEM_BOARD] [$WORKITEM_STAGE] [${WORKITEM_REPO-unset}] $1\" >> .watch-log\ncat >> .watch-items");
    let stages = board(&w, r#"[{"name":"queue","next":["other"]},{"name":"other"}]"#).replace(r#""entry": "qa""#, r#""entry": "queue""#);
    fs::write(w.ws.join("boards/default.json"), &stages).unwrap();
    fs::write(w.ws.join("boards/side.json"), &stages).unwrap();
    fs::write(w.ws.join("workspace.toml"), "[[watch]]\nevery = \"1d\"\ncommand = \"record\"\nargs = [\"hi\"]\n").unwrap();

    dispatch::pass(&w.state).await;
    eventually("the watcher ran", || w.ws.join(".watch-items").exists()).await;
    dispatch::pass(&w.state).await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    assert_eq!(fs::read_to_string(w.ws.join(".watch-log")).unwrap(), "[] [] [unset] hi\n", "once, not per board or pass");
    let items = fs::read_to_string(w.ws.join(".watch-items")).unwrap();
    assert!(items.contains("T-37-default") && items.contains("T-38-side"), "every board: {items}");
    let workspaces: serde_json::Value = serde_json::from_str(&get(&w, "/api/workspaces").await).unwrap();
    assert_eq!(workspaces[0]["watch"], serde_json::json!([{ "every": "1d", "command": "record", "args": ["hi"], "implicit": false }]), "no board reads PRs");
}

#[tokio::test]
async fn a_board_with_a_pr_stage_gives_the_workspace_one_pr_status_unless_it_lists_its_own() {
    let w = world();
    let template = fs::read_to_string(w.ws.join("boards/default.json")).unwrap();
    fs::write(w.ws.join("boards/second.json"), &template).unwrap();
    let watch = || async { serde_json::from_str::<serde_json::Value>(&get(&w, "/api/workspaces").await).unwrap()[0]["watch"].clone() };
    assert_eq!(watch().await, serde_json::json!([{ "every": "1m", "command": "pr-status", "args": [], "implicit": true }]), "one for both boards");
    fs::write(w.ws.join("workspace.toml"), "[[watch]]\nevery = \"2m\"\ncommand = \"pr-status\"\n").unwrap();
    assert_eq!(watch().await, serde_json::json!([{ "every": "2m", "command": "pr-status", "args": [], "implicit": false }]));
    fs::write(w.ws.join("workspace.toml"), "[[watch]]\nevery = \"oops\"\n").unwrap();
    let workspaces: serde_json::Value = serde_json::from_str(&get(&w, "/api/workspaces").await).unwrap();
    assert!(workspaces[0]["error"].as_str().unwrap().contains("workspace.toml"));
    assert_eq!(workspaces[0]["watch"][0]["every"], "2m", "the last valid file stays in use");
}

#[tokio::test]
async fn a_paused_column_starts_no_watcher_until_it_is_resumed() {
    let w = world();
    write_item(&w.ws, "T-33-in", "queue", "");
    write_watch_script(&w.ws, "record", "echo ran >> .watch-log");
    fs::write(
        w.ws.join("boards/default.json"),
        board(&w, r#"[{"name":"queue","next":["other"],"watch":{"every":"1d","command":"record"}},{"name":"other"}]"#).replace(r#""entry": "qa""#, r#""entry": "queue""#),
    )
    .unwrap();
    let pause = |paused: bool| {
        let request = Request::put("/api/workspaces/app/boards/default/stages/queue/paused")
            .header(header::HOST, format!("127.0.0.1:{}", w.state.port))
            .header(header::COOKIE, format!("wi_session={}", w.token))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(format!(r#"{{"paused":{paused}}}"#)))
            .unwrap();
        router(w.state.clone()).oneshot(request)
    };

    assert_eq!(pause(true).await.unwrap().status(), StatusCode::NO_CONTENT);
    settle(&w).await;
    assert!(!w.repo.join(".watch-log").exists(), "a paused watcher does not run");
    let boards = get(&w, "/api/pipelines").await;
    assert!(boards.contains(r#""watched":["queue"]"#) && boards.contains(r#""paused":["queue"]"#), "{boards}");
    let stored = fs::read_to_string(w.state.store.sibling("paused.json")).unwrap();
    assert!(stored.contains("app/default/queue"), "the pause survives a restart: {stored}");

    assert_eq!(pause(false).await.unwrap().status(), StatusCode::NO_CONTENT);
    settle(&w).await;
    eventually("the resumed watcher ran", || w.repo.join(".watch-log").exists()).await;
    assert!(get(&w, "/api/pipelines").await.contains(r#""paused":[]"#));
}

#[tokio::test]
async fn a_watcher_with_a_missing_script_does_nothing_and_does_not_stop_the_dispatcher() {
    let w = world();
    write_item(&w.ws, "T-32-qa", "qa", "");
    fs::write(
        w.ws.join("boards/default.json"),
        board(&w, r#"[{"name":"qa","handler":"my-qa","auto":true,"next":["x"]},{"name":"x","watch":{"every":"1m","command":"nope"}}]"#),
    )
    .unwrap();
    write_skill(&w.ws.join("skills"), "my-qa", "QA\n");
    settle(&w).await;
    eventually("handlers still run", || stub_log(&w.repo) == vec!["T-32-qa qa"]).await;
}

#[tokio::test]
async fn the_bundled_age_out_script_deletes_or_moves_old_items_and_spares_new_ones_and_open_prs() {
    let w = world();
    write_item(&w.ws, "T-40-old", "done", "created: 2020-01-01T10:00:00Z\n");
    write_item(&w.ws, "T-41-new", "done", &format!("created: {}\n", "2999-01-01T10:00:00Z"));
    write_item(&w.ws, "T-42-pr", "done", "created: 2020-01-01T10:00:00Z\npr: https://github.com/o/r/pull/1\n");
    fs::write(
        w.ws.join("boards/default.json"),
        board(&w, r#"[{"name":"done","watch":{"every":"1d","command":"age-out","args":["--older-than","30d","--delete"]}}]"#).replace(r#""entry": "qa""#, r#""entry": "done""#),
    )
    .unwrap();
    settle(&w).await;
    let items = w.ws.clone();
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
    write_item(&w.ws, "T-50-qa", "qa", "");
    write_item(&w.ws, "T-51-blocked", "qa", "agent_status: blocked: need input\n");
    write_item(&w.ws, "T-52-manual", "shipped", "");
    fs::write(
        w.ws.join("boards/default.json"),
        board(&w, r#"[{"name":"qa","handler":"my-qa","next":["shipped"]},{"name":"shipped"}]"#),
    )
    .unwrap();
    write_skill(&w.ws.join("skills"), "my-qa", "QA STEPS\n");

    let denied = Request::post("/api/workitems/app/T-50-qa/run").header(header::HOST, format!("127.0.0.1:{}", w.state.port)).body(Body::empty()).unwrap();
    assert_eq!(router(w.state.clone()).oneshot(denied).await.unwrap().status(), StatusCode::UNAUTHORIZED);

    let (status, body) = post_bearer(&w, "/api/workitems/app/T-50-qa/run").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    eventually("the handler ran", || stub_log(&w.repo) == vec!["T-50-qa qa"]).await;
    assert_eq!(field(&w.ws, "T-50-qa", "agent_status"), "working");
    assert!(args_log(&w).contains("QA STEPS"));

    assert_eq!(post_bearer(&w, "/api/workitems/app/T-50-qa/run").await.0, StatusCode::CONFLICT, "already running");
    assert_eq!(post_bearer(&w, "/api/workitems/app/T-51-blocked/run").await.0, StatusCode::CONFLICT, "blocked items are left alone");
    let (status, body) = post_bearer(&w, "/api/workitems/app/T-52-manual/run").await;
    assert_eq!(status, StatusCode::CONFLICT, "a stage without a handler has nothing to run");
    assert!(body.contains("no handler"), "{body}");
}

#[tokio::test]
async fn an_item_on_a_board_without_a_repo_runs_in_its_folder_with_every_repo_added() {
    let w = world();
    fs::write(
        w.ws.join("boards/intake.json"),
        r#"{ "brief_sections": [], "entry": "analyze", "stages": [{"name":"analyze","handler":"my-analyze","next":["done"]},{"name":"done"}] }"#,
    )
    .unwrap();
    write_skill(&w.ws.join("skills"), "my-analyze", "ANALYZE STEPS\n");
    write_item(&w.ws, "T-60-idea", "analyze", "board: intake\n");
    // A server started from an item session holds that item's vars; a session must not inherit them.
    w.tmux.run(&["start-server", ";", "set-option", "-g", "exit-empty", "off", ";", "set-environment", "-g", "WORKITEM_REPO", "/leak", ";", "set-environment", "-g", "WORKITEM_WORKTREE", "/leak"]);

    let (status, body) = post_bearer(&w, "/api/workitems/app/T-60-idea/run").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let cwd = w.dir.path().join("cwd-T-60-idea");
    eventually("the handler ran", || cwd.exists()).await;
    let line = fs::read_to_string(&cwd).unwrap();
    let [pwd, repo, worktree]: [&str; 3] = line.trim_end().split('|').collect::<Vec<_>>().try_into().unwrap();
    assert_eq!(worktree, "", "no WORKITEM_WORKTREE");
    assert_eq!(Path::new(&pwd).canonicalize().unwrap(), w.ws.join("T-60-idea").canonicalize().unwrap(), "starts in the item folder");
    assert_eq!(repo, "", "no WORKITEM_REPO");
    let log = args_log(&w);
    assert!(log.starts_with("argc=8\nclaude_md=1\n"), "--add-dir, workspace, the other boards' repo, --mcp-config, its file, --settings, hooks, prompt: {log}");
    assert!(log.contains("ANALYZE STEPS") && log.contains("board without a repo"), "{log}");
    let pipelines = get(&w, "/api/pipelines").await;
    assert!(pipelines.contains(r#""name":"intake""#) && !pipelines.contains(r#""repo":null"#), "{pipelines}");
}

async fn post_json_bearer(w: &World, uri: &str, body: &str) -> (StatusCode, String) {
    let request = Request::post(uri)
        .header(header::HOST, format!("127.0.0.1:{}", w.state.port))
        .header(header::AUTHORIZATION, format!("Bearer {}", w.token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = router(w.state.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    (status, String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes()).into_owned())
}

#[tokio::test]
async fn run_starts_a_named_handler_instead_of_the_stage_handler() {
    let w = world();
    write_item(&w.ws, "T-53-review", "shipped", "");
    write_item(&w.ws, "T-54-review", "shipped", "");
    fs::write(w.ws.join("boards/default.json"), board(&w, r#"[{"name":"qa","next":["shipped"]},{"name":"shipped"}]"#)).unwrap();
    write_skill(&w.ws.join("skills"), "move-ticket", "TICKET STEPS\n");

    let (status, body) = post_json_bearer(&w, "/api/workitems/app/T-53-review/run", r#"{"handler":"move-ticket"}"#).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    eventually("the named handler ran", || stub_log(&w.repo) == vec!["T-53-review shipped"]).await;
    assert_eq!(field(&w.ws, "T-53-review", "agent_status"), "working");
    assert!(args_log(&w).contains("TICKET STEPS"));

    let (status, body) = post_json_bearer(&w, "/api/workitems/app/T-54-review/run", r#"{"handler":"missing"}"#).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "an unknown handler cannot be used");
    assert!(body.contains("not found"), "{body}");
    let (status, _) = post_json_bearer(&w, "/api/workitems/app/T-54-review/run", r#"{"handler":"../x"}"#).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "a handler name is not a path");
}

#[tokio::test]
async fn learnings_are_read_and_saved_per_board() {
    let w = world();
    let boards = w.ws.join("boards");
    fs::create_dir_all(&boards).unwrap();
    fs::write(boards.join("review.json"), board(&w, r#"[{"name":"qa"}]"#)).unwrap();
    let read = |board: &'static str| {
        let w = &w;
        async move { serde_json::from_str::<serde_json::Value>(&get(w, &format!("/api/workspaces/app/boards/{board}/learnings")).await).unwrap() }
    };
    let save = |board: &str, base: &str, content: &str| {
        let request = Request::put(format!("/api/workspaces/app/boards/{board}/learnings"))
            .header(header::HOST, format!("127.0.0.1:{}", w.state.port))
            .header(header::COOKIE, format!("wi_session={}", w.token))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::json!({ "base": base, "content": content }).to_string()))
            .unwrap();
        router(w.state.clone()).oneshot(request)
    };

    let none = read("review").await;
    assert_eq!(none["content"], "", "no file yet");
    assert!(none["path"].as_str().unwrap().ends_with("workspaces/app/boards/review/learnings.md"));
    assert!(read("default").await["path"].as_str().unwrap().ends_with("workspaces/app/boards/default/learnings.md"));

    assert_eq!(save("review", "", "# Learnings\n\n- Flag only real bugs.\n").await.unwrap().status(), StatusCode::NO_CONTENT);
    assert_eq!(fs::read_to_string(boards.join("review/learnings.md")).unwrap(), "# Learnings\n\n- Flag only real bugs.\n");
    assert_eq!(read("review").await["content"], "# Learnings\n\n- Flag only real bugs.\n");
    assert_eq!(read("default").await["content"], "", "boards do not share a file");

    let stale = save("review", "", "- Mine.\n").await.unwrap();
    assert_eq!(stale.status(), StatusCode::CONFLICT, "a fixer's edit since opening the editor is never overwritten");
    assert_eq!(read("review").await["content"], "# Learnings\n\n- Flag only real bugs.\n");

    assert_eq!(save("nope", "", "- x\n").await.unwrap().status(), StatusCode::NOT_FOUND);
    assert_eq!(save("..", "", "- x\n").await.unwrap().status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_stage_with_learnings_gets_its_boards_file() {
    let w = world();
    let boards = w.ws.join("boards");
    fs::create_dir_all(boards.join("review")).unwrap();
    fs::write(
        boards.join("review.json"),
        board(&w, r#"[{"name":"qa","handler":"my-qa","auto":true,"learnings":true,"next":["post"]},{"name":"post","handler":"my-qa","auto":true}]"#),
    )
    .unwrap();
    write_skill(&boards.join("review/skills"), "my-qa", "REVIEW QA\n");
    fs::write(boards.join("review/learnings.md"), "- Flag only real bugs.\n").unwrap();
    fs::create_dir_all(w.ws.join("boards/default")).unwrap();
    fs::write(w.ws.join("boards/default/learnings.md"), "- Dev rule.\n").unwrap();
    write_item(&w.ws, "T-60-qa", "qa", "board: review\n");
    write_item(&w.ws, "T-61-post", "post", "board: review\n");

    settle(&w).await;
    eventually("both handlers ran", || stub_log(&w.repo).len() == 2).await;
    let prompts: Vec<String> = fs::read_dir(w.dir.path().join("logs")).unwrap().map(|e| fs::read_to_string(e.unwrap().path()).unwrap()).collect();
    let qa = prompts.iter().find(|p| p.contains("Current stage: qa")).unwrap();
    let post = prompts.iter().find(|p| p.contains("Current stage: post")).unwrap();
    assert!(qa.contains("- Flag only real bugs.") && !qa.contains("Dev rule"), "{qa}");
    assert!(!post.contains("Flag only real bugs"), "a stage without the key gets none: {post}");
}

#[tokio::test]
async fn stage_source_shows_the_files_the_dispatcher_resolves() {
    let w = world();
    let workitems = w.ws.clone();
    fs::create_dir_all(workitems.join("boards")).unwrap();
    fs::write(
        workitems.join("boards/research.json"),
        board(
            &w,
            r#"[{"name":"qa","handler":"my-qa","auto":true,"model":"sonnet"},
                {"name":"inline","prompt":"Do the thing."},
                {"name":"pr","handler":"builtin:pr-review","auto":true,"next":["gate"],"on_merge":"gate"},
                {"name":"gate"},
                {"name":"lost","handler":"nope","auto":true}]"#,
        ),
    )
    .unwrap();
    write_skill(&workitems.join("skills"), "my-qa", "PROJECT QA\n");
    write_skill(&workitems.join("boards/research/skills"), "my-qa", "BOARD QA\n");
    write_skill(&workitems.join("boards/research/skills"), "workitem-fix-ci", "BOARD CI FIXER\n");
    let source = |stage: &str| {
        let w = &w;
        let uri = format!("/api/workspaces/app/boards/research/stages/{stage}/source");
        async move { serde_json::from_str::<serde_json::Value>(&get(w, &uri).await).unwrap() }
    };

    let qa = source("qa").await;
    assert_eq!(qa["model"], "sonnet");
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
    assert!(inline["model"].is_null(), "no model: the agent's default");

    let pr = source("pr").await;
    assert!(pr["prose"].is_null(), "a builtin handler has no skill");
    assert_eq!(pr["scripts"][0]["name"], "pr-review");
    let fixers: Vec<_> = pr["fixers"].as_array().unwrap().iter().map(|f| (f["name"].as_str().unwrap(), f["origin"].as_str().unwrap())).collect();
    assert_eq!(fixers, [("workitem-fix-ci", "board"), ("workitem-fix-comments", "bundled"), ("workitem-fix-conflict", "bundled")]);
    assert!(pr["fixers"][0]["content"].as_str().unwrap().contains("BOARD CI FIXER"));
    assert!(qa["fixers"].as_array().unwrap().is_empty(), "only the PR stage has fixers");

    let gate = source("gate").await;
    assert!(gate["prose"].is_null() && gate["scripts"].as_array().unwrap().is_empty());

    let lost = source("lost").await;
    assert!(lost["prose"]["error"].as_str().unwrap().contains("not found"));

    for uri in [
        "/api/workspaces/app/boards/research/stages/nope/source",
        "/api/workspaces/app/boards/nope/stages/qa/source",
        "/api/workspaces/app/boards/..%2Fx/stages/qa/source",
        "/api/workspaces/other/boards/research/stages/qa/source",
    ] {
        assert!(get(&w, uri).await.contains("not found"), "{uri}");
    }
}

#[tokio::test]
async fn files_tab_lists_and_serves_the_workitem_folder_sandboxed() {
    let w = world();
    write_item(&w.ws, "T-1-files", "analyze", "");
    let dir = w.ws.join("T-1-files");
    fs::create_dir_all(dir.join("notes")).unwrap();
    fs::write(dir.join("notes/page.html"), "<script>x</script>").unwrap();
    fs::write(dir.join("notes/shot.png"), [0x89, b'P', b'N', b'G']).unwrap();
    fs::write(w.ws.join("secret.txt"), "no").unwrap();

    let list: serde_json::Value = serde_json::from_str(&get(&w, "/api/workitems/app/T-1-files/files").await).unwrap();
    let paths: Vec<_> = list.as_array().unwrap().iter().map(|e| e["path"].as_str().unwrap()).collect();
    assert_eq!(paths, ["notes/page.html", "notes/shot.png", "workitem.md"]);

    let request = Request::get("/api/workitems/app/T-1-files/files/notes/page.html")
        .header(header::HOST, format!("127.0.0.1:{}", w.state.port))
        .header(header::COOKIE, format!("wi_session={}", w.token))
        .body(Body::empty())
        .unwrap();
    let response = router(w.state.clone()).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "text/html; charset=utf-8");
    assert_eq!(response.headers()[header::CONTENT_SECURITY_POLICY], "sandbox allow-scripts");

    // Video and audio seek with range requests.
    let request = Request::get("/api/workitems/app/T-1-files/files/notes/page.html")
        .header(header::HOST, format!("127.0.0.1:{}", w.state.port))
        .header(header::COOKIE, format!("wi_session={}", w.token))
        .header(header::RANGE, "bytes=8-9")
        .body(Body::empty())
        .unwrap();
    let response = router(w.state.clone()).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 8-9/18");
    assert_eq!(response.headers()[header::CONTENT_TYPE], "text/html; charset=utf-8");
    assert_eq!(response.headers()[header::CONTENT_SECURITY_POLICY], "sandbox allow-scripts");
    assert_eq!(response.into_body().collect().await.unwrap().to_bytes(), "x<");

    // Only text gets a charset.
    let request = Request::get("/api/workitems/app/T-1-files/files/notes/shot.png")
        .header(header::HOST, format!("127.0.0.1:{}", w.state.port))
        .header(header::COOKIE, format!("wi_session={}", w.token))
        .body(Body::empty())
        .unwrap();
    let response = router(w.state.clone()).oneshot(request).await.unwrap();
    assert_eq!(response.headers()[header::CONTENT_TYPE], "image/png");

    for uri in ["/api/workitems/app/T-1-files/files/..%2Fsecret.txt", "/api/workitems/app/T-1-files/files/notes"] {
        assert!(get(&w, uri).await.contains("file not found"), "{uri}");
    }
    for path in ["../secret.txt", "notes", "/etc/hosts"] {
        let (status, _) = post(&w, "/api/workitems/app/T-1-files/reveal", &serde_json::json!({ "path": path }).to_string()).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn a_workspace_github_account_reaches_sessions_and_watchers_and_a_missing_one_stops_it() {
    let w = world();
    let probe = w.dir.path().join("probe");
    fs::create_dir_all(w.ws.join("watch")).unwrap();
    fs::write(w.ws.join("watch/token-probe"), format!("#!/bin/sh\necho \"token=$GH_TOKEN\" >> {}\n", probe.display())).unwrap();
    fs::set_permissions(w.ws.join("watch/token-probe"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(w.ws.join("workspace.toml"), "[[watch]]\nevery = \"1m\"\ncommand = \"token-probe\"\n").unwrap();
    write_item(&w.ws, "T-40-gh", "ready_for_development", "");

    let (status, body) = put(&w, "/api/workspaces/app", r#"{"github_account":"nobody"}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "gh must know the account: {body}");
    let (status, body) = put(&w, "/api/workspaces/app", r#"{"github_account":"work-bot"}"#).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""issue_tracker":"jira""#) && body.contains(r#""github_account":"work-bot""#), "{body}");
    settle(&w).await;
    assert_eq!(fs::read_to_string(&probe).unwrap(), "token=tok-123\n", "a watcher gets the account's token");
    let (status, body) = post(&w, "/api/workitems/app/T-40-gh/session", "").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    eventually("the session's token", || stub_logs(&w).iter().any(|log| log.contains("gh_token=tok-123"))).await;
    w.tmux.run(&["kill-session", "-t", "=wi-T-40-gh"]);
    settle(&w).await;

    // An account gh no longer knows stops the workspace until it is fixed.
    let runs = fs::read_to_string(&probe).unwrap().lines().count();
    fs::write(w.ws.join("workspace.toml"), "github_account = \"gone\"\n\n[[watch]]\nevery = \"1m\"\ncommand = \"token-probe\"\n").unwrap();
    settle(&w).await;
    assert_eq!(fs::read_to_string(&probe).unwrap().lines().count(), runs, "no watcher runs");
    let (status, body) = post(&w, "/api/workitems/app/T-40-gh/session", "").await;
    assert!(!status.is_success() && body.contains("GitHub account gone is not logged in"), "{status} {body}");
    assert!(get(&w, "/api/workspaces").await.contains("GitHub account gone is not logged in"), "the header says why");
    let (status, body) = put(&w, "/api/workspaces/app", r#"{"agent":null,"issue_tracker":"jira","github_account":"gone"}"#).await;
    assert_eq!(status, StatusCode::OK, "a broken saved account does not block the other settings: {body}");
    let (status, body) = put(&w, "/api/workspaces/app", r#"{"issue_tracker":"github"}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("default: ") && body.contains("can only be assigned or closed"), "the Jira states of the board would break: {body}");
    assert!(!fs::read_to_string(w.ws.join("workspace.toml")).unwrap().contains("github\""), "nothing was written");
    fs::write(w.ws.join("workspace.toml"), "[[watch]]\nevery = \"1m\"\ncommand = \"token-probe\"\n").unwrap();
    settle(&w).await;
    assert!(!get(&w, "/api/workspaces").await.contains("account_error"));
    assert_eq!(fs::read_to_string(&probe).unwrap().lines().last(), Some("token="), "without an account gh's own applies");
}

fn stub_logs(w: &World) -> Vec<String> {
    let mut logs: Vec<_> = fs::read_dir(w.dir.path().join("logs")).unwrap().filter_map(Result::ok).map(|e| e.path()).collect();
    logs.sort();
    logs.iter().map(|path| fs::read_to_string(path).unwrap()).collect()
}

#[tokio::test]
async fn a_plan_runs_in_the_workspace_ends_with_its_session_resumes_and_is_deleted() {
    let w = world();
    let (status, _) = post(&w, "/api/workspaces/app/plans", r#"{"input":"  "}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let pwn = w.dir.path().join("pwned");
    let input = format!("How should we split auth?\n$(touch {})", pwn.display());
    let (status, body) = post(&w, "/api/workspaces/app/plans", &serde_json::json!({ "input": input }).to_string()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let started: serde_json::Value = serde_json::from_str(&body).unwrap();
    let (id, session) = (started["id"].as_str().unwrap().to_string(), started["session"].as_str().unwrap().to_string());
    assert_eq!(session, format!("wi-plan-{id}"));
    assert!(w.tmux.has(&session));

    let record = w.ws.join("plans").join(format!("{id}.md"));
    let text = fs::read_to_string(&record).unwrap();
    assert!(text.contains("topic: How should we split auth?\n"), "{text}");
    assert!(text.contains("status: working\n"));
    assert!(text.contains(&format!("## Prompt\n\n{input}\n")));
    assert!(text.contains("agent: claude\n"), "{text}");
    let uuid = text.lines().find_map(|l| l.strip_prefix("agent_session: ")).unwrap().to_string();
    assert_eq!(uuid.len(), 36, "Claude takes the id from the launcher");

    eventually("stub recorded the plan prompt", || stub_logs(&w).iter().any(|t| t.contains("planning session"))).await;
    let log = stub_logs(&w).into_iter().find(|t| t.contains("planning session")).unwrap();
    assert!(log.starts_with("argc=7\nclaude_md=1\n"), "--add-dir, repo, --settings, json, --session-id, uuid, prompt; reads the repos' CLAUDE.md: {log}");
    assert!(log.contains(&format!("- default: repo {}", w.repo.display())), "the prompt lists the boards and their repos: {log}");
    assert!(log.contains(&input), "prompt keeps the text verbatim");
    assert!(!pwn.exists(), "text must never reach a shell");
    let settings: serde_json::Value = serde_json::from_str(log.lines().find_map(|l| l.strip_prefix("settings=")).unwrap()).unwrap();
    for event in ["UserPromptSubmit", "PostToolUse"] {
        let command = settings["hooks"][event][0]["hooks"][0]["command"].as_str().unwrap_or_default();
        assert!(command.contains("workitem-plan-note' --status working"), "{event} sets the record back to working, also after an approved permission prompt: {settings}");
    }

    let plans = get(&w, "/api/plans").await;
    assert!(plans.contains(r#""status":"working""#) && plans.contains(r#""live":true"#), "{plans}");
    assert!(get(&w, "/api/sessions").await == "[]", "plan sessions are not create sessions");

    // The agent saves a plan; the detail endpoint returns it apart from the prompt.
    fs::write(&record, format!("{text}\n## Plan\n\n1. Do it\n")).unwrap();
    let detail = get(&w, &format!("/api/plans/app/{id}")).await;
    assert!(detail.contains(r#""text":"1. Do it""#) && detail.contains(r#""has_plan":true"#), "{detail}");

    settle(&w).await;
    assert!(w.tmux.has(&session), "a running plan session survives the dispatcher");
    w.tmux.run(&["kill-session", "-t", &format!("={session}")]);
    settle(&w).await;
    assert!(fs::read_to_string(&record).unwrap().contains("status: ended\n"));

    let (status, _) = post(&w, &format!("/api/plans/app/{id}/resume"), "").await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(w.tmux.has(&session));
    assert!(fs::read_to_string(&record).unwrap().contains("status: working\n"));
    eventually("stub was resumed", || stub_logs(&w).iter().any(|t| t.contains(&format!("last={uuid}")))).await;
    let (status, _) = post(&w, &format!("/api/plans/app/{id}/resume"), "").await;
    assert_eq!(status, StatusCode::CONFLICT, "already running");

    let request = Request::delete(format!("/api/plans/app/{id}"))
        .header(header::HOST, format!("127.0.0.1:{}", w.state.port))
        .header(header::COOKIE, format!("wi_session={}", w.token))
        .body(Body::empty())
        .unwrap();
    assert_eq!(router(w.state.clone()).oneshot(request).await.unwrap().status(), StatusCode::NO_CONTENT);
    assert!(!record.exists());
    assert!(!w.tmux.has(&session));
    assert_eq!(get(&w, "/api/plans").await, "[]");
}

#[tokio::test]
async fn a_prompt_over_the_tmux_command_limit_reaches_the_agent_unchanged() {
    let w = world();
    let project = w.state.store.workspace("app").unwrap();
    // tmux refuses commands over about 16 KB; the prompt must not be part of one.
    let prompt = format!("{}\n'quotes' \"and\" $(no expansion) `here`\n\n", "x".repeat(20_000));
    workitem_ui::launch::spawn(&w.state, "wi-big", &project, workitem_ui::launch::Scope::Workspace, &prompt).await.unwrap();
    eventually("the agent ran", || !args_log(&w).is_empty()).await;
    let log = args_log(&w);
    let (args, _settings) = log.split_once("\nsettings=").unwrap();
    assert_eq!(args, format!("argc=5\nclaude_md=1\nlast={prompt}"), "a workspace session reads the repos' CLAUDE.md");
    assert!(w.tmux.has("wi-big"));
}

#[tokio::test]
async fn a_handler_that_cannot_start_blocks_the_item_with_the_cause() {
    let w = world();
    write_item(&w.ws, "T-60-qa", "qa", "");
    fs::write(w.ws.join("boards/default.json"), board(&w, r#"[{"name":"qa","handler":"my-qa","next":["shipped"]},{"name":"shipped"}]"#)).unwrap();
    write_skill(&w.ws.join("skills"), "my-qa", "QA STEPS\n");
    // A session of the same name, not started by the UI, is in the way.
    w.tmux.run(&["new-session", "-d", "-s", "wi-T-60-qa", "sleep 60"]);

    let (status, body) = post_bearer(&w, "/api/workitems/app/T-60-qa/run").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let agent_status = field(&w.ws, "T-60-qa", "agent_status");
    assert!(agent_status.starts_with("blocked: cannot start wi-T-60-qa: session wi-T-60-qa already exists"), "{agent_status}");
}

/// Posts `uri` while a watcher launches the item's handler between the request's read of the
/// item and its turn on `busy`: the session appears and the status turns `working` meanwhile.
async fn post_racing_a_launch(w: &World, dir: &str, uri: &str, json: &str) -> (StatusCode, String) {
    let busy = w.state.busy.lock().await;
    let launch = async {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        own_session(w, &format!("wi-{dir}"));
        let path = w.ws.clone().join(dir).join("workitem.md");
        fs::write(&path, fs::read_to_string(&path).unwrap().replace("agent_status: ok", "agent_status: working")).unwrap();
        drop(busy);
    };
    tokio::join!(post(w, uri, json), launch).0
}

#[tokio::test]
async fn a_launch_racing_a_watcher_leaves_the_watchers_session_alone() {
    let w = world();
    write_item(&w.ws, "T-40-race", "in_review", PR);
    set_pr(&w, "OPEN", "APPROVED", "MERGEABLE", "SUCCESS");

    let (status, body) = post_racing_a_launch(&w, "T-40-race", "/api/workitems/app/T-40-race/fix", r#"{"kind":"comments"}"#).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(field(&w.ws, "T-40-race", "agent_status"), "working", "the stale ok is not restored");
    settle(&w).await;
    assert!(w.tmux.has("wi-T-40-race"), "the dispatcher does not reap the watcher's session");
    assert!(stub_log(&w.repo).is_empty(), "no fixer ran");

    write_item(&w.ws, "T-41-race", "analyze", "");
    let (status, _) = post_racing_a_launch(&w, "T-41-race", "/api/workitems/app/T-41-race/session", "").await;
    assert_eq!((status, field(&w.ws, "T-41-race", "agent_status")), (StatusCode::CONFLICT, "working".into()));
}

#[tokio::test]
async fn workspaces_and_boards_are_added_from_the_ui_and_depends_on_names_items_of_the_workspace() {
    let w = world();
    let (status, _) = post(&w, "/api/workspaces", r#"{"name":"web"}"#).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(post(&w, "/api/workspaces", r#"{"name":"web"}"#).await.0, StatusCode::BAD_REQUEST, "taken");
    assert_eq!(post(&w, "/api/workspaces", r#"{"name":"../x"}"#).await.0, StatusCode::BAD_REQUEST);
    let listed = get(&w, "/api/workspaces").await;
    assert!(listed.contains(r#""name":"app""#) && listed.contains(r#""name":"web""#), "{listed}");

    let repo = serde_json::to_string(&w.repo.display().to_string()).unwrap();
    let (status, body) = post(&w, "/api/workspaces/app/boards", &format!(r#"{{"name":"ui","repo":{repo}}}"#)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(post(&w, "/api/workspaces/app/boards", &format!(r#"{{"name":"ui","repo":{repo}}}"#)).await.0, StatusCode::BAD_REQUEST, "taken");
    assert_eq!(post(&w, "/api/workspaces/app/boards", r#"{"name":"x","repo":"/no/such/repo"}"#).await.0, StatusCode::BAD_REQUEST);
    let boards: serde_json::Value = serde_json::from_str(&get(&w, "/api/pipelines").await).unwrap();
    let ui = boards["app"].as_array().unwrap().iter().find(|b| b["name"] == "ui").unwrap().clone();
    assert_eq!(ui["pipeline"]["entry"], "analyze", "a copy of the bundled board");
    assert_eq!(ui["pipeline"]["repo"], w.repo.display().to_string());

    write_item(&w.ws, "T-70-api", "analyze", "");
    write_item(&w.ws, "T-71-ui", "analyze", "board: ui\n");
    let edit = |deps: &str| serde_json::json!({ "fields": { "depends_on": deps } }).to_string();
    let (status, body) = post(&w, "/api/workitems/app/T-71-ui/frontmatter", &edit("T-70-api, T-99-gone")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("T-99-gone"), "{body}");
    assert_eq!(post(&w, "/api/workitems/app/T-71-ui/frontmatter", &edit("T-71-ui")).await.0, StatusCode::BAD_REQUEST, "not itself");
    assert_eq!(post(&w, "/api/workitems/app/T-71-ui/frontmatter", &edit("T-70-api")).await.0, StatusCode::NO_CONTENT, "added although the key was missing");
    let items: serde_json::Value = serde_json::from_str(&get(&w, "/api/workitems").await).unwrap();
    let ui_item = items.as_array().unwrap().iter().find(|i| i["dir"] == "T-71-ui").unwrap();
    assert_eq!((&ui_item["workspace"], &ui_item["depends_on"]), (&serde_json::json!("app"), &serde_json::json!(["T-70-api"])));
}

#[tokio::test]
async fn board_settings_are_edited_from_the_ui_and_checked_before_the_file_is_written() {
    let w = world();
    let file = w.ws.join("boards/default.json");
    let settings = r#"{"label":"Backend","entry":"analyze","order":["created desc"],"issue_project":"MNT","reviewers":"acme/web"}"#;
    let (status, body) = put(&w, "/api/workspaces/app/boards/default", settings).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let boards: serde_json::Value = serde_json::from_str(&get(&w, "/api/pipelines").await).unwrap();
    let board = boards["app"].as_array().unwrap().iter().find(|b| b["name"] == "default").unwrap()["pipeline"].clone();
    assert_eq!((&board["label"], &board["entry"], &board["order"]), (&serde_json::json!("Backend"), &serde_json::json!("analyze"), &serde_json::json!(["created desc"])));
    assert_eq!((&board["issue_project"], &board["reviewers"]), (&serde_json::json!("MNT"), &serde_json::json!("acme/web")));
    assert_eq!(board["repo"], w.repo.display().to_string());

    let saved = fs::read_to_string(&file).unwrap();
    for (bad, why) in [
        (r#"{"entry":"gone"}"#, "entry 'gone'"),
        (r#"{"entry":"analyze","order":["nope"]}"#, "nope"),
        (r#"{"entry":"analyze","issue_project":"acme/web"}"#, "Jira project key"),
        (r#"{"entry":"analyze","reviewers":"team"}"#, "org/team"),
    ] {
        let (status, body) = put(&w, "/api/workspaces/app/boards/default", bad).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
        assert!(body.contains(why), "{body}");
    }
    let (status, _) = put(&w, "/api/workspaces/app/boards/default", r#"{"entry":"analyze","repo":"/tmp"}"#).await;
    assert!(status.is_client_error(), "repo cannot be changed: {status}");
    assert_eq!(fs::read_to_string(&file).unwrap(), saved, "a refused change leaves the file as it was");
    assert_eq!(put(&w, "/api/workspaces/app/boards/nope", r#"{"entry":"analyze"}"#).await.0, StatusCode::NOT_FOUND);
    assert_eq!(put(&w, "/api/workspaces/nope/boards/default", r#"{"entry":"analyze"}"#).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn stage_settings_are_edited_from_the_ui_and_shown_in_the_stage_source() {
    let w = world();
    let file = w.ws.join("boards/default.json");
    let uri = "/api/workspaces/app/boards/default/stages/analyze";
    let (status, body) = put(&w, uri, r#"{"model":"sonnet","require_issue":true}"#).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let source: serde_json::Value = serde_json::from_str(&get(&w, &format!("{uri}/source")).await).unwrap();
    assert_eq!((&source["model"], &source["require_issue"]), (&serde_json::json!("sonnet"), &serde_json::json!(true)));

    assert_eq!(put(&w, uri, r#"{"model":"","require_issue":false}"#).await.0, StatusCode::NO_CONTENT);
    let source: serde_json::Value = serde_json::from_str(&get(&w, &format!("{uri}/source")).await).unwrap();
    assert_eq!((&source["model"], &source["require_issue"]), (&serde_json::Value::Null, &serde_json::json!(false)));

    let saved = fs::read_to_string(&file).unwrap();
    let (status, body) = put(&w, uri, r#"{"model":"--opus","require_issue":false}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(fs::read_to_string(&file).unwrap(), saved, "a refused change leaves the file as it was");
    let settings = r#"{"model":"","require_issue":false}"#;
    assert_eq!(put(&w, "/api/workspaces/app/boards/default/stages/nope", settings).await.0, StatusCode::NOT_FOUND);
    assert_eq!(put(&w, "/api/workspaces/app/boards/nope/stages/analyze", settings).await.0, StatusCode::NOT_FOUND);
    assert_eq!(put(&w, "/api/workspaces/nope/boards/default/stages/analyze", settings).await.0, StatusCode::NOT_FOUND);
}

/// One JSON-RPC message to an item's MCP endpoint (`questions.rs`), as an agent sends it: with the
/// Bearer token, not the cookie.
async fn mcp(w: &World, dir: &str, message: serde_json::Value) -> (StatusCode, serde_json::Value) {
    let request = Request::post(format!("/api/mcp/app/{dir}"))
        .header(header::HOST, format!("127.0.0.1:{}", w.state.port))
        .header(header::AUTHORIZATION, format!("Bearer {}", w.token))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .body(Body::from(message.to_string()))
        .unwrap();
    let response = router(w.state.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
}

/// `tools/call` of `tool`: (its text, isError).
async fn tool(w: &World, dir: &str, tool: &str, arguments: serde_json::Value) -> (String, bool) {
    let (status, reply) = mcp(w, dir, serde_json::json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": { "name": tool, "arguments": arguments } })).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    (reply["result"]["content"][0]["text"].as_str().unwrap().to_string(), reply["result"]["isError"].as_bool().unwrap())
}

fn status_of(w: &World, dir: &str) -> String {
    let text = fs::read_to_string(w.ws.join(dir).join("workitem.md")).unwrap();
    text.lines().find_map(|l| l.strip_prefix("agent_status: ")).unwrap().to_string()
}

#[tokio::test]
async fn the_mcp_endpoint_speaks_enough_mcp_for_an_agent() {
    let w = world();
    write_item(&w.ws, "T-70-item", "analyze", "");
    let (status, reply) = mcp(&w, "T-70-item", serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "1" } } })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["id"], 1);
    assert_eq!(reply["result"]["protocolVersion"], "2025-06-18", "the client's version is accepted");
    assert!(reply["result"]["capabilities"]["tools"].is_object());
    let (status, _) = mcp(&w, "T-70-item", serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "a notification gets no response");
    let (_, reply) = mcp(&w, "T-70-item", serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" })).await;
    let names: Vec<&str> = reply["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["ask_parent", "wait_for_answer", "list_child_questions", "answer_child"]);
    let (_, reply) = mcp(&w, "T-70-item", serde_json::json!({ "jsonrpc": "2.0", "id": 3, "method": "resources/list" })).await;
    assert_eq!(reply["error"]["code"], -32601);

    let (status, _) = mcp(&w, "T-71-missing", serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" })).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let request = Request::get("/api/mcp/app/T-70-item").header(header::HOST, format!("127.0.0.1:{}", w.state.port)).header(header::AUTHORIZATION, format!("Bearer {}", w.token)).body(Body::empty()).unwrap();
    assert_eq!(router(w.state.clone()).oneshot(request).await.unwrap().status(), StatusCode::METHOD_NOT_ALLOWED, "no SSE stream");
    let request = Request::post("/api/mcp/app/T-70-item").header(header::HOST, format!("127.0.0.1:{}", w.state.port)).body(Body::from("{}")).unwrap();
    assert_eq!(router(w.state.clone()).oneshot(request).await.unwrap().status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_child_asks_its_parent_and_gets_the_answer_back() {
    let w = world();
    write_item(&w.ws, "T-72-parent", "analyze", "answers_children: true\n");
    write_item(&w.ws, "T-73-child", "analyze", "parent: T-72-parent\n");
    write_item(&w.ws, "T-74-other", "analyze", "");

    let asking = {
        let w_state = w.state.clone();
        let token = w.token.clone();
        tokio::spawn(async move {
            let request = Request::post("/api/mcp/app/T-73-child")
                .header(header::HOST, format!("127.0.0.1:{}", w_state.port))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "ask_parent", "arguments": { "question": "Create a doc request\nfor the new param?", "recommendation": "No, internal only." } } }).to_string()))
                .unwrap();
            let response = router(w_state).oneshot(request).await.unwrap();
            let reply: serde_json::Value = serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
            reply["result"]["content"][0]["text"].as_str().unwrap().to_string()
        })
    };
    eventually("the child is blocked on its question", || status_of(&w, "T-73-child") == "blocked: asking parent: Create a doc request for the new param?").await;

    let (open, error) = tool(&w, "T-72-parent", "list_child_questions", serde_json::json!({})).await;
    assert!(!error);
    let open: serde_json::Value = serde_json::from_str(&open).unwrap();
    assert_eq!(open[0]["child"], "T-73-child");
    assert_eq!(open[0]["id"], 1);
    assert_eq!(open[0]["recommendation"], "No, internal only.");

    let (text, error) = tool(&w, "T-74-other", "answer_child", serde_json::json!({ "child": "T-73-child", "id": 1, "answer": "yes", "source": "x" })).await;
    assert!(error && text.contains("not a child"), "only the parent answers: {text}");
    let (_, error) = tool(&w, "T-72-parent", "answer_child", serde_json::json!({ "child": "T-73-child", "id": 1, "answer": "No: the param is internal.", "source": "the human" })).await;
    assert!(!error);

    assert_eq!(asking.await.unwrap(), "Answer to question 1 (from the human):\nNo: the param is internal.");
    assert_eq!(status_of(&w, "T-73-child"), "ok", "no session waits, so its handler runs again");
    let saved = fs::read_to_string(w.ws.join("T-73-child/questions.json")).unwrap();
    assert!(saved.contains("\"answered_at\""), "{saved}");
    let (text, error) = tool(&w, "T-72-parent", "answer_child", serde_json::json!({ "child": "T-73-child", "id": 1, "answer": "again", "source": "x" })).await;
    assert!(error && text.contains("already answered"), "{text}");
}

#[tokio::test]
async fn an_unanswered_question_can_be_waited_for_again() {
    let w = world();
    write_item(&w.ws, "T-75-parent", "analyze", "answers_children: true\n");
    write_item(&w.ws, "T-76-child", "analyze", "parent: T-75-parent\n");
    let (text, error) = tool(&w, "T-76-child", "ask_parent", serde_json::json!({ "question": "Which endpoint?" })).await;
    assert!(!error);
    assert_eq!(text, "No answer to question 1 yet. Call wait_for_answer with id 1 to keep waiting.");
    tool(&w, "T-75-parent", "answer_child", serde_json::json!({ "child": "T-76-child", "id": 1, "answer": "The batch one.", "source": "## Refinement" })).await;
    let (text, _) = tool(&w, "T-76-child", "wait_for_answer", serde_json::json!({ "id": 1 })).await;
    assert_eq!(text, "Answer to question 1 (from ## Refinement):\nThe batch one.");

    let (text, error) = tool(&w, "T-75-parent", "ask_parent", serde_json::json!({ "question": "x" })).await;
    assert!(error && text.contains("no parent"), "{text}");
    assert_eq!(status_of(&w, "T-75-parent"), "ok");

    write_item(&w.ws, "T-78-plain-parent", "analyze", "");
    write_item(&w.ws, "T-79-split", "analyze", "parent: T-78-plain-parent\n");
    let (text, error) = tool(&w, "T-79-split", "ask_parent", serde_json::json!({ "question": "x" })).await;
    assert!(error && text.contains("does not answer questions"), "a parent nothing answers for: {text}");
    assert_eq!(status_of(&w, "T-79-split"), "ok");
    assert!(!w.ws.join("T-79-split/questions.json").exists());
}

#[tokio::test]
async fn item_sessions_get_the_questions_server_with_the_token_in_a_private_file() {
    let w = world();
    write_item(&w.ws, "T-77-item", "analyze", "");
    let (status, _) = post(&w, "/api/workitems/app/T-77-item/session", "{}").await;
    assert_eq!(status, StatusCode::ACCEPTED);
    eventually("the session started", || args_log(&w).contains("mcp=")).await;
    let log = args_log(&w);
    let file = log.lines().find_map(|l| l.strip_prefix("mcp=")).unwrap();
    assert_eq!(fs::metadata(file).unwrap().permissions().mode() & 0o777, 0o600);
    let config: serde_json::Value = serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap();
    let server = &config["mcpServers"]["workitem"];
    assert_eq!(server["url"], format!("http://127.0.0.1:{}/api/mcp/app/T-77-item", w.state.port));
    assert_eq!(server["headers"]["Authorization"], format!("Bearer {}", w.token));
}
