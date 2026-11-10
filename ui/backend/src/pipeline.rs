//! The boards of a workspace: `<workspace>/boards/<name>.json`, each with its repo, its stages in
//! column order and what runs in each. `workitem-move` enforces the same file; this side only
//! reads it. The bundled `workitem-pipeline/pipeline.json` is the template for a new board.
use std::{collections::{HashMap, HashSet}, fs, path::{Path, PathBuf}, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{config::Workspace, AppState};

/// Handler value for the stage whose PR the backend polls (`review.rs`).
pub const PR_REVIEW: &str = "builtin:pr-review";

/// Handler whose approved items the "Start development" action can launch.
const DEVELOP: &str = "workitem-start";

/// A column where a human decides: approving moves the item to `approve_to`, rejecting to `reject_to`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Gate {
    pub label: String,
    pub approve_to: String,
    pub reject_to: String,
}

/// How often the watcher runs for a stage written the old way (`auto` and no `watch`).
const LEGACY_EVERY: Duration = Duration::from_secs(5);
/// Watchers that read facts for every item of a workspace in one call, so they run once per
/// workspace (`workspace.toml`, see `watch::workspace_watchers`) and never in a board file.
pub const WORKSPACE_ONLY: &[&str] = &["pr-status", "issue-status"];

/// A script the backend runs on a timer for a column, whether or not items are in it.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Watch {
    /// How often it runs: a number and `s`, `m`, `h` or `d`, like `30s` or `1d`.
    #[serde(deserialize_with = "every", serialize_with = "every_text")]
    pub every: Duration,
    /// Script name (`[a-z0-9._-]+`), see `watch::resolve`.
    pub command: String,
    /// Arguments for the script, passed as they are.
    #[serde(default)]
    pub args: Vec<String>,
    /// Not in the file: added for a stage written the old way, or `pr-status` for a workspace.
    #[serde(skip_deserializing)]
    pub implicit: bool,
}

/// `30s`, `10m`, `2h` or `1d`; never zero.
pub fn parse_every(text: &str) -> Result<Duration, String> {
    let bad = || format!("'{text}' is not an interval like 30s, 10m, 2h or 1d");
    let unit = text.chars().last().ok_or_else(bad)?;
    let number: u64 = text[..text.len() - unit.len_utf8()].parse().map_err(|_| bad())?;
    let seconds = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86400,
        _ => return Err(bad()),
    };
    match number.checked_mul(seconds) {
        Some(total) if total > 0 => Ok(Duration::from_secs(total)),
        _ => Err(bad()),
    }
}

pub fn watchers<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<Watch>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(Watch),
        Many(Vec<Watch>),
    }
    Ok(match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(watch) => vec![watch],
        OneOrMany::Many(list) => list,
    })
}

fn every<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
    parse_every(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
}

