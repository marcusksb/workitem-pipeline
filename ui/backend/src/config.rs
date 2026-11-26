use std::{
    fs, io,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::pipeline::Watch;

pub const DEFAULT_PORT: u16 = 7373;

/// The agent CLIs sessions can run (decision 56). Each kind has its own arguments and hook glue.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    #[default]
    Claude,
    Opencode,
    Pi,
    Omp,
}

impl AgentKind {
    pub const ALL: [AgentKind; 4] = [AgentKind::Claude, AgentKind::Opencode, AgentKind::Pi, AgentKind::Omp];

    pub fn name(self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Opencode => "opencode",
            AgentKind::Pi => "pi",
            AgentKind::Omp => "omp",
        }
    }
}

/// The agent that runs sessions: its kind and the executable (a name on `PATH` or an absolute
/// path), which defaults to the kind's own name, or to `AppState::claude` for Claude.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "AgentSetting")]
pub struct Agent {
    pub kind: AgentKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

/// An older file holds the agent as a bare executable (decision 39), which is a Claude wrapper.
#[derive(Deserialize)]
#[serde(untagged)]
enum AgentSetting {
    Command(String),
    Table {
        #[serde(default)]
        kind: AgentKind,
        #[serde(default)]
        command: Option<String>,
    },
}

impl From<AgentSetting> for Agent {
    fn from(setting: AgentSetting) -> Agent {
        match setting {
            AgentSetting::Command(command) => Agent { kind: AgentKind::Claude, command: Some(command) },
            AgentSetting::Table { kind, command } => Agent { kind, command },
        }
    }
}

impl Agent {
    /// Trims the command, drops an empty one (the kind's default) and checks that the executable resolves.
    fn checked(self) -> Result<Agent, String> {
        let command = self.command.map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
        let name = command.as_deref().unwrap_or(self.kind.name());
        // Claude without a command runs `AppState::claude`, which the launcher resolves itself.
        if (command.is_some() || self.kind != AgentKind::Claude) && resolve_executable(name).is_none() {
            return Err(format!("'{name}' is not one executable on PATH or an absolute path to one"));
        }
        Ok(Agent { kind: self.kind, command })
    }
}

/// A folder in `<root>/workspaces/` (decision 44): its items, boards, skills, watch scripts and plans.
#[derive(Clone, Serialize)]
pub struct Workspace {
    /// The folder name.
    pub name: String,
    pub dir: PathBuf,
    /// Agent that runs this workspace's sessions instead of the default one (decisions 52, 56),
    /// from `workspace.toml`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<Agent>,
    /// `jira` or `github` (decision 62), from `workspace.toml`; `jira` when it names none.
    pub issue_tracker: String,
    /// The gh login whose token every process of the workspace gets (decision 67); gh's active
    /// account when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_account: Option<String>,
    /// The bkt context every bkt call of the workspace gets `--context` (decision 82); bkt's
    /// active context when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bkt_context: Option<String>,
    /// Scripts that run once for the whole workspace on a timer (`watch.rs`), from `workspace.toml`.
    #[serde(skip)]
    pub watch: Vec<Watch>,
    /// Why `workspace.toml` is not in use, if so; its last valid version is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Config {
    token: String,
    #[serde(default = "default_port")]
    port: u16,
    /// Agent of every workspace without its own (decision 56).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent: Option<Agent>,
}

/// `<workspace>/workspace.toml`.
#[derive(Clone, Default, Deserialize)]
struct Settings {
    #[serde(default)]
    agent: Option<Agent>,
    #[serde(default)]
    issue_tracker: Option<String>,
    #[serde(default)]
    github_account: Option<String>,
    #[serde(default)]
    bkt_context: Option<String>,
    /// `[[watch]]` tables (or one `[watch]`), each with `every`, `command` and optional `args`.
    #[serde(default, deserialize_with = "crate::pipeline::watchers")]
    watch: Vec<Watch>,
}

impl Settings {
    fn parse(text: &str) -> Result<Settings, String> {
        let settings: Settings = toml::from_str(text).map_err(|e| e.message().to_string())?;
        if let Some(watch) = settings.watch.iter().find(|w| !crate::watch::valid_name(&w.command)) {
            return Err(format!("watch command '{}' is not a script name (lowercase letters, digits, . _ - only)", watch.command));
        }
        if let Some(tracker) = settings.issue_tracker.as_deref().filter(|t| !ISSUE_TRACKERS.contains(t)) {
            return Err(format!("issue_tracker '{tracker}' is not one of {}", ISSUE_TRACKERS.join(", ")));
        }
        if let Some(login) = settings.github_account.as_deref().filter(|l| !valid_login(l)) {
            return Err(format!("github_account '{login}' is not a GitHub login"));
        }
        if let Some(context) = settings.bkt_context.as_deref().filter(|c| !valid_name(c)) {
            return Err(format!("bkt_context '{context}' is not a bkt context name"));
        }
        Ok(settings)
    }
}

/// What `issue_tracker` can name; the first is the default.
pub const ISSUE_TRACKERS: &[&str] = &["jira", "github"];

/// A GitHub login: letters, digits and single hyphens, as GitHub allows them.
pub fn valid_login(login: &str) -> bool {
    !login.is_empty() && login.len() <= 39 && !login.starts_with('-') && login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

fn new_token() -> String {
    let bytes: [u8; 32] = rand::rng().random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Workspace and board names become folder and file names, so only plain ones are accepted.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub struct Store {
    root: PathBuf,
    config: Mutex<Config>,
    /// Last valid `workspace.toml` per workspace, so a typo in it stops nothing.
    settings: Mutex<HashMap<String, Settings>>,
}

impl Store {
    /// `WORKITEM_UI_HOME`, else `~/.workitems-pipeline`.
    pub fn default_root() -> PathBuf {
        if let Some(path) = std::env::var_os("WORKITEM_UI_HOME") {
            return path.into();
        }
        let home = std::env::var_os("HOME").expect("HOME is not set");
        Path::new(&home).join(".workitems-pipeline")
    }

    /// The config file of the UI before workspaces, whose token and port a first start takes over.
    pub fn legacy_config() -> Option<PathBuf> {
        Some(Path::new(&std::env::var_os("HOME")?).join(".config/workitem-ui/config.toml"))
    }

    /// Loads `<root>/config.toml`, creating it on first run with the token and port of `legacy`
    /// when that file exists, else a fresh token.
    pub fn open(root: PathBuf, legacy: Option<&Path>) -> io::Result<Store> {
        let path = root.join("config.toml");
        let read = |path: &Path| -> io::Result<Config> {
            let text = fs::read_to_string(path)?;
            toml::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{}: {e}", path.display())))
        };
        let config = match read(&path) {
            Ok(config) => config,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let config = legacy
                    .and_then(|old| read(old).ok())
                    .map(|old| Config { agent: None, ..old })
                    .unwrap_or_else(|| Config { token: new_token(), port: DEFAULT_PORT, agent: None });
                save(&path, &config)?;
                config
            }
            Err(e) => return Err(e),
        };
        fs::create_dir_all(root.join("workspaces"))?;
        Ok(Store { root, config: Mutex::new(config), settings: Default::default() })
    }

    /// The config file, which holds the token.
    pub fn path(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    /// A file in the root, for state the backend keeps.
    pub fn sibling(&self, file: &str) -> PathBuf {
        self.root.join(file)
    }

    pub fn token(&self) -> String {
        self.config.lock().unwrap().token.clone()
    }

    pub fn port(&self) -> u16 {
        self.config.lock().unwrap().port
    }

    /// Reads the workspace. When its `workspace.toml` is invalid the last valid one stays in use
    /// and `error` says why; no file is an empty one.
    fn load(&self, name: &str) -> Workspace {
        let dir = self.root.join("workspaces").join(name);
        let path = dir.join("workspace.toml");
        let read = match fs::read_to_string(&path) {
            Ok(text) => Settings::parse(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        };
        let mut known = self.settings.lock().unwrap();
        let (settings, error) = match read {
            Ok(settings) => {
                known.insert(name.to_string(), settings.clone());
                (settings, None)
            }
            Err(error) => {
                eprintln!("workitem-ui: {name}: {error}");
                (known.get(name).cloned().unwrap_or_default(), Some(error))
            }
        };
        Workspace {
            name: name.to_string(),
            dir,
            agent: settings.agent,
            issue_tracker: settings.issue_tracker.unwrap_or_else(|| ISSUE_TRACKERS[0].into()),
            github_account: settings.github_account,
            bkt_context: settings.bkt_context,
            watch: settings.watch,
            error,
        }
    }

    /// Every folder in `workspaces/` with a plain name, by name.
    pub fn workspaces(&self) -> Vec<Workspace> {
        let entries = fs::read_dir(self.root.join("workspaces")).into_iter().flatten().flatten();
        let mut names: Vec<String> = entries
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| e.file_name().to_str().map(String::from))
            .filter(|name| valid_name(name))
            .collect();
        names.sort();
        names.iter().map(|name| self.load(name)).collect()
    }

    pub fn workspace(&self, name: &str) -> Option<Workspace> {
        (valid_name(name) && self.root.join("workspaces").join(name).is_dir()).then(|| self.load(name))
    }

    pub fn rotate_token(&self) -> io::Result<String> {
        let mut config = self.config.lock().unwrap();
        let mut next = config.clone();
        next.token = new_token();
        save(&self.path(), &next)?;
        *config = next;
        Ok(config.token.clone())
    }

    /// Creates an empty workspace folder.
    pub fn add_workspace(&self, name: &str) -> Result<Workspace, String> {
        let name = name.trim();
        if !valid_name(name) {
            return Err("a workspace name uses letters, digits, - and _ only".into());
        }
        let dir = self.root.join("workspaces").join(name);
        if dir.exists() {
            return Err(format!("workspace '{name}' already exists"));
        }
        fs::create_dir_all(dir.join("boards")).map_err(|e| e.to_string())?;
        Ok(self.load(name))
    }

    /// The agent of every workspace without its own.
    pub fn default_agent(&self) -> Option<Agent> {
        self.config.lock().unwrap().agent.clone()
    }

    /// The agent that runs `workspace`'s sessions: its own, else the default one, else Claude.
    pub fn agent_for(&self, workspace: &Workspace) -> Agent {
        workspace.agent.clone().or_else(|| self.default_agent()).unwrap_or_default()
    }

    /// Sets or, with `None`, clears the default agent.
    pub fn set_default_agent(&self, agent: Option<Agent>) -> Result<Option<Agent>, String> {
        let agent = agent.map(Agent::checked).transpose()?;
        let mut config = self.config.lock().unwrap();
        let next = Config { agent: agent.clone(), ..config.clone() };
        save(&self.path(), &next).map_err(|e| e.to_string())?;
        *config = next;
        Ok(agent)
    }

    /// Sets or, with `None`, clears the workspace's agent. `None` when there is no such workspace.
    pub fn set_agent(&self, name: &str, agent: Option<Agent>) -> Result<Option<Workspace>, String> {
        self.update(name, SettingsChange { agent: Some(agent), ..Default::default() })
    }

    /// Applies the given settings to `workspace.toml` in one write, after checking all of them, so
    /// a bad one changes nothing. An empty value (or `Some(None)` for the agent) removes its key.
    /// Only these keys change; the rest of the file (its watchers) stays, though not its comments.
    /// `None` when there is no such workspace. Whether gh knows a GitHub account is the caller's
    /// check (`github.rs`); whether bkt knows the context is the caller's too (`bkt.rs`).
    pub fn update(&self, name: &str, change: SettingsChange) -> Result<Option<Workspace>, String> {
        let (tracker, login) = (change.issue_tracker.map(str::trim), change.github_account.map(str::trim));
        let context = change.bkt_context.map(str::trim);
        let agent = change.agent.map(|agent| agent.map(Agent::checked).transpose()).transpose()?;
        if let Some(tracker) = tracker.filter(|t| !t.is_empty() && !ISSUE_TRACKERS.contains(t)) {
            return Err(format!("issue tracker '{tracker}' is not one of {}", ISSUE_TRACKERS.join(", ")));
        }
        if let Some(login) = login.filter(|l| !l.is_empty() && !valid_login(l)) {
            return Err(format!("'{login}' is not a GitHub login"));
        }
        if let Some(context) = context.filter(|c| !c.is_empty() && !valid_name(c)) {
            return Err(format!("'{context}' is not a bkt context name"));
        }
        let Some(workspace) = self.workspace(name) else {
            return Ok(None);
        };
        if let Some(error) = workspace.error {
            return Err(format!("fix workspace.toml first: {error}"));
        }
        let path = workspace.dir.join("workspace.toml");
        let mut table: toml::Table = fs::read_to_string(&path).ok().map(|t| t.parse()).transpose().map_err(|e: toml::de::Error| e.to_string())?.unwrap_or_default();
        match agent {
            Some(None) => drop(table.remove("agent")),
            Some(Some(agent)) => drop(table.insert("agent".into(), toml::Value::try_from(agent).map_err(|e| e.to_string())?)),
            None => {}
        }
        for (key, value) in [("issue_tracker", tracker), ("github_account", login), ("bkt_context", context)] {
            match value {
                Some("") => drop(table.remove(key)),
                Some(value) => drop(table.insert(key.into(), value.into())),
                None => {}
            }
        }
        fs::write(&path, toml::to_string_pretty(&table).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        Ok(Some(self.load(name)))
    }
}

/// What `Store::update` changes; a `None` key stays as it is.
#[derive(Default)]
pub struct SettingsChange<'a> {
    pub agent: Option<Option<Agent>>,
    pub issue_tracker: Option<&'a str>,
    pub github_account: Option<&'a str>,
    pub bkt_context: Option<&'a str>,
}

/// Absolute path of executable `name`: an absolute path as is, a bare name looked up on `PATH`.
/// `None` for anything else (relative paths, whitespace) or when it is not an executable file.
pub fn resolve_executable(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let executable = |path: &Path| fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
    if name.is_empty() || name.chars().any(char::is_whitespace) {
        return None;
    }
    if name.contains('/') {
        let path = PathBuf::from(name);
        return (path.is_absolute() && executable(&path)).then_some(path);
    }
    std::env::split_paths(&std::env::var_os("PATH")?).map(|dir| dir.join(name)).find(|path| path.is_absolute() && executable(path))
}

/// Writes atomically with mode 0600; the file holds the token.
fn save(path: &Path, config: &Config) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(config).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("toml.tmp");
    let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
    file.write_all(text.as_bytes())?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_token_file_with_private_mode_and_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("sub");
        let store = Store::open(root.clone(), None).unwrap();
        let mode = fs::metadata(root.join("config.toml")).unwrap().permissions();
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&mode) & 0o777, 0o600);
        assert_eq!(store.token().len(), 64);
        assert_eq!(Store::open(root, None).unwrap().token(), store.token());
    }

    #[test]
    fn a_first_start_takes_over_the_old_token_and_port() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old.toml");
        fs::write(&old, "token = \"abc\"\nport = 7400\n\n[[projects]]\nname = \"x\"\nrepo = \"/x\"\n").unwrap();
        let store = Store::open(dir.path().join("root"), Some(&old)).unwrap();
        assert_eq!((store.token().as_str(), store.port()), ("abc", 7400));
        assert!(!fs::read_to_string(store.path()).unwrap().contains("projects"));
    }

    #[test]
    fn workspaces_are_the_folders_with_plain_names() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf(), None).unwrap();
        store.add_workspace("web").unwrap();
        store.add_workspace(" api ").unwrap();
        assert!(store.add_workspace("api").is_err());
        for bad in ["", "a b", "../x", "a/b"] {
            assert!(store.add_workspace(bad).is_err(), "{bad}");
        }
        fs::create_dir_all(dir.path().join("workspaces/.hidden")).unwrap();
        fs::write(dir.path().join("workspaces/file"), "").unwrap();
        assert_eq!(store.workspaces().iter().map(|w| w.name.as_str()).collect::<Vec<_>>(), ["api", "web"]);
        assert!(store.workspace("api").unwrap().dir.join("boards").is_dir());
        assert!(store.workspace("nope").is_none());
        assert!(store.workspace("..").is_none());
    }

    fn claude(command: &str) -> Option<Agent> {
        Some(Agent { kind: AgentKind::Claude, command: Some(command.into()) })
    }

    #[test]
    fn agent_persists_clears_and_rejects_non_executables() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf(), None).unwrap();
        store.add_workspace("app").unwrap();
        assert_eq!(store.set_agent("app", claude(" /bin/sh ")).unwrap().unwrap().agent, claude("/bin/sh"));
        assert_eq!(Store::open(dir.path().to_path_buf(), None).unwrap().workspace("app").unwrap().agent, claude("/bin/sh"));
        assert!(store.set_agent("app", claude("sh")).is_ok(), "a bare name is looked up on PATH");
        for bad in ["sh -c", "bin/sh", "/no/such/agent", "/etc/hosts"] {
            assert!(store.set_agent("app", claude(bad)).is_err(), "{bad}");
        }
        let missing = Agent { kind: AgentKind::Pi, command: Some("no-such-pi".into()) };
        assert!(store.set_agent("app", Some(missing)).is_err());
        let blank = Agent { kind: AgentKind::Claude, command: Some(" ".into()) };
        assert_eq!(store.set_agent("app", Some(blank)).unwrap().unwrap().agent, Some(Agent::default()), "claude may run AppState::claude");
        assert!(store.set_agent("nope", claude("sh")).unwrap().is_none());
        assert_eq!(store.set_agent("app", None).unwrap().unwrap().agent, None);
        assert!(!fs::read_to_string(dir.path().join("workspaces/app/workspace.toml")).unwrap().contains("agent"));
    }

    #[test]
    fn the_default_agent_applies_to_workspaces_without_their_own() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf(), None).unwrap();
        let app = store.add_workspace("app").unwrap();
        assert_eq!(store.agent_for(&app), Agent::default());
        let omp = Agent { kind: AgentKind::Omp, command: Some("/bin/sh".into()) };
        store.set_default_agent(Some(omp.clone())).unwrap();
        let reopened = Store::open(dir.path().to_path_buf(), None).unwrap();
        assert_eq!(reopened.agent_for(&reopened.workspace("app").unwrap()), omp);
        let own = store.set_agent("app", claude("/bin/sh")).unwrap().unwrap();
        assert_eq!(store.agent_for(&own), claude("/bin/sh").unwrap());
        store.set_default_agent(None).unwrap();
        assert_eq!(Store::open(dir.path().to_path_buf(), None).unwrap().default_agent(), None);
    }

    #[test]
    fn a_bare_agent_string_reads_as_a_claude_wrapper() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf(), None).unwrap();
        let file = store.add_workspace("app").unwrap().dir.join("workspace.toml");
        fs::write(&file, "agent = \"yolo\"\n").unwrap();
        assert_eq!(store.workspace("app").unwrap().agent, claude("yolo"));
        store.set_agent("app", Some(Agent { kind: AgentKind::Omp, command: Some("/bin/sh".into()) })).unwrap();
        assert!(fs::read_to_string(&file).unwrap().contains("kind = \"omp\""), "written as a table");
    }

    #[test]
    fn watchers_come_from_the_last_valid_file_and_survive_an_agent_change() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf(), None).unwrap();
        let file = store.add_workspace("app").unwrap().dir.join("workspace.toml");
        assert!(store.workspace("app").unwrap().watch.is_empty());
        fs::write(&file, "[[watch]]\nevery = \"5m\"\ncommand = \"issue-status\"\n").unwrap();
        let watch = |store: &Store| store.workspace("app").unwrap().watch.iter().map(|w| (w.command.clone(), w.every.as_secs())).collect::<Vec<_>>();
        assert_eq!(watch(&store), [("issue-status".to_string(), 300)]);
        store.set_agent("app", claude("/bin/sh")).unwrap();
        assert_eq!(watch(&store), [("issue-status".to_string(), 300)], "setting the agent keeps the watchers");
        for bad in ["[[watch]]\nevery = \"5x\"\ncommand = \"a\"\n", "[[watch]]\nevery = \"5m\"\ncommand = \"../a\"\n", "agent = "] {
            fs::write(&file, bad).unwrap();
            let workspace = store.workspace("app").unwrap();
            assert!(workspace.error.is_some(), "{bad}");
            assert_eq!((watch(&store), workspace.agent), (vec![("issue-status".to_string(), 300)], claude("/bin/sh")), "{bad}: the last valid file stays");
            assert!(store.set_agent("app", None).err().unwrap().contains("workspace.toml"), "an invalid file is not overwritten");
        }
        let fresh = Store::open(dir.path().to_path_buf(), None).unwrap().workspace("app").unwrap();
        assert!(fresh.error.is_some() && fresh.watch.is_empty(), "nothing valid was ever read");
    }

    #[test]
    fn issue_tracker_and_github_account_default_persist_and_are_checked_before_anything_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf(), None).unwrap();
        store.add_workspace("app").unwrap();
        let app = store.workspace("app").unwrap();
        assert_eq!((app.issue_tracker.as_str(), app.github_account, app.bkt_context), ("jira", None, None));
        store.set_agent("app", claude("/bin/sh")).unwrap();
        let change = |tracker, login, context| SettingsChange {
            issue_tracker: Some(tracker), github_account: Some(login), bkt_context: Some(context), ..Default::default()
        };
        let app = store.update("app", change("github", " work-bot ", " work ")).unwrap().unwrap();
        assert_eq!((app.issue_tracker.as_str(), app.github_account.as_deref(), app.bkt_context.as_deref(), app.agent),
                   ("github", Some("work-bot"), Some("work"), claude("/bin/sh")), "other keys stay");
        assert!(store.update("app", change("jira", "a b", "work")).is_err());
        assert_eq!(store.workspace("app").unwrap().issue_tracker, "github", "a bad key changes nothing");
        assert!(store.update("app", change("gitlab", "", "a b")).is_err());
        let app = store.update("app", change("", "", "")).unwrap().unwrap();
        assert_eq!((app.issue_tracker.as_str(), app.github_account, app.bkt_context), ("jira", None, None), "empty goes back to the defaults");
        let file = dir.path().join("workspaces/app/workspace.toml");
        fs::write(&file, "issue_tracker = \"linear\"\n").unwrap();
        assert!(store.workspace("app").unwrap().error.unwrap().contains("issue_tracker 'linear'"));
    }

    #[test]
    fn rotate_changes_token() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf(), None).unwrap();
        let before = store.token();
        assert_ne!(store.rotate_token().unwrap(), before);
    }
}
