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

/// A folder in `<root>/workspaces/` (decision 44): its items, boards, skills, watch scripts and plans.
#[derive(Clone, Serialize)]
pub struct Workspace {
    /// The folder name.
    pub name: String,
    pub dir: PathBuf,
    /// Executable that runs this workspace's sessions instead of `AppState::claude` (decision 39),
    /// from `workspace.toml`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
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
}

/// `<workspace>/workspace.toml`.
#[derive(Clone, Default, Deserialize)]
struct Settings {
    #[serde(default)]
    agent: Option<String>,
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
        Ok(settings)
    }
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
                    .unwrap_or_else(|| Config { token: new_token(), port: DEFAULT_PORT });
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
        Workspace { name: name.to_string(), dir, agent: settings.agent, watch: settings.watch, error }
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

    /// Sets or, with an empty `agent`, clears the workspace's agent. `None` when there is no such workspace.
    pub fn set_agent(&self, name: &str, agent: &str) -> Result<Option<Workspace>, String> {
        let agent = agent.trim();
        if !agent.is_empty() && resolve_executable(agent).is_none() {
            return Err(format!("'{agent}' is not one executable on PATH or an absolute path to one"));
        }
        let Some(workspace) = self.workspace(name) else {
            return Ok(None);
        };
        if let Some(error) = workspace.error {
            return Err(format!("fix workspace.toml first: {error}"));
        }
        // Only `agent` changes; the rest of the file (its watchers) stays, though not its comments.
        let path = workspace.dir.join("workspace.toml");
        let mut table: toml::Table = fs::read_to_string(&path).ok().map(|t| t.parse()).transpose().map_err(|e: toml::de::Error| e.to_string())?.unwrap_or_default();
        if agent.is_empty() {
            table.remove("agent");
        } else {
            table.insert("agent".into(), agent.into());
        }
        fs::write(&path, toml::to_string_pretty(&table).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        Ok(Some(self.load(name)))
    }
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

    #[test]
    fn agent_persists_clears_and_rejects_non_executables() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf(), None).unwrap();
        store.add_workspace("app").unwrap();
        assert_eq!(store.set_agent("app", " /bin/sh ").unwrap().unwrap().agent.as_deref(), Some("/bin/sh"));
        assert_eq!(Store::open(dir.path().to_path_buf(), None).unwrap().workspace("app").unwrap().agent.as_deref(), Some("/bin/sh"));
        assert!(store.set_agent("app", "sh").is_ok(), "a bare name is looked up on PATH");
        for bad in ["sh -c", "bin/sh", "/no/such/agent", "/etc/hosts"] {
            assert!(store.set_agent("app", bad).is_err(), "{bad}");
        }
        assert!(store.set_agent("nope", "sh").unwrap().is_none());
        assert_eq!(store.set_agent("app", "").unwrap().unwrap().agent, None);
        assert!(!fs::read_to_string(dir.path().join("workspaces/app/workspace.toml")).unwrap().contains("agent"));
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
        store.set_agent("app", "/bin/sh").unwrap();
        assert_eq!(watch(&store), [("issue-status".to_string(), 300)], "setting the agent keeps the watchers");
        for bad in ["[[watch]]\nevery = \"5x\"\ncommand = \"a\"\n", "[[watch]]\nevery = \"5m\"\ncommand = \"../a\"\n", "agent = "] {
            fs::write(&file, bad).unwrap();
            let workspace = store.workspace("app").unwrap();
            assert!(workspace.error.is_some(), "{bad}");
            assert_eq!((watch(&store), workspace.agent.as_deref()), (vec![("issue-status".to_string(), 300)], Some("/bin/sh")), "{bad}: the last valid file stays");
            assert!(store.set_agent("app", "").err().unwrap().contains("workspace.toml"), "an invalid file is not overwritten");
        }
        let fresh = Store::open(dir.path().to_path_buf(), None).unwrap().workspace("app").unwrap();
        assert!(fresh.error.is_some() && fresh.watch.is_empty(), "nothing valid was ever read");
    }

    #[test]
    fn rotate_changes_token() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf(), None).unwrap();
        let before = store.token();
        assert_ne!(store.rotate_token().unwrap(), before);
    }
}