/// The largest unit that divides `every` evenly: 60s is `1m`, 90s stays `90s`.
fn every_text<S: serde::Serializer>(every: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
    let seconds = every.as_secs();
    let (count, unit) = [(86400, 'd'), (3600, 'h'), (60, 'm')].into_iter().find(|(size, _)| seconds.is_multiple_of(*size)).map_or((seconds, 's'), |(size, unit)| (seconds / size, unit));
    serializer.serialize_str(&format!("{count}{unit}"))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Stage {
    pub name: String,
    /// Column title; the name with `_` as a space when absent.
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub handler: Option<String>,
    /// Inline instructions, instead of a `handler` skill.
    #[serde(default, skip_serializing)]
    pub prompt: Option<String>,
    /// `--model` of every session of an item in this stage (decision 60); the agent's default when absent.
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub auto: bool,
    #[serde(default)]
    pub next: Vec<String>,
    #[serde(default)]
    pub gate: Option<Gate>,
    /// On the `PR_REVIEW` stage: where an item goes once its PR is merged.
    #[serde(default)]
    pub on_merge: Option<String>,
    /// At most this many items of the board can be in the stage.
    #[serde(default)]
    pub limit: Option<usize>,
    /// How this column is sorted, like the board's `order`, which it replaces when not empty.
    #[serde(default)]
    pub order: Vec<String>,
    /// Read the PRs of the board with the workspace's `pr-status` watcher, for the chips, even
    /// without a stage that reacts to them. Older files; a workspace now lists `pr-status` itself.
    #[serde(default, skip_serializing)]
    pub pr_status: bool,
    /// Start the handler at most once per entry into the stage.
    #[serde(default)]
    pub once: bool,
    /// Give the handler the board's learnings (`develops` stages always get them).
    #[serde(default, skip_serializing)]
    pub learnings: bool,
    /// Start the handler only for items whose frontmatter has all of these values.
    #[serde(default, skip_serializing)]
    pub when: HashMap<String, String>,
    /// Scripts that run for this column on a timer (see `watch.rs`): one object or a list.
    #[serde(default, skip_serializing, deserialize_with = "watchers")]
    pub watch: Vec<Watch>,
}

impl Stage {
    /// The stage whose PRs are watched, and acted on, by the `pr-review` script.
    pub fn watches_prs(&self) -> bool {
        self.handler.as_deref() == Some(PR_REVIEW) || self.watch.iter().any(|w| w.command == "pr-review")
    }

    /// Approved items here are developed by the start handler.
    pub fn develops(&self) -> bool {
        self.handler.as_deref() == Some(DEVELOP)
    }
}

/// Item fields a board's `order` can name; the frontend sorts by them.
const ORDER_FIELDS: &[&str] = &["issue_status_category", "issue_status", "issue", "title", "kind", "agent_status", "created", "dir"];

/// One `order` entry: a field of `ORDER_FIELDS`, optionally followed by `asc` or `desc`.
fn valid_order(entry: &str) -> bool {
    matches!(entry.split_whitespace().collect::<Vec<_>>()[..], [field] | [field, "asc" | "desc"] if ORDER_FIELDS.contains(&field))
}

/// A board's `repo`: `~/` is the home folder.
fn repo_path<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<PathBuf>, D::Error> {
    let text = String::deserialize(deserializer)?;
    Ok(Some(match (text.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest),
        _ => PathBuf::from(text),
    }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Pipeline {
    /// The repo every item of the board belongs to (decision 45). A board without one (an intake
    /// board that writes no code) has no worktrees and no PRs; its sessions start in the item folder
    /// with every board's repo added.
    #[serde(default, deserialize_with = "repo_path", skip_serializing_if = "Option::is_none")]
    pub repo: Option<PathBuf>,
    /// Tab title; the board's name (its file name) when absent.
    #[serde(default)]
    pub label: Option<String>,
    pub entry: String,
    pub stages: Vec<Stage>,
    /// Scripts that run for the whole board on a timer, with every item of the board on stdin.
    #[serde(default, skip_serializing, deserialize_with = "watchers")]
    pub watch: Vec<Watch>,
    /// How each column is sorted: item fields, first one first, each optionally followed by `asc` or
    /// `desc` (`"issue_status desc"`); title breaks ties. Empty means title only.
    #[serde(default)]
    pub order: Vec<String>,
    /// Required by `workitem-move`, so a file without it is invalid here too.
    #[serde(skip_serializing)]
    #[allow(dead_code)]
    brief_sections: Vec<String>,
}

impl Pipeline {
    pub fn stage(&self, name: &str) -> Option<&Stage> {
        self.stages.iter().find(|stage| stage.name == name)
    }

    /// Its PRs must be read: a stage reacts to them, or asks for their chips with `pr_status`.
    pub fn reads_prs(&self) -> bool {
        self.stages.iter().any(|s| s.pr_status || s.watches_prs())
    }

    pub fn parse(text: &str) -> Result<Pipeline, String> {
        let mut pipeline: Pipeline = serde_json::from_str(text).map_err(|e| e.to_string())?;
        // Older files have no `watch`: `auto` plus a handler or prompt is `run-items`, `auto` with
        // nothing to run a queue (`pull-queue`) and the PR handler is `pr-review`.
        let legacy = |command: &str, every| Watch { every, command: command.into(), args: vec![], implicit: true };
        for stage in pipeline.stages.iter_mut().filter(|s| s.watch.is_empty()) {
            if stage.auto {
                match (stage.handler.as_deref(), stage.prompt.as_deref()) {
                    (None, None) => stage.watch.push(legacy("pull-queue", LEGACY_EVERY)),
                    (Some(PR_REVIEW), _) => stage.watch.push(legacy("pr-review", LEGACY_EVERY)),
                    (Some(handler), _) if !handler.starts_with("builtin:") => stage.watch.push(legacy("run-items", LEGACY_EVERY)),
                    (None, Some(_)) => stage.watch.push(legacy("run-items", LEGACY_EVERY)),
                    _ => {}
                }
            }
        }
        let all = pipeline.watch.iter().chain(pipeline.stages.iter().flat_map(|s| &s.watch));
        if let Some(watch) = all.into_iter().find(|w| WORKSPACE_ONLY.contains(&w.command.as_str())) {
            return Err(format!("'{}' runs once per workspace: list it in workspace.toml, not in a board", watch.command));
        }
        match &pipeline.repo {
            Some(repo) if !repo.is_absolute() || !repo.is_dir() => {
                return Err(format!("repo '{}' is not an absolute path to a directory", repo.display()));
            }
            None if pipeline.reads_prs() => {
                return Err("a board without a repo has no PRs, so no PR stage and no pr_status".into());
            }
            _ => {}
        }
        let names: HashSet<&str> = pipeline.stages.iter().map(|s| s.name.as_str()).collect();
        if names.contains("") {
            return Err("every stage needs a name".into());
        }
        if names.len() != pipeline.stages.len() {
            return Err("stage names must be unique".into());
        }
        if !names.contains(pipeline.entry.as_str()) {
            return Err(format!("entry '{}' is not a stage", pipeline.entry));
        }
        if let Some(watch) = pipeline.watch.iter().find(|w| !crate::watch::valid_name(&w.command)) {
            return Err(format!("the board has watch command '{}', which is not a script name (lowercase letters, digits, . _ - only)", watch.command));
        }
        let orders = std::iter::once(&pipeline.order).chain(pipeline.stages.iter().map(|s| &s.order));
        if let Some(entry) = orders.flatten().find(|entry| !valid_order(entry)) {
            return Err(format!("order has '{entry}', which is not a field ({}) optionally followed by asc or desc", ORDER_FIELDS.join(", ")));
        }
        for stage in &pipeline.stages {
            if let Some(target) = stage.next.iter().find(|t| !names.contains(t.as_str())) {
                return Err(format!("'{}' has next '{target}', which is not a stage", stage.name));
            }
            // `workitem-move` only allows moves listed in `next`, so a target outside it could never work.
            let targets = stage.gate.iter().flat_map(|g| [("approve_to", &g.approve_to), ("reject_to", &g.reject_to)]);
            for (key, target) in targets.chain(stage.on_merge.iter().map(|t| ("on_merge", t))) {
                if !stage.next.contains(target) {
                    return Err(format!("'{}' has {key} '{target}', which is not in its next", stage.name));
                }
            }
            if stage.handler.is_some() && stage.prompt.is_some() {
                return Err(format!("'{}' has both a handler and a prompt", stage.name));
            }
            if let Some(model) = stage.model.as_deref().filter(|m| !valid_model(m)) {
                return Err(format!("'{}' has model '{model}', which is not a model name (letters, digits, . _ : - [ ], not starting with -)", stage.name));
            }
            if stage.limit == Some(0) {
                return Err(format!("'{}' has limit 0, which no item could enter", stage.name));
            }
            for watch in &stage.watch {
                if !crate::watch::valid_name(&watch.command) {
                    return Err(format!("'{}' has watch command '{}', which is not a script name (lowercase letters, digits, . _ - only)", stage.name, watch.command));
                }
            }
            if stage.on_merge.is_some() != stage.watches_prs() {
                return Err(format!("'{}': on_merge goes with the handler {PR_REVIEW}, and only with it", stage.name));
            }
        }
        if pipeline.stages.iter().filter(|s| s.watches_prs()).count() > 1 {
            return Err(format!("at most one stage can have the handler {PR_REVIEW}"));
        }
        Ok(pipeline)
    }
}

/// What one board of a workspace looks like right now.
#[derive(Clone, Serialize)]
pub struct Resolved {
    /// The newest valid file. `None` only when none was ever valid.
    pub pipeline: Option<Pipeline>,
    /// Why the file on disk is not in use, if so.
    pub error: Option<String>,
}

/// A model name or alias for `--model`. It is one argv element, never seen by a shell; this
/// only catches typos, and a leading `-` that would read as a flag.
fn valid_model(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('-') && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-' | b'[' | b']'))
}

/// Board names become file names, so only plain ones are accepted.
pub fn valid_board_name(name: &str) -> bool {
    crate::config::valid_name(name)
}

fn read(workspace: &Workspace, board: &str) -> Result<Pipeline, String> {
    if !valid_board_name(board) {
        return Err(format!("'{board}' is not a board name"));
    }
    let path = workspace.dir.join("boards").join(format!("{board}.json"));
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Pipeline::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

impl AppState {
    /// Reads a board of the workspace. When its file is invalid the last valid one stays in use, so
    /// a typo never stops the dispatcher or empties the board.
    pub fn pipeline(&self, workspace: &Workspace, board: &str) -> Resolved {
        let key = format!("{}/{board}", workspace.name);
        let mut known = self.pipelines.lock().unwrap();
        match read(workspace, board) {
            Ok(pipeline) => {
                known.insert(key, pipeline.clone());
                Resolved { pipeline: Some(pipeline), error: None }
            }
            Err(error) => {
                eprintln!("workitem-ui: {key}: {error}");
                Resolved { pipeline: known.get(&key).cloned(), error: Some(error) }
            }
        }
    }

    /// Boards of the workspace, one per `boards/<name>.json`, by name.
    pub fn board_names(&self, workspace: &Workspace) -> Vec<String> {
        let files = fs::read_dir(workspace.dir.join("boards")).into_iter().flatten().flatten();
        let mut names: Vec<String> = files
            .filter_map(|entry| entry.file_name().to_str()?.strip_suffix(".json").map(String::from))
            .filter(|name| valid_board_name(name))
            .collect();
        names.sort();
        names
    }

    /// The repo of `board`, from its last valid file; `None` for a board without one.
    pub fn repo_of(&self, workspace: &Workspace, board: &str) -> Result<Option<PathBuf>, String> {
        self.pipeline(workspace, board).pipeline.map(|p| p.repo).ok_or_else(|| format!("board '{board}' has no valid file"))
    }

    /// The distinct repos of the workspace's boards, for sessions that are not tied to one board.
    pub fn repos(&self, workspace: &Workspace) -> Vec<PathBuf> {
        let mut repos: Vec<PathBuf> = self.board_names(workspace).iter().filter_map(|b| self.repo_of(workspace, b).ok().flatten()).collect();
        repos.sort();
        repos.dedup();
        repos
    }

    /// Writes `boards/<name>.json` as a copy of the bundled board with `repo` set.
    pub fn add_board(&self, workspace: &Workspace, name: &str, repo: &str) -> Result<(), String> {
        if !valid_board_name(name) {
            return Err("a board name uses letters, digits, - and _ only".into());
        }
        let repo = Path::new(repo.trim());
        if !repo.is_absolute() || !repo.is_dir() {
            return Err(format!("{} is not an existing absolute directory", repo.display()));
        }
        let repo = repo.canonicalize().map_err(|e| e.to_string())?;
        let path = workspace.dir.join("boards").join(format!("{name}.json"));
        if path.exists() {
            return Err(format!("board '{name}' already exists"));
        }
        let template = fs::read_to_string(self.skills_dir.join("workitem-pipeline/pipeline.json")).map_err(|e| e.to_string())?;
        let text = with_repo(&template, &repo)?;
        Pipeline::parse(&text)?;
        fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::write(&path, text).map_err(|e| e.to_string())
    }
}

/// The board file `template` with `"repo"` as its first key, keeping the rest of the text as it is.
fn with_repo(template: &str, repo: &Path) -> Result<String, String> {
    let rest = template.trim_start().strip_prefix('{').ok_or("the bundled board is not a JSON object")?;
    let repo = serde_json::to_string(&repo.display().to_string()).map_err(|e| e.to_string())?;
    Ok(format!("{{\n  \"repo\": {repo},{rest}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BRIEF: &str = r#""repo": "/", "brief_sections": []"#;

    fn parse(stages: &str, entry: &str) -> Result<Pipeline, String> {
        Pipeline::parse(&format!(r#"{{ {BRIEF}, "entry": "{entry}", "stages": {stages} }}"#))
    }

    #[test]
    fn keeps_stage_order_and_reads_the_optional_keys() {
        let pipeline = parse(r#"[{"name":"b","next":["a"]},{"name":"a","label":"A","handler":null,"auto":true}]"#, "b").unwrap();
        assert_eq!(pipeline.stages.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["b", "a"]);
        assert_eq!(pipeline.stage("a").unwrap().label.as_deref(), Some("A"));
        assert!(pipeline.stage("a").unwrap().auto);
        assert!(pipeline.stage("zzz").is_none());
    }

    #[test]
    fn reads_the_stage_model_and_rejects_what_is_not_a_model_name() {
        let with = |model: &str| parse(&format!(r#"[{{"name":"a","model":"{model}"}}]"#), "a");
        for model in ["sonnet", "claude-opus-5-5", "sonnet[1m]", "us.anthropic.claude:v1_2"] {
            assert_eq!(with(model).unwrap().stage("a").unwrap().model.as_deref(), Some(model));
        }
        assert!(parse(r#"[{"name":"a"}]"#, "a").unwrap().stage("a").unwrap().model.is_none());
        for model in ["", "--opus", "son net", "opus;rm"] {
            assert!(with(model).unwrap_err().contains("model"), "{model}");
        }
    }

    #[test]
    fn rejects_files_the_board_cannot_use() {
        assert!(Pipeline::parse("{").is_err());
        assert!(parse(r#"[{"name":"a"}]"#, "b").unwrap_err().contains("entry"));
        assert!(parse(r#"[{"name":"a"},{"name":"a"}]"#, "a").unwrap_err().contains("unique"));
        assert!(parse(r#"[{"name":""}]"#, "").unwrap_err().contains("name"));
        assert!(parse(r#"[{"name":"a","next":["gone"]}]"#, "a").unwrap_err().contains("gone"));
        assert!(Pipeline::parse(r#"{"entry":"a","stages":[{"name":"a"}]}"#).is_err());
    }

    #[test]
    fn reads_the_column_order_and_rejects_unknown_fields() {
        let with = |order: &str| Pipeline::parse(&format!(r#"{{ {BRIEF}, "entry": "a", "stages": [{{"name":"a"}}], "order": {order} }}"#));
        assert_eq!(with(r#"["issue_status_category desc", "title asc", "created"]"#).unwrap().order, ["issue_status_category desc", "title asc", "created"]);
        assert!(parse(r#"[{"name":"a"}]"#, "a").unwrap().order.is_empty());
        assert!(with(r#"["nope"]"#).unwrap_err().contains("nope"));
        assert!(with(r#"["title sideways"]"#).unwrap_err().contains("sideways"));
        assert!(with(r#"[""]"#).is_err());
        let stage = parse(r#"[{"name":"a","order":["issue_status desc"]}]"#, "a").unwrap();
        assert_eq!(stage.stage("a").unwrap().order, ["issue_status desc"]);
        assert!(parse(r#"[{"name":"a","order":["nope"]}]"#, "a").unwrap_err().contains("nope"));
    }

    #[test]
    fn checks_the_roles_of_gates_and_the_pr_stage() {
        let gate = |to: &str| format!(r#"[{{"name":"a","next":["b"],"gate":{{"label":"Ok","approve_to":"{to}","reject_to":"b"}}}},{{"name":"b"}}]"#);
        let pipeline = parse(&gate("b"), "a").unwrap();
        assert_eq!(pipeline.stage("a").unwrap().gate.as_ref().unwrap().label, "Ok");
        assert!(parse(&gate("a"), "a").unwrap_err().contains("approve_to"));

        let pr = r#"{"name":"a","handler":"builtin:pr-review","next":["b"],"on_merge":"b"}"#;
        let stage = parse(&format!(r#"[{pr},{{"name":"b"}}]"#), "a").unwrap().stage("a").unwrap().clone();
        assert!(stage.watches_prs() && !stage.develops());
        let no_merge = r#"[{"name":"a","handler":"builtin:pr-review","next":["b"]},{"name":"b"}]"#;
        assert!(parse(no_merge, "a").unwrap_err().contains("on_merge"));
        let stray = r#"[{"name":"a","handler":"x","next":["b"],"on_merge":"b"},{"name":"b"}]"#;
        assert!(parse(stray, "a").unwrap_err().contains("on_merge"));
        let two = format!(r#"[{pr},{},{{"name":"b"}}]"#, pr.replace(r#""a""#, r#""c""#));
        assert!(parse(&two, "a").unwrap_err().contains("at most one"));
    }

    #[test]
    fn a_stage_has_a_handler_or_a_prompt_not_both() {
        let own = parse(r#"[{"name":"a","prompt":"Do the thing."},{"name":"b","handler":"x"}]"#, "a").unwrap();
        assert_eq!(own.stage("a").unwrap().prompt.as_deref(), Some("Do the thing."));
        assert!(parse(r#"[{"name":"a","handler":"x","prompt":"y"}]"#, "a").unwrap_err().contains("both"));
    }

    #[test]
    fn reads_limit_once_and_when() {
        let pipeline = parse(r#"[{"name":"a","limit":1,"once":true,"when":{"approved":"true"}},{"name":"b"}]"#, "a").unwrap();
        let stage = pipeline.stage("a").unwrap();
        assert_eq!((stage.limit, stage.once), (Some(1), true));
        assert_eq!(stage.when["approved"], "true");
        let plain = pipeline.stage("b").unwrap();
        assert_eq!((plain.limit, plain.once, plain.when.len()), (None, false, 0));
        assert!(parse(r#"[{"name":"a","limit":0}]"#, "a").unwrap_err().contains("limit"));
        assert!(parse(r#"[{"name":"a","pr_status":true}]"#, "a").unwrap().stage("a").unwrap().pr_status);
    }

    #[test]
    fn reads_a_watch_script_and_its_interval() {
        let pipeline = parse(r#"[{"name":"a","watch":{"every":"2h","command":"prune"}},{"name":"b"}]"#, "a").unwrap();
        let watch = &pipeline.stage("a").unwrap().watch[0];
        assert_eq!((watch.every, watch.command.as_str()), (Duration::from_secs(7200), "prune"));
        assert!(pipeline.stage("b").unwrap().watch.is_empty());
        for every in ["0s", "10", "m", "1w", "-5m", "", "1.5h"] {
            assert!(parse(&format!(r#"[{{"name":"a","watch":{{"every":"{every}","command":"x"}}}}]"#), "a").is_err(), "{every}");
        }
        assert!(parse(r#"[{"name":"a","watch":{"every":"1d","command":"../x"}}]"#, "a").unwrap_err().contains("script name"));
        assert!(parse(r#"[{"name":"a","watch":{"every":"1d"}}]"#, "a").is_err());
    }

    #[test]
    fn an_auto_stage_with_nothing_to_run_is_a_queue_watched_by_pull_queue() {
        let pipeline = parse(r#"[{"name":"q","auto":true,"next":["w"]},{"name":"w","handler":"h","auto":true},{"name":"m"}]"#, "q").unwrap();
        assert_eq!(pipeline.stage("q").unwrap().watch[0].command, "pull-queue");
        assert_eq!(pipeline.stage("w").unwrap().watch[0].command, "run-items");
        assert!(pipeline.stage("m").unwrap().watch.is_empty());
        let own = parse(r#"[{"name":"q","auto":true,"watch":{"every":"1h","command":"mine"}}]"#, "q").unwrap();
        assert_eq!(own.stage("q").unwrap().watch[0].command, "mine");
    }

    #[test]
    fn an_auto_stage_with_a_handler_or_prompt_is_run_by_run_items() {
        let pipeline = parse(r#"[{"name":"a","handler":"h","auto":true},{"name":"b","prompt":"p","auto":true},{"name":"c","handler":"builtin:pr-review","auto":true,"on_merge":"a","next":["a"]},{"name":"d","handler":"h"}]"#, "a").unwrap();
        let commands = |name: &str| pipeline.stage(name).unwrap().watch.iter().map(|w| w.command.as_str()).collect::<Vec<_>>();
        assert_eq!((commands("a"), commands("b")), (vec!["run-items"], vec!["run-items"]));
        assert_eq!(commands("c"), vec!["pr-review"], "the PR handler is the pr-review script");
        assert!(commands("d").is_empty(), "a manual stage is not watched");
        assert!(pipeline.watch.is_empty() && pipeline.reads_prs(), "its PRs are read by the workspace, not the board");
    }

    #[test]
    fn watch_is_one_object_or_a_list_and_pr_status_asks_for_the_reader() {
        let list = parse(r#"[{"name":"a","watch":[{"every":"5s","command":"x"},{"every":"1m","command":"y","args":["-z"]}]}]"#, "a").unwrap();
        assert_eq!(list.stage("a").unwrap().watch.iter().map(|w| w.command.as_str()).collect::<Vec<_>>(), ["x", "y"]);
        assert!(!list.reads_prs(), "no PR stage, no reader");
        let status = parse(r#"[{"name":"a","handler":"h","auto":true,"pr_status":true}]"#, "a").unwrap();
        assert_eq!(status.stage("a").unwrap().watch.iter().map(|w| w.command.as_str()).collect::<Vec<_>>(), ["run-items"]);
        assert!(status.reads_prs());
    }

    #[test]
    fn workspace_watchers_are_refused_in_a_board() {
        for command in WORKSPACE_ONLY {
            let on_stage = parse(&format!(r#"[{{"name":"a","watch":{{"every":"1m","command":"{command}"}}}}]"#), "a");
            assert!(on_stage.unwrap_err().contains("workspace.toml"), "{command}");
            let on_board = Pipeline::parse(&format!(r#"{{ {BRIEF}, "entry": "a", "stages": [{{"name":"a"}}], "watch": {{"every":"1m","command":"{command}"}} }}"#));
            assert!(on_board.unwrap_err().contains("workspace.toml"), "{command}");
        }
    }

    #[test]
    fn every_is_written_in_its_largest_whole_unit() {
        let text = |every: &str| serde_json::to_value(Watch { every: parse_every(every).unwrap(), command: "x".into(), args: vec![], implicit: false }).unwrap()["every"].clone();
        assert_eq!([text("60s"), text("90s"), text("120m"), text("1d")], ["1m", "90s", "2h", "1d"]);
    }

    #[test]
    fn the_bundled_file_is_a_valid_template() {
        let text = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../skills/workitem-pipeline/pipeline.json")).unwrap();
        let pipeline = Pipeline::parse(&with_repo(&text, Path::new("/")).unwrap()).unwrap();
        assert_eq!(pipeline.entry, "backlog");
        assert_eq!(pipeline.stages.len(), 6);
        assert!(pipeline.stage("ready_for_development").unwrap().gate.is_some());
        assert!(pipeline.stage("in_review").unwrap().watches_prs());
        assert!(pipeline.stage("in_development").unwrap().develops());
        assert!(pipeline.watch.is_empty() && pipeline.reads_prs(), "the workspace reads the bundled board's PRs");
    }

    #[test]
    fn a_board_needs_an_existing_absolute_repo_and_expands_the_home_folder() {
        let with = |repo: &str| Pipeline::parse(&format!(r#"{{"repo": "{repo}", "brief_sections": [], "entry": "a", "stages": [{{"name":"a"}}]}}"#));
        assert_eq!(with("/").unwrap().repo.as_deref(), Some(Path::new("/")));
        assert_eq!(with("/").unwrap().label, None);
        let labelled = Pipeline::parse(r#"{"repo": "/", "label": "Storyrails - Develop", "brief_sections": [], "entry": "a", "stages": [{"name":"a"}]}"#);
        assert_eq!(labelled.unwrap().label.as_deref(), Some("Storyrails - Develop"));
        assert!(with("relative").unwrap_err().contains("repo"));
        assert!(with("/no/such/dir").unwrap_err().contains("repo"));
        let home = std::env::var("HOME").unwrap();
        assert_eq!(with("~/").unwrap().repo, Some(Path::new(&home).join("")));
    }

    #[test]
    fn a_board_without_a_repo_has_no_prs() {
        let without = |stages: &str, watch: &str| Pipeline::parse(&format!(r#"{{"brief_sections": [], "entry": "a", "stages": {stages}{watch}}}"#));
        assert_eq!(without(r#"[{"name":"a","handler":"x","auto":true}]"#, "").unwrap().repo, None);
        let reviews = r#"[{"name":"a","next":["b"],"watch":{"every":"5s","command":"pr-review"},"on_merge":"b"},{"name":"b"}]"#;
        assert!(without(reviews, "").unwrap_err().contains("without a repo"));
        assert!(without(r#"[{"name":"a","pr_status":true}]"#, "").unwrap_err().contains("without a repo"));
    }

    #[test]
    fn a_new_board_is_the_template_with_its_repo_first() {
        let template = "{\n  \"entry\": \"a\",\n  \"brief_sections\": [],\n  \"stages\": [{ \"name\": \"a\" }]\n}\n";
        let text = with_repo(template, Path::new("/")).unwrap();
        assert!(text.starts_with("{\n  \"repo\": \"/\",\n  \"entry\""), "{text}");
        assert_eq!(Pipeline::parse(&text).unwrap().repo.as_deref(), Some(Path::new("/")));
    }
}
