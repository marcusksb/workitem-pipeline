//! The board of a project: its stages in column order and what runs in each. Read from
//! `<workitems dir>/pipeline.json`, or the bundled `workitem-pipeline/pipeline.json` when the
//! project has none. `workitem-move` enforces the same file; this side only reads it.
use std::{collections::{HashMap, HashSet}, fs, path::Path, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{config::Project, workitem, AppState};

/// The board of items without a `board:` key: `boards/default.json`, else `pipeline.json`, else the bundled file.
pub const DEFAULT_BOARD: &str = "default";

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
const LEGACY_PR_EVERY: Duration = Duration::from_secs(60);
const LEGACY_PR_STATUS_EVERY: Duration = Duration::from_secs(300);

/// A script the backend runs on a timer for a column, whether or not items are in it.
#[derive(Clone, Debug, Deserialize)]
pub struct Watch {
    /// How often it runs: a number and `s`, `m`, `h` or `d`, like `30s` or `1d`.
    #[serde(deserialize_with = "every")]
    pub every: Duration,
    /// Script name (`[a-z0-9._-]+`), see `watch::resolve`.
    pub command: String,
    /// Arguments for the script, passed as they are.
    #[serde(default)]
    pub args: Vec<String>,
    /// Not in the file: added for a stage written the old way.
    #[serde(skip)]
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

fn watchers<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<Watch>, D::Error> {
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
    /// Show the PR status chips on the cards of this stage. Polls the PR read-only: no fixers, no
    /// merge handling (that is `builtin:pr-review`, which already shows them).
    #[serde(default, skip_serializing)]
    pub pr_status: bool,
    /// Start the handler at most once per entry into the stage.
    #[serde(default)]
    pub once: bool,
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

    /// The stage shows PR status chips on its cards, read by the `pr-status` script.
    pub fn shows_pr_status(&self) -> bool {
        self.pr_status || self.watch.iter().any(|w| w.command == "pr-status")
    }

    /// Approved items here are developed by the start handler.
    pub fn develops(&self) -> bool {
        self.handler.as_deref() == Some(DEVELOP)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Pipeline {
    pub entry: String,
    pub stages: Vec<Stage>,
    /// Required by `workitem-move`, so a file without it is invalid here too.
    #[serde(skip_serializing)]
    #[allow(dead_code)]
    brief_sections: Vec<String>,
}

impl Pipeline {
    pub fn stage(&self, name: &str) -> Option<&Stage> {
        self.stages.iter().find(|stage| stage.name == name)
    }

    pub fn parse(text: &str) -> Result<Pipeline, String> {
        let mut pipeline: Pipeline = serde_json::from_str(text).map_err(|e| e.to_string())?;
        // Older files have no `watch`: `auto` plus a handler or prompt is `run-items`, `auto` with
        // nothing to run a queue (`pull-queue`), the PR handler is `pr-review`, and `pr_status`
        // adds `pr-status`.
        for stage in pipeline.stages.iter_mut().filter(|s| s.watch.is_empty()) {
            let legacy = |command: &str, every| Watch { every, command: command.into(), args: vec![], implicit: true };
            if stage.auto {
                match (stage.handler.as_deref(), stage.prompt.as_deref()) {
                    (None, None) => stage.watch.push(legacy("pull-queue", LEGACY_EVERY)),
                    (Some(PR_REVIEW), _) => stage.watch.push(legacy("pr-review", LEGACY_PR_EVERY)),
                    (Some(handler), _) if !handler.starts_with("builtin:") => stage.watch.push(legacy("run-items", LEGACY_EVERY)),
                    (None, Some(_)) => stage.watch.push(legacy("run-items", LEGACY_EVERY)),
                    _ => {}
                }
            }
            if stage.pr_status {
                stage.watch.push(legacy("pr-status", LEGACY_PR_STATUS_EVERY));
            }
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

/// What the board of one project looks like right now.
#[derive(Clone, Serialize)]
pub struct Resolved {
    /// The newest valid file. `None` only when none was ever valid.
    pub pipeline: Option<Pipeline>,
    /// Why the file on disk is not in use, if so.
    pub error: Option<String>,
}

/// Board names become file names, so only plain ones are accepted.
pub fn valid_board_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn read(skills_dir: &Path, project: &Project, board: &str) -> Result<Pipeline, String> {
    let dir = workitem::workitems_dir(&project.repo);
    let path = if !valid_board_name(board) {
        return Err(format!("'{board}' is not a board name"));
    } else if board == DEFAULT_BOARD {
        // The project's own default board: `boards/default.json`, else `pipeline.json`, else the bundled one.
        [dir.join("boards/default.json"), dir.join("pipeline.json")]
            .into_iter()
            .find(|own| own.is_file())
            .unwrap_or_else(|| skills_dir.join("workitem-pipeline/pipeline.json"))
    } else {
        dir.join("boards").join(format!("{board}.json"))
    };
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Pipeline::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

impl AppState {
    /// Reads a board of the project. When its file is invalid the last valid one stays in use, so
    /// a typo never stops the dispatcher or empties the board.
    pub fn pipeline(&self, project: &Project, board: &str) -> Resolved {
        let key = format!("{}/{board}", project.name);
        let mut known = self.pipelines.lock().unwrap();
        match read(&self.skills_dir, project, board) {
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

    /// Boards of the project: `default` first, then one per `boards/<name>.json`, by name.
    pub fn board_names(&self, project: &Project) -> Vec<String> {
        let files = fs::read_dir(workitem::workitems_dir(&project.repo).join("boards")).into_iter().flatten().flatten();
        let mut names: Vec<String> = files
            .filter_map(|entry| entry.file_name().to_str()?.strip_suffix(".json").map(String::from))
            .filter(|name| valid_board_name(name) && name != DEFAULT_BOARD)
            .collect();
        names.sort();
        names.insert(0, DEFAULT_BOARD.into());
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BRIEF: &str = r#""brief_sections": []"#;

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
    fn rejects_files_the_board_cannot_use() {
        assert!(Pipeline::parse("{").is_err());
        assert!(parse(r#"[{"name":"a"}]"#, "b").unwrap_err().contains("entry"));
        assert!(parse(r#"[{"name":"a"},{"name":"a"}]"#, "a").unwrap_err().contains("unique"));
        assert!(parse(r#"[{"name":""}]"#, "").unwrap_err().contains("name"));
        assert!(parse(r#"[{"name":"a","next":["gone"]}]"#, "a").unwrap_err().contains("gone"));
        assert!(Pipeline::parse(r#"{"entry":"a","stages":[{"name":"a"}]}"#).is_err());
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
    }

    #[test]
    fn watch_is_one_object_or_a_list_and_pr_status_adds_its_script() {
        let list = parse(r#"[{"name":"a","watch":[{"every":"5s","command":"x"},{"every":"1m","command":"y","args":["-z"]}]}]"#, "a").unwrap();
        assert_eq!(list.stage("a").unwrap().watch.iter().map(|w| w.command.as_str()).collect::<Vec<_>>(), ["x", "y"]);
        let status = parse(r#"[{"name":"a","handler":"h","auto":true,"pr_status":true}]"#, "a").unwrap();
        let stage = status.stage("a").unwrap();
        assert_eq!(stage.watch.iter().map(|w| w.command.as_str()).collect::<Vec<_>>(), ["run-items", "pr-status"]);
        assert!(stage.shows_pr_status() && !stage.watches_prs());
    }

    #[test]
    fn the_bundled_file_is_valid() {
        let text = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../skills/workitem-pipeline/pipeline.json")).unwrap();
        let pipeline = Pipeline::parse(&text).unwrap();
        assert_eq!(pipeline.entry, "backlog");
        assert_eq!(pipeline.stages.len(), 6);
        assert!(pipeline.stage("ready_for_development").unwrap().gate.is_some());
        assert!(pipeline.stage("in_review").unwrap().watches_prs());
        assert!(pipeline.stage("in_development").unwrap().develops());
    }
}
